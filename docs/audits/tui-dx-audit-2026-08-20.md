# Jet TUI DX audit — 2026-08-20

Status: source-only partial audit. No build, Jet binary, test, formatter, Tower
CLI, board write, or commit ran in this worker. Scores below are source scores;
live captures are pending orchestrator proof.

## Executive result

Jet has several strong local interaction mechanisms: the REPL has raw-mode
editing, multiline input, completion, folds, pins, history search, and a
bindings pane; diagnostics have one structured renderer and shared color policy;
`jet ?` has a TTY app plus a non-TTY fallback. The system is not yet omp-grade
as a family because terminal lifecycle, progress, resize, and disclosure rules
are not one shared contract.

Largest gaps:

1. P1 — terminal lifecycle contract is split. REPL uses raw ANSI; `jet inspect
   live` clears with literal ANSI; dev/build/test mostly print lines. Resize and
   cancellation behavior differ by surface.
2. P1 — status/progress is split between `watching`, `[watch]`, `[build]`, test
   result lines, timing lines, and ad-hoc notices. No common event grammar or
   first-paint contract.
3. P1 — interactive discovery is uneven. REPL advertises most keys, but debug,
   help, notebook, and live inspect have separate prompt/control vocabularies.
4. P2 — progressive disclosure is strong in REPL but weak in diagnostics,
   explain/inspect, test, and build output. Long reports are text streams, not
   summaries with drill-down.
5. P2 — Canvas is a browser/dev-server surface, not a terminal surface. The
   requested “canvas/scene CLI” has no identified terminal entry point; this is
   an inventory gap, not proof of a missing feature.
6. P2 — non-TTY behavior is intentionally present but not demonstrated for all
   surfaces. `jet ?` has an explicit floor; REPL has a cooked floor; dev/build/
   test/inspect need transcript proof under pipes and narrow columns.

Jet wins where one mechanism already exists: diagnostics carry What/Why/Fix and
machine JSON through one renderer, and the REPL's raw-mode code centralizes key
decoding. Jet loses on cross-surface consistency and live proof.

## Scope and inventory

| Surface | Current owner | Inventory result |
|---|---|---|
| REPL | `crates/jet-repl/src/Interactive.rs`, `Render.rs`, `Term.rs`, `History.rs` | covered; rich TTY UI plus cooked fallback |
| Dev loop | `Source/CmdDevTools.rs`, `jet-devserver` | covered; watch loop, hot replacement, timing, Canvas URL |
| Live inspect | `Source/main.rs:1512-1540` | covered; polling text loop nested under inspect |
| Test runner | `Source/CmdCompile.rs:1858-2288` | covered; per-target result lines and shared diagnostics |
| Build/run progress | `Source/CmdCompile.rs:292-565`, `4553-4685` | covered; verbose `[build]` steps, cache, timing |
| Diagnostics | `Source/main.rs:3292-3320`, `crates/jet-foundation/src/Diagnostics.rs:869-900` | covered; human, linked human, JSON |
| Explain/inspect reports | `Source/CmdDevTools.rs:1704-1819`, `Source/CmdDossier.rs:84-150` | covered; text and JSON reports |
| Canvas/scene | `crates/jet-devserver/src/WebHost.rs:565-572`, `889-1129`, `docs/reference/canvas-protocol.md` | covered as web/dev-server; no terminal CLI found |
| `jet ?` | `Source/main.rs:1040-1076` | covered; TTY app and non-TTY palette |
| `jet debug` | `Source/main.rs:2207-2242`, `crates/jet-debug/src/Native.rs:188-220` | covered; `(jet)` prompt, native backend, DAP escape hatch |
| `jet notebook` terminal client | `Source/CmdNotebook.rs`, `crates/jet-repl/src/Notebook` | covered as server/browser client; terminal client path not identified |
| bare `jet` / usage | `Source/main.rs:455-462`, `crates/jet-foundation/src/CLISchema.rs` | covered at dispatcher; runtime capture pending |

The inventory includes all seven named surfaces and all five candidate surfaces
from the card. “Terminal client” means the terminal-facing path only; the
notebook HTML client and Canvas browser UI are recorded as adjacent owners, not
silently counted as TUI.

## Capture protocol

The required live matrix was not run here. The orchestrator must rebuild first,
then capture each command in a real PTY and a pipe, with normal color,
`NO_COLOR=1`, and `COLUMNS=60`. Save raw output before trimming. For interactive
sessions, also record key input and exit behavior.

Common shell forms:

```sh
scripts/agent/jet-env cargo build
mkdir -p /tmp/jet-tui-captures-2026-08-20
script -qec 'COLUMNS=60 target/debug/jet ?' /tmp/jet-tui-captures-2026-08-20/help-tty.txt
NO_COLOR=1 script -qec 'COLUMNS=60 target/debug/jet ?' /tmp/jet-tui-captures-2026-08-20/help-tty-no-color.txt
target/debug/jet ? </dev/null > /tmp/jet-tui-captures-2026-08-20/help-pipe.txt 2>&1
NO_COLOR=1 target/debug/jet ? </dev/null > /tmp/jet-tui-captures-2026-08-20/help-pipe-no-color.txt 2>&1
```

Use the same four-way matrix for every non-interactive command below. Use a PTY
helper for key scripts where `script` cannot inject the required sequence.

## Current-state captures

These are capture slots, not fabricated transcripts. Each block names the
exact command family and the missing runtime evidence.

### REPL

Source signal: raw mode requires TTYs and falls back when unavailable
(`crates/jet-repl/src/Term.rs:39-84`, `Interactive.rs:32-58`). The TTY banner
advertises `:quit`, `:help`, and `^B`; the discovery hint advertises completion,
docs, history, pin, fold, rerun, and bindings (`Render.rs:24-60`).

```text
LIVE CAPTURE PENDING
TTY:       jet repl  [keys: multiline, Tab, F1, F3, ^B, ^P, ^F, ^R, Esc, ^C, ^D]
non-TTY:   printf '1 + 1\n' | jet repl
NO_COLOR:  both forms
narrow:    COLUMNS=60
Required checks: prompt wrap, completion redraw, fold marker, pin rail, ^B pane,
F3 cancel/accept, Ctrl-C during evaluation, resize while editing.
```

### Dev loop and live inspect

Source signal: `jet dev` prints a watch banner, polls every 120 ms, and reports
hot replacement and edit-to-visible budget events (`Source/CmdDevTools.rs:89-186`).
`jet inspect live` polls every 250 ms and clears with literal `\x1b[2J\x1b[H`
unless non-TTY/JSON (`Source/main.rs:1520-1539`).

```text
LIVE CAPTURE PENDING
TTY:       jet dev examples/features/basics/hello.jet
non-TTY:   jet dev --watch=off examples/features/basics/hello.jet | sed -n l
NO_COLOR:  repeat both; inspect live <pid> --once and polling mode
narrow:    COLUMNS=60 jet dev examples/features/basics/hello.jet
Required checks: first paint, edit-to-visible line, Ctrl-C, invalid edit recovery,
inspect redraw without a TTY, JSON/pipe termination, and narrow status line.
```

### Test runner

Source signal: directory tests are sorted and run sequentially
(`Source/CmdCompile.rs:1858-1889`); individual doctest failures print
`FAIL (does not compile)` or `FAIL (runtime error)`, then a pass/FAIL line
(`Source/CmdCompile.rs:2210-2287`).

```text
LIVE CAPTURE PENDING
TTY:       jet test examples/features/basics/hello.jet
non-TTY:   jet test examples/features/basics/hello.jet | sed -n l
NO_COLOR:  repeat both
narrow:    COLUMNS=60 jet test examples/features/basics/hello.jet
Required checks: pass/fail ordering, compile/runtime failure context, multi-file
progress, color bytes, and behavior when a test waits for input.
```

### Build/run progress

Source signal: `jet run` can print a verbose cache-hit status
(`Source/CmdCompile.rs:547-565`); verbose build emits deterministic `[build]`
steps (`Source/CmdCompile.rs:4553-4567`), while timing emits separate
`jet-timing` lines (`Source/CmdCompile.rs:4670-4675`).

```text
LIVE CAPTURE PENDING
TTY:       jet build -v examples/features/basics/hello.jet
non-TTY:   jet build -v examples/features/basics/hello.jet | sed -n l
NO_COLOR:  repeat both; include jet run and a cache hit
narrow:    COLUMNS=60 jet build -v examples/features/basics/hello.jet
Required checks: first visible progress, cache-hit distinction, failure handoff,
timing placement, stdout/stderr ordering, and line wrapping.
```

### Diagnostics

Source signal: human diagnostics use linked rendering and append a count plus an
`explain` pointer (`Source/main.rs:3292-3320`). The renderer has a color-aware
batch path and OSC 8 hyperlinks (`crates/jet-foundation/src/Diagnostics.rs:869-900`).
Color resolution is centralized (`crates/jet-foundation/src/Terminal.rs:26-36`,
`Source/main.rs:99-115`).

```text
LIVE CAPTURE PENDING
TTY:       jet check tests/ui/<known-failing-fixture>.jet
non-TTY:   jet check tests/ui/<known-failing-fixture>.jet | sed -n l
NO_COLOR:  repeat both; compare absence of ESC bytes
narrow:    COLUMNS=60 jet check tests/ui/<known-failing-fixture>.jet
Required checks: source frame, What/Why/Fix order, pointer line, hyperlink policy,
multiple diagnostics, and narrow-column degradation.
```

### Explain and inspect reports

Source signal: `jet explain` chooses text or JSON, with the same `Explain::render`
writer for diagnostic essays and policy markers (`Source/CmdDevTools.rs:1704-1819`,
`1876-1896`). Dossier text prints labels and provenance rows, with a JSON path
(`Source/CmdDossier.rs:84-150`).

```text
LIVE CAPTURE PENDING
TTY:       jet explain E2104; jet inspect dossier examples/features/basics/hello.jet run
non-TTY:   jet explain E2104 | sed -n l
NO_COLOR:  repeat text forms; include --json
narrow:    COLUMNS=60 jet inspect dossier examples/features/basics/hello.jet run
Required checks: summary before detail, long provenance, unknown-code recovery,
JSON stability, and narrow output.
```

### Canvas/scene surface

Source signal: the dev server announces a Canvas URL (`WebHost.rs:565-572`);
graph, project, command, transaction, and debug routes are HTTP endpoints
(`WebHost.rs:889-1129`). The protocol document defines source-backed graph and
transaction state (`docs/reference/canvas-protocol.md`). No terminal command or
terminal renderer was found in the inspected entry points.

```text
LIVE CAPTURE PENDING / INVENTORY BLOCKED
TTY candidate: jet dev examples/features/basics/hello.jet
Non-TTY candidate: same command piped through sed -n l
Required check: confirm whether “canvas/scene CLI” means the dev-server's
terminal status strip, a missing CLI command, or an out-of-scope browser client.
```

### `jet ?` interactive help

Source signal: TTY opens `Help::Interactive`; query and non-TTY paths print once
(`Source/main.rs:1040-1076`).

```text
LIVE CAPTURE PENDING
TTY:       jet ?  [query, arrows, Enter, Esc, Ctrl-C, shell prefill]
non-TTY:   jet ?; jet ? build
NO_COLOR:  repeat both
narrow:    COLUMNS=60 jet ?
Required checks: query-to-selection path, selected command stdout contract,
non-TTY width, and Esc/Ctrl-C exit.
```

### `jet debug`

Source signal: one command selects interpreter or native backend and exposes a
`(jet)` prompt (`Source/main.rs:2207-2242`; native prompt
`crates/jet-debug/src/Native.rs:199-220`). Native mode reports missing `lldb`
and points to the interpreter path (`Native.rs:105-118`).

```text
LIVE CAPTURE PENDING
TTY:       jet debug examples/features/basics/hello.jet  [step, next, continue, quit]
non-TTY:   scripted debug input through the interpreter backend
NO_COLOR:  repeat TTY and scripted forms
narrow:    COLUMNS=60 jet debug examples/features/basics/hello.jet
Required checks: prompt discovery, command errors, Ctrl-C/EOF, backend boundary,
and Jet terms versus raw lldb frames.
```

### `jet notebook` terminal client

Source signal: `jet notebook` starts an HTTP notebook server and prints its URL
(`Source/CmdNotebook.rs:145-175`); the shipped client is HTML and has actions for
run/inspect/debug/profile, stdin, interrupt, import/export, and offline drafts
(`crates/jet-repl/src/Notebook/client.html:82-121`). No separate terminal client
was identified.

```text
LIVE CAPTURE PENDING / INVENTORY BLOCKED
TTY:       jet notebook [path]
non-TTY:   jet notebook --protocol < scripted protocol messages
NO_COLOR:  terminal launcher/status only
narrow:    COLUMNS=60 jet notebook [path]
Required check: confirm whether the protocol client is the intended terminal
surface; otherwise remove “terminal client” from the TUI inventory and audit the
HTML client elsewhere.
```

### bare `jet` / usage

Source signal: bare usage is generated through `jet::CLI::usage_page`
(`Source/main.rs:455-462`), while command help uses the same CLI tables
(`Source/main.rs:459-475`).

```text
LIVE CAPTURE PENDING
TTY:       jet
non-TTY:   jet | sed -n l
NO_COLOR:  repeat both
narrow:    COLUMNS=60 jet
Required checks: greeting versus usage, next-step affordance, color bytes,
line wrapping, and exit code.
```

## Scorecard

Static score: `3` source-complete mechanism, `2` partial/locally complete, `1`
gap or ad-hoc path. `U` means live behavior remains unverified. Evidence is a
source pointer, not a runtime claim.

| Surface | Pattern | Score | Evidence |
|---|---|---:|---|
| REPL | responsiveness | 2U | byte reader has idle polling; evaluation path is synchronous (`Term.rs:281-285`, `Interactive.rs:32-58`) |
| REPL | keyboard model | 3U | decoded keys and discovery hint cover editing, docs, history, folds, pins (`Term.rs:226-260`, `Render.rs:39-60`) |
| REPL | progressive disclosure | 3U | folds, pin rail, bindings pane, completion menu (`Render.rs:92-180`) |
| REPL | color / NO_COLOR | 2U | Theme roles used, but interactive caller receives a bool and needs matrix proof (`Interactive.rs:18-32`, `Render.rs:27-69`) |
| REPL | resize | 2U | width-aware geometry and `stty size`; no SIGWINCH path found (`Interactive.rs:725-893`, `Term.rs:368-385`) |
| Dev loop | responsiveness | 3U | 120 ms poll and edit-to-visible budget (`CmdDevTools.rs:125-186`) |
| Dev loop | keyboard model | 1U | Ctrl-C is documented; no shared interactive key help beyond banner (`CmdDevTools.rs:89-90`) |
| Dev loop | progressive disclosure | 2U | watch, restart, hot-replace, and timing notices, no drill-down (`CmdDevTools.rs:131-184`, `673-694`) |
| Dev loop | color / NO_COLOR | 2U | diagnostics use shared renderer; status strings have separate paths (`CmdDevTools.rs:108-110`, `180-184`) |
| Dev loop | resize | 1U | no redraw/resize contract in loop (`CmdDevTools.rs:125-186`) |
| Live inspect | responsiveness | 2U | fixed 250 ms poll, but full clear/redraw (`main.rs:1523-1539`) |
| Live inspect | keyboard model | 1U | no cancel/help key handling in polling loop (`main.rs:1523-1539`) |
| Live inspect | progressive disclosure | 2U | human render or JSON, no interactive drill-down (`main.rs:1528-1537`) |
| Live inspect | color / NO_COLOR | 1U | literal clear escape is emitted outside Theme (`main.rs:1531-1534`) |
| Live inspect | resize | 1U | no width query or SIGWINCH handling (`main.rs:1523-1539`) |
| Test runner | responsiveness | 1U | sequential file loop; no progress event/first-paint contract (`CmdCompile.rs:1873-1883`) |
| Test runner | keyboard model | 1U | no interactive controls in runner (`CmdCompile.rs:1858-1889`) |
| Test runner | progressive disclosure | 2U | result summary plus diagnostics, but no per-run summary fold (`CmdCompile.rs:2210-2287`) |
| Test runner | color / NO_COLOR | 2U | diagnostics use OutputMode; result lines are plain (`CmdCompile.rs:2214-2218`, `2284-2287`) |
| Test runner | resize | 1U | no width-aware output (`CmdCompile.rs:2210-2287`) |
| Build/progress | responsiveness | 2U | verbose steps and cache hit exist; default path is mostly batch (`CmdCompile.rs:547-565`, `4553-4567`) |
| Build/progress | keyboard model | 1U | no cancel/help interaction contract (`CmdCompile.rs:4553-4685`) |
| Build/progress | progressive disclosure | 2U | verbose opt-in and timing output; no summary-to-detail model (`CmdCompile.rs:4553-4567`, `4670-4675`) |
| Build/progress | color / NO_COLOR | 2U | messages use stderr but progress does not consistently use Theme (`CmdCompile.rs:4555-4558`) |
| Build/progress | resize | 1U | no width-aware progress rendering (`CmdCompile.rs:4553-4685`) |
| Diagnostics | responsiveness | 2U | batch renderer waits for diagnostic set (`Diagnostics.rs:869-877`) |
| Diagnostics | keyboard model | 1U | no interactive navigation in human renderer (`main.rs:3292-3320`) |
| Diagnostics | progressive disclosure | 2U | What/Why/Fix plus pointer; long cause chains are not interactively folded (`main.rs:3303-3319`) |
| Diagnostics | color / NO_COLOR | 3U | shared ColorChoice, Theme, linked renderer, JSON (`Terminal.rs:26-72`, `Diagnostics.rs:869-900`) |
| Diagnostics | resize | 1U | renderer has no terminal-width input (`Diagnostics.rs:869-877`) |
| Explain/inspect | responsiveness | 2U | direct text/JSON render, no streaming contract (`CmdDevTools.rs:1795-1819`, `CmdDossier.rs:139-150`) |
| Explain/inspect | keyboard model | 1U | report is non-interactive (`CmdDevTools.rs:1704-1750`) |
| Explain/inspect | progressive disclosure | 2U | text has labels/provenance; no fold or summary mode (`CmdDossier.rs:69-80`) |
| Explain/inspect | color / NO_COLOR | 2U | Explain resolves stdout color; dossier text path needs capture proof (`CmdDevTools.rs:1817-1819`) |
| Explain/inspect | resize | 1U | no width-aware report renderer (`CmdDossier.rs:69-80`) |
| Canvas/scene | responsiveness | 2U | server has live reload poll; terminal status is not a TUI (`WebHost.rs:550-573`) |
| Canvas/scene | keyboard model | 1U | no terminal client entry point found (`WebHost.rs:889-1129`) |
| Canvas/scene | progressive disclosure | 3U | graph/project/query/debug endpoints separate views (`WebHost.rs:889-1129`) |
| Canvas/scene | color / NO_COLOR | 1U | HTTP JSON/source responses, no terminal color contract found (`WebHost.rs:889-1129`) |
| Canvas/scene | resize | 1U | browser viewport behavior is outside this TUI audit (`docs/reference/canvas-protocol.md`) |
| `jet ?` | responsiveness | 2U | query prints directly; TTY path enters app (`main.rs:1065-1074`) |
| `jet ?` | keyboard model | 2U | interactive app exists, but key transcript pending (`main.rs:1070-1072`) |
| `jet ?` | progressive disclosure | 3U | query, categorized palette, and interactive modes (`main.rs:1065-1074`) |
| `jet ?` | color / NO_COLOR | 3U | ColorChoice resolves against TTY and explicit flags (`main.rs:1051-1052`) |
| `jet ?` | resize | 2U | non-TTY width is fixed 72; interactive resize unverified (`main.rs:1074`) |
| `jet debug` | responsiveness | 2U | prompt-driven stepping; native read has 30 s timeout (`Native.rs:50-75`, `Inferior.rs:54-63`) |
| `jet debug` | keyboard model | 2U | `(jet)` command prompt exists; command list/escape behavior unverified (`Native.rs:199-220`) |
| `jet debug` | progressive disclosure | 2U | Jet frames hide generated Rust by default; raw frames opt in (`Native.rs:8-11`) |
| `jet debug` | color / NO_COLOR | 1U | no shared Theme path found in prompt/session output (`Native.rs:188-220`) |
| `jet debug` | resize | 1U | line prompt has no width/resize path (`Native.rs:199-220`) |
| notebook terminal client | responsiveness | 1U | server/API state is present; terminal client not identified (`CmdNotebook.rs:145-175`, `client.html:103-121`) |
| notebook terminal client | keyboard model | 1U | HTML buttons/API actions, no terminal key model (`client.html:92-121`) |
| notebook terminal client | progressive disclosure | 2U | stale/quarantined output and details exist in client (`client.html:64-78`) |
| notebook terminal client | color / NO_COLOR | 1U | browser CSS/client, no terminal policy found (`client.html:1-125`) |
| notebook terminal client | resize | 1U | browser layout, not terminal behavior (`client.html:1-125`) |
| bare `jet` / usage | responsiveness | 3U | one generated usage page (`main.rs:455-462`) |
| bare `jet` / usage | keyboard model | 1U | one-shot usage has no keyboard path (`main.rs:455-462`) |
| bare `jet` / usage | progressive disclosure | 2U | CLI schema is shared; hierarchy runtime capture pending (`CLISchema.rs:1-3`) |
| bare `jet` / usage | color / NO_COLOR | 1U | usage wrapper does not show explicit color resolution (`main.rs:455-462`) |
| bare `jet` / usage | resize | 1U | no width argument in wrapper (`main.rs:455-462`) |

## Ranked gap list

| Rank | Severity | Affected surface | Evidence | Fix direction |
|---:|---|---|---|---|
| 1 | P1 | All TTY surfaces | raw ANSI in REPL plus literal live clear (`Interactive.rs:806-851`, `main.rs:1531-1534`) | Make one terminal session/renderer own clear, cursor, width, resize, color, and restore; callers emit semantic rows. |
| 2 | P1 | Dev/build/test/run | separate `[watch]`, `[build]`, test, and timing strings (`CmdDevTools.rs:178-184`, `CmdCompile.rs:4553-4567`, `4670-4675`) | Define one progress event vocabulary with plain, TTY, JSON, and quiet renderers. |
| 3 | P1 | REPL/help/debug/live inspect | each surface has distinct controls; only REPL has a broad discovery hint (`Render.rs:39-60`, `Native.rs:199-220`, `main.rs:1523-1539`) | Add a shared interaction footer and consistent Esc/Ctrl-C/EOF cancellation semantics. |
| 4 | P2 | Diagnostics/explain/inspect/test/build | long output has batch text and JSON, but no summary/drill-down (`Diagnostics.rs:869-900`, `CmdDossier.rs:69-80`) | Add summary-first human output with explicit detail requests; keep JSON complete. |
| 5 | P2 | REPL/live inspect/dev/build/diagnostics | width is handled only in REPL; other output has no width input (`Term.rs:368-385`, `Diagnostics.rs:869-877`) | Centralize terminal width and graceful narrow rendering; add resize/redraw or line-safe degradation. |
| 6 | P2 | Canvas/scene | browser routes exist, no terminal CLI entry point (`WebHost.rs:889-1129`) | Decide inventory boundary. If terminal status is intended, expose a small semantic status/report surface; otherwise remove CLI claim. |
| 7 | P2 | notebook | shipped client is HTML; no terminal client path found (`CmdNotebook.rs:145-175`, `client.html:1-125`) | Decide whether protocol stdin is the terminal surface; document or delete the candidate from TUI scope. |
| 8 | P3 | bare `jet` / usage | generated usage wrapper has no width/color contract (`main.rs:455-462`) | Route usage through shared terminal policy and width-aware renderer; preserve pipe-safe plain output. |

## Proposed follow-up card clusters

No cards were minted. The brief forbids Tower writes. These are the exact
clusters the orchestrator should mint, each body linking this report and
`docs/reference/tui-interaction.md`:

1. **P1: unify Jet terminal lifecycle and ANSI ownership** — all raw mode,
   cursor control, clear/redraw, color, width, resize, restore, and non-TTY
   floor rules.
2. **P1: unify Jet progress/status event rendering** — dev, build, run, test,
   timing, quiet, pipe, and JSON contracts.
3. **P1: unify interactive key discovery and cancellation** — REPL, help,
   debug, live inspect, and future terminal clients.
4. **P2: add summary-first progressive disclosure to reports** — diagnostics,
   explain, inspect, test, and build output.
5. **P2: resolve Canvas and notebook TUI inventory boundary** — either add the
   terminal contract or remove the candidate from TUI scope.

Required card body references:

```text
Audit: docs/audits/tui-dx-audit-2026-08-20.md
Principles: docs/reference/tui-interaction.md
Origin: c0v7cn5o
```

## Micro sweep

| Category | Result |
|---|---|
| Syntax | Terminal control syntax is hidden in Rust; no Jet user syntax added. |
| Ergonomics | REPL obvious path is strong; other reports require command knowledge and have no shared drill-down. |
| Surfaces | Shared diagnostic and color homes exist; progress, width, and control homes do not. |
| APIs, types, methods | `ColorChoice`, `Theme`, `render_all_colored`, `render_all_linked`, `terminal_width`, and `KeyReader` are useful homes; no shared progress event type found. |
| Defaults | REPL falls back to cooked input; `jet ?` falls back to one-shot output; other TTY/non-TTY floors need proof. |
| Naming | `[watch]`, `[build]`, `jet-timing`, `watching`, and `ran in` describe related status with different vocabularies. |
| Error text and diagnostics | What/Why/Fix and pointer line are strong; report navigation is batch-only. |
| UX and DX | REPL has progressive interaction; build/test/debug/inspect are separate loops. |
| Tooling and CLI shape | CLI schema/help sharing is a strength; terminal rendering is not similarly shared. |
| Ceremony versus control | Users get hidden control in REPL only after discovering F3/^B/^P/^F/^R; other surfaces lack explicit control affordances. |

## Four standing-lens answers

1. **Level playing field:** Jet can win categorically with one source-owned
   terminal contract: one color policy, one progress vocabulary, one renderer,
   and one non-TTY floor across every command. This is designed in pieces, not
   shipped as a whole.
2. **What to avoid:** avoid raw ANSI outside the terminal owner; avoid one-off
   progress strings; avoid browser-only assumptions in a TUI inventory; avoid
   claiming a TTY behavior from source alone. The shared diagnostic renderer and
   `ColorChoice` are structural immunities against some duplication, not proof
   that every caller uses them.
3. **AI-driven development:** shared semantic rows improve verdict actionability,
   context economy, and repair determinism. Fixed polling and batch output lose
   verdict latency and make an agent infer state from prose. Stable JSON paths
   are the machine floor; human TTY rows need the same event source.
4. **Concrete surfaces:** covered with source proof: REPL, dev, live inspect,
   test, build/run, diagnostics, explain/inspect, `jet ?`, debug, notebook
   launcher, bare usage. Worth checking: Canvas terminal boundary, all command
   progress variants, SIGWINCH behavior, and every `--json`/`--quiet` pair.
   Missing: shared progress event type, shared terminal lifecycle owner, and a
   confirmed terminal Canvas/notebook client.

## Open proof and strongest assumption

Criteria 1–2 remain open until the orchestrator adds real TTY/non-TTY/
`NO_COLOR`/narrow captures. Criterion 3 remains open until five follow-up cards
are minted and their IDs are recorded in the report, card log, and committed
board store.

Strongest unverified assumption: the requested “Canvas/scene CLI” and “notebook
terminal client” may refer to terminal status/protocol paths not present in the
inspected source; inventory must be confirmed before implementation cards are
closed.
