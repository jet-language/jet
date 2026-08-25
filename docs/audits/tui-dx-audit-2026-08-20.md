# Jet TUI DX audit — 2026-08-20

Status: source audit plus bounded runtime captures, using the existing
`target/debug/jet` on 2026-08-20. No `cargo build`, fresh Jet binary build, Jet
test run, generator, formatter, Tower CLI, board write, or commit ran in this
worker. The mandatory `scripts/agent/lane-check.sh` passed. Normal build, dev,
and test success paths therefore remain source-only. Scores combine source
evidence with the captures named below; `U` marks an unverified interaction path.

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
2. P1 — raw ANSI ownership is violated in multiple terminal owners. The
   mechanical sweep found escapes in `jet-cli` help, `jet-devserver`, Jetpack
   output, command helpers, and generated Prelude paths outside the two named
   owners.
3. P1 — status/progress is split between `watching`, `[watch]`, `[build]`, test
   result lines, timing lines, and ad-hoc notices. No common event grammar or
   first-paint contract.
4. P1 — interactive discovery is uneven. REPL advertises most keys, but debug,
   help, notebook, and live inspect have separate prompt/control vocabularies.
5. P2 — progressive disclosure is strong in REPL but weak in diagnostics,
   explain/inspect, test, and build output. Long reports are text streams, not
   summaries with drill-down.
6. P2 — Canvas is a browser/dev-server surface, not a terminal surface. The
   requested “canvas/scene CLI” has no identified terminal entry point; this is
   an inventory gap, not proof of a missing feature.
7. P2 — non-TTY behavior is uneven. `jet ?`, REPL, notebook protocol, and
   diagnostics have observed floors; dev/build/test/report success paths still
   need transcript proof under pipes and narrow columns.

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
| Help/usage | `Source/main.rs:455-462`, `1127-1133`, `crates/jet-foundation/src/CLISchema.rs` | `jet --help`/`jet help` print generated usage; bare `jet` enters the REPL |
| Jetpack/adjacent CLI output | `crates/jetpack/src/Output.rs`, `Source/CmdMemory.rs`, `Source/CmdGc.rs` | adjacent terminal owners; included in the ANSI sweep, not normal-flow captured |

The inventory includes all seven named surfaces, the candidate interactive
surfaces found beside them, and adjacent Jetpack output that can reach a Jet
user. “Terminal client” means the terminal-facing path only; the notebook HTML
client and Canvas browser UI are recorded as adjacent owners, not silently
counted as TUI.

## Capture protocol

The bounded matrix below used the existing binary. TTY runs used `script`,
`TERM=xterm-256color`, and `stty rows 24 cols 60`. Pipe runs used
`NO_COLOR=1`. This proves current command entry, fallback, color, and error
behavior. It does not prove a successful compiler run, resize signal handling,
or every interactive key path.

Common shell forms:

```sh
target/debug/jet --version
target/debug/jet --help
NO_COLOR=1 target/debug/jet ? </dev/null
env -u NO_COLOR TERM=xterm-256color script -qefc \
  'stty rows 24 cols 60; target/debug/jet ?' capture.txt
NO_COLOR=1 bash -c "printf ':help\\n:quit\\n' | target/debug/jet repl"
env -u NO_COLOR TERM=xterm-256color bash -c \
  "printf ':quit\\n' | script -qefc 'stty rows 24 cols 60; target/debug/jet repl' repl.txt"
NO_COLOR=1 target/debug/jet inspect live 1 --once
NO_COLOR=1 target/debug/jet explain E2105
NO_COLOR=1 target/debug/jet notebook --protocol <<'EOF'
state
EOF
NO_COLOR=1 target/debug/jet canvas --help
```

The card forbids per-card test/build verification, so the dev, test, build, and
run blocks use their real `--help` contracts and safe invalid-target errors.

### ANSI ownership sweep

Command used (read-only source search):

```sh
rg -n '\\x1b\\[' Source crates --glob '*.rs' \
  | rg -v 'crates/jet-foundation/src/(Terminal|Diagnostics)\\.rs|crates/jet-repl/src/(Term|Interactive)\\.rs'
```

Representative hits:

```text
Source/main.rs:1532                         live inspect clear/redraw
Source/CmdMemory.rs:362                     audit banner
Source/CmdGc.rs:463                         local color helper
Source/CmdDevTools.rs:1566-1580            local bold/status helpers
crates/jet-cli/src/Help/Interactive.rs:38-294  alternate-screen/key redraw
crates/jet-cli/src/Help/Render.rs:73-193       direct color sequences
crates/jet-devserver/src/WebHost.rs:233-373    dashboard redraw
crates/jet-devserver/src/WebHost.rs:667         status dot color
crates/jetpack/src/Output.rs:399-489           progress redraw
crates/jet-codegen/src/Prelude/Term.rs:257     generated terminal color
crates/jet-foundation/src/Outcome.rs:262-263   generated report color
```

This is a finding, not a claim that every hit is wrong: `jet-cli` help and the
dev-server are legitimate terminal owners, but they are outside the two-owner
allow-list and do not visibly share one lifecycle contract. The follow-up must
either widen the canonical owner list with explicit seams or migrate these
paths. Tests in the grep output are not product paths.

## Current-state captures

Each block names the command, the observed output, and the proof limit. Output
below is shortened only where it contains terminal control bytes or a
machine-generated identifier.

### REPL

Source signal: raw mode requires TTYs and falls back when unavailable
(`crates/jet-repl/src/Term.rs:39-84`, `Interactive.rs:32-58`). The TTY banner
advertises `:quit`, `:help`, and `^B`; the discovery hint advertises completion,
docs, history, pin, fold, rerun, and bindings (`Render.rs:24-60`).

```text
CAPTURED — cooked pipe:
Jet 1.0.0 — interactive REPL  (:quit, :help, ^B bindings)
Try: ?name docs · :pin/:fold/:rerun <id> · interactive keys require a TTY
1 user> REPL meta-commands
  :quit ...  :help ...  ^P ... ^F ... ^R ... ^B ...
1 user> bye
exit=0; NO_COLOR=1; no ESC bytes.

CAPTURED — TTY, `TERM=xterm-256color`, 24x60, scripted `:quit`:
Jet 1.0.0 — interactive REPL  (:quit, :help, ^B bindings)
Try: Tab complete · F1 cursor docs · ?name docs · F3 history · ^P pin
1 user> :quit
bye
The raw transcript contains SGR color, cursor movement, and erase bytes.

Limit: this run proves cooked fallback, raw entry, prompt discovery, and clean
quit. It does not prove evaluation cancel, completion, folds, or resize.
```

### Dev loop and live inspect

Source signal: `jet dev` prints a watch banner, polls every 120 ms, and reports
hot replacement and edit-to-visible budget events (`Source/CmdDevTools.rs:89-186`).
`jet inspect live` polls every 250 ms and clears with literal `\x1b[2J\x1b[H`
unless non-TTY/JSON (`Source/main.rs:1520-1539`).

```text
CAPTURED — command contract:
NO_COLOR=1 target/debug/jet dev --help
jet dev — Rerun a program whenever files change
  jet dev [args]
  --watch  re-run on dependency changes; --watch=off runs once
  --swap   hot-swap compatible edits and restart after type changes

CAPTURED — inspect one-shot error floor:
NO_COLOR=1 target/debug/jet inspect live 1 --once
Error [E2105]: no live Jet runtime is observable at process 1
 Why: Jet could not complete the named file, tool, or operating-system operation
 Fix: start the program with --observe, or attach to a jet dev process
exit=1; no clear/redraw bytes in the pipe.

Limit: no successful dev loop ran because this card does not own compiler-build
verification. First paint, edit-to-visible, Ctrl-C, and live resize remain U.
```

### Test runner

Source signal: directory tests are sorted and run sequentially
(`Source/CmdCompile.rs:1858-1889`); individual doctest failures print
`FAIL (does not compile)` or `FAIL (runtime error)`, then a pass/FAIL line
(`Source/CmdCompile.rs:2210-2287`).

```text
CAPTURED — command contract:
NO_COLOR=1 target/debug/jet test --help
jet test — Run tests
  jet test [<file.jet|dir>] [<filter>]
  --coverage ...  --serial ...  --shuffle ...  --record ...

CAPTURED — invalid-target floor:
NO_COLOR=1 target/debug/jet test /definitely/not-a-jet-file
Error [E2105]: can't find `/definitely/not-a-jet-file`
 Why: Jet could not complete the named file, tool, or operating-system operation
 Fix: correct the named problem, then run the command again
exit=1.

Limit: no test suite ran under this card; pass/fail ordering, progress, and
input-wait behavior remain source-only.
```

### Build/run progress

Source signal: `jet run` can print a verbose cache-hit status
(`Source/CmdCompile.rs:547-565`); verbose build emits deterministic `[build]`
steps (`Source/CmdCompile.rs:4553-4567`), while timing emits separate
`jet-timing` lines (`Source/CmdCompile.rs:4670-4675`).

```text
CAPTURED — command contracts:
NO_COLOR=1 target/debug/jet build --help
jet build — Create a native executable
  --verbose  print the bridge steps
  --profile  how hard to optimize: release, debug, ci, or a named optimization bundle

NO_COLOR=1 target/debug/jet run --help
jet run — Run a program or project
  --watch  re-run on dependency changes; --watch=off runs once
  --output  run a named build output

CAPTURED — invalid-target floors:
jet build /definitely/not-a-jet-file -> Error [E1334]: authority file `/definitely` is missing
jet run /definitely/not-a-jet-file -> Error [E2105]: can't find the file `/definitely/not-a-jet-file`

Limit: no build/run success path ran. First progress, cache-hit, timing, stream
ordering, and narrow wrapping remain source-only.
```

### Diagnostics

Source signal: human diagnostics use linked rendering and append a count plus an
`explain` pointer (`Source/main.rs:3292-3320`). The renderer has a color-aware
batch path and OSC 8 hyperlinks (`crates/jet-foundation/src/Diagnostics.rs:869-900`).
Color resolution is centralized (`crates/jet-foundation/src/Terminal.rs:26-36`,
`Source/main.rs:99-115`).

```text
CAPTURED — human diagnostic, pipe, `NO_COLOR=1`:
target/debug/jet inspect live 1 --once
Error [E2105]: no live Jet runtime is observable at process 1
 Why: Jet could not complete the named file, tool, or operating-system operation
 Fix: start the program with --observe, or attach to a jet dev process
No source frame is needed for this process-state diagnostic. The pipe contains
no ESC bytes.

Limit: a source check fixture was not run under this card. Source frames,
multiple diagnostics, OSC 8 links, and narrow source rendering remain U.
```

### Explain and inspect reports

Source signal: `jet explain` chooses text or JSON, with the same `Explain::render`
writer for diagnostic essays and policy markers (`Source/CmdDevTools.rs:1704-1819`,
`1876-1896`). Dossier text prints labels and provenance rows, with a JSON path
(`Source/CmdDossier.rs:84-150`).

```text
CAPTURED — text explanation, pipe, `NO_COLOR=1`:
target/debug/jet explain E2105
E2105

What this means:
  `{problem}`
Why Jet enforces it:
  Jet could not complete the named file, tool, or operating-system operation.
How to fix it:
  Correct the named problem, then run the command again.
This explanation comes from jet's diagnostics reference.
exit=0; no ESC bytes.

Limit: dossier and JSON report paths were not run. Summary-first ordering,
long provenance, and narrow report wrapping remain source-only.
```

### Canvas/scene surface

Source signal: the dev server announces a Canvas URL (`WebHost.rs:565-572`);
graph, project, command, transaction, and debug routes are HTTP endpoints
(`WebHost.rs:889-1129`). The protocol document defines source-backed graph and
transaction state (`docs/reference/canvas-protocol.md`). No terminal command or
terminal renderer was found in the inspected entry points.

```text
CAPTURED — terminal inventory boundary:
NO_COLOR=1 target/debug/jet canvas --help
Error [E2101]: `canvas` isn't a jet command.
 Why: every jet run starts with a command like `run`, `check`, or `new`.
 Fix: did you mean `jetpack hangar`? Run `jet help` to see them all.
exit=2.

The dev-server source announces a browser Canvas URL and owns HTTP routes. No
terminal Canvas/scene renderer or CLI entry point exists in this checkout.
Canvas is therefore an adjacent browser surface, not a TUI surface. This
resolves the inventory question; it does not create a missing feature card.
```

### `jet ?` interactive help

Source signal: TTY opens `Help::Interactive`; query and non-TTY paths print once
(`Source/main.rs:1040-1076`).

```text
CAPTURED — pipe fallback:
NO_COLOR=1 target/debug/jet ? </dev/null
┌─ jet ? — command palette ────────────────────────────────────────────┐
│  type to search · ↑↓ · ⏎ command · Alt+⏎ example · ⇥ detail · F1     │
│> ▸ Build & Run                                                       │
...
└──────────────────────────────────────────────────────────────────────┘
All pipe rows measured 72 columns; no ESC bytes.

CAPTURED — TTY, `TERM=xterm-256color`, 24x60:
The same palette rendered with clipped hint text (`det…`), color SGR, and
cursor erase/redraw. A scripted `:quit`-style key exit was not supplied; only
initial paint was captured.

Color check: `NO_COLOR=1` pipe had 0 ESC bytes; `FORCE_COLOR=1` had 4; setting
both kept 0; `jet ? --color=always` had 4 and `--color=never` had 0.
```

### `jet debug`

Source signal: one command selects interpreter or native backend and exposes a
`(jet)` prompt (`Source/main.rs:2207-2242`; native prompt
`crates/jet-debug/src/Native.rs:199-220`). Native mode reports missing `lldb`
and points to the interpreter path (`Native.rs:105-118`).

```text
CAPTURED — command contract:
NO_COLOR=1 target/debug/jet debug --help
jet debug — Debug a program from Jet source
  jet debug [args]
  --replay  consume a named replay as --replay=<name>

The `(jet)` prompt and native/interpreter split are source-confirmed. No debug
session ran under this card; prompt keys, Ctrl-C/EOF, and narrow output remain U.
```

Ship note (2026-08-24): targeted production coverage now exercises native DAP
launch and same-user attach, with Jet-projected threads, stacks, scopes, nested
values, and read-only evaluation. `showRawFrames` remains the explicit escape
hatch for generated-Rust frames; the interactive TUI gaps above remain open.

### `jet notebook` terminal client

Source signal: `jet notebook` starts an HTTP notebook server and prints its URL
(`Source/CmdNotebook.rs:145-175`); the shipped client is HTML and has actions for
run/inspect/debug/profile, stdin, interrupt, import/export, and offline drafts
(`crates/jet-repl/src/Notebook/client.html:82-121`). No separate terminal client
was identified.

```text
CAPTURED — headless terminal protocol:
NO_COLOR=1 target/debug/jet notebook --protocol <<EOF
state
EOF
{"status":"ok","body":"{...\"cells\":[],...\"turns\":[]}"}
exit=0.

TTY `jet notebook` starts an HTTP server and prints its URL; the shipped rich
client is HTML. The protocol is a line-oriented terminal adapter, not a TUI:
it returns JSONL replies and has no shared key model, color, or resize path.
```

### bare `jet` / usage

Source signal: `jet::CLI::usage_page` generates `jet --help` and `jet help`
(`Source/main.rs:455-475`); bare `jet` dispatches to the REPL
(`Source/main.rs:1127-1133`).

```text
CAPTURED — generated usage:
target/debug/jet --version -> Jet 1.0.0
target/debug/jet --help -> usage page from `jet::CLI::usage_page`, including
the command list and flag list; exit=0.

Bare `target/debug/jet` is not a usage page. It enters the REPL, covered above.
The usage page is one-shot and does not expose keyboard, width, or drill-down
controls. `--color` is handled by the command palette, but usage itself is
plain in a pipe.
```

## Scorecard

Static score: `3` source-complete mechanism, `2` partial/locally complete, `1`
gap or ad-hoc path. `U` means at least one important interaction path remains
unverified. Evidence combines source pointers and the bounded captures above.

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
| `jet ?` | color / NO_COLOR | 3 | ColorChoice plus observed pipe matrix: `NO_COLOR` 0 ESC, `FORCE_COLOR` 4 ESC, explicit always/never 4/0 (`main.rs:1051-1052`) |
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
| Help/usage | responsiveness | 3 | `jet --help` and `jet help` print one generated page; bare `jet` enters REPL (`main.rs:455-462`, `1127-1133`) |
| Help/usage | keyboard model | 1 | one-shot usage has no keyboard path (`main.rs:455-462`) |
| Help/usage | progressive disclosure | 2 | shared CLI schema, but no drill-down from the one-shot page (`CLISchema.rs:1-3`) |
| Help/usage | color / NO_COLOR | 1 | usage is plain in the pipe; no width/color renderer is wired at the wrapper (`main.rs:455-462`) |
| Help/usage | resize | 1 | no width argument in wrapper (`main.rs:455-462`) |

The scorecard's color scores are deliberately conservative after the ANSI
sweep: shared diagnostics are strong, but the terminal family is not compliant
with one-owner policy until the listed alternate owners are reconciled.

## Ranked gap list

| Rank | Severity | Affected surface | Evidence | Fix direction |
|---:|---|---|---|---|
| 1 | P1 | All TTY surfaces | raw ANSI in REPL plus literal live clear (`Interactive.rs:806-851`, `main.rs:1531-1534`) | Make one terminal session/renderer own clear, cursor, width, resize, color, and restore; callers emit semantic rows. |
| 2 | P1 | All terminal owners | raw escape sweep finds `jet-cli`, `jet-devserver`, Jetpack, command helpers, and generated Prelude paths (`ANSI ownership sweep` above) | Define the canonical owner boundary; migrate or explicitly seam every hit; keep generated paths on the same policy. |
| 3 | P1 | Dev/build/test/run | separate `[watch]`, `[build]`, test, and timing strings (`CmdDevTools.rs:178-184`, `CmdCompile.rs:4553-4567`, `4670-4675`) | Define one progress event vocabulary with plain, TTY, JSON, and quiet renderers. |
| 4 | P1 | REPL/help/debug/live inspect | each surface has distinct controls; only REPL has a broad discovery hint (`Render.rs:39-60`, `Native.rs:199-220`, `main.rs:1523-1539`) | Add a shared interaction footer and consistent Esc/Ctrl-C/EOF cancellation semantics. |
| 5 | P2 | Diagnostics/explain/inspect/test/build | long output has batch text and JSON, but no summary/drill-down (`Diagnostics.rs:869-900`, `CmdDossier.rs:69-80`) | Add summary-first human output with explicit detail requests; keep JSON complete. |
| 6 | P2 | REPL/live inspect/dev/build/diagnostics | width is handled only in REPL; other output has no width input (`Term.rs:368-385`, `Diagnostics.rs:869-877`) | Centralize terminal width and graceful narrow rendering; add resize/redraw or line-safe degradation. |
| 7 | P2 | Canvas/scene | browser routes exist, no terminal CLI entry point (`WebHost.rs:889-1129`) | Decide inventory boundary. If terminal status is intended, expose a small semantic status/report surface; otherwise remove CLI claim. |
| 8 | P2 | notebook | shipped client is HTML; no terminal client path found (`CmdNotebook.rs:145-175`, `client.html:1-125`) | Decide whether protocol stdin is the terminal surface; document or delete the candidate from TUI scope. |
| 9 | P3 | Help/usage | generated usage wrapper has no width/color contract (`main.rs:455-462`) | Route usage through shared terminal policy and width-aware renderer; preserve pipe-safe plain output. |

## Proposed follow-up card clusters

No cards were minted. The brief forbids Tower writes. These are the exact
clusters the orchestrator should mint, each body linking this report and
`docs/reference/tui-interaction.md`:

1. **P1: unify Jet terminal lifecycle and ANSI ownership** — all raw mode,
   cursor control, clear/redraw, color, width, resize, restore, and non-TTY
   floor rules; done when the ANSI sweep has one documented owner per path.
2. **P1: unify Jet progress/status event rendering** — dev, build, run, test,
   timing, quiet, pipe, and JSON contracts; done when one event stream drives
   every view and proves first-paint/final-result order.
3. **P1: unify interactive key discovery and cancellation** — REPL, help,
   debug, live inspect, and future terminal clients; done when controls are
   printed at first use and accept/cancel/EOF/interrupt behavior is captured.
4. **P2: add summary-first progressive disclosure to reports** — diagnostics,
   explain, inspect, test, and build output; done when human detail has a named
   drill-down and JSON keeps the complete facts.
5. **P2: resolve Canvas and notebook TUI inventory boundary** — record Canvas
   as browser-only unless an owner adds a terminal command; keep notebook's
   JSONL protocol separate from a future TUI; done when both boundaries are
   stated in their owning references.

Required card body references:

```text
Audit: docs/audits/tui-dx-audit-2026-08-20.md
Principles: docs/reference/tui-interaction.md
Origin: b1l7zqt1
```

## Micro sweep

| Category | Result |
|---|---|
| Syntax | Terminal control syntax is hidden in Rust; no Jet user syntax added. |
| Ergonomics | REPL obvious path is strong; other reports require command knowledge and have no shared drill-down. |
| Surfaces | Shared diagnostic and color homes exist; progress, width, and control homes do not. |
| APIs, types, methods | `ColorChoice`, `Theme`, `render_all_colored`, `render_all_linked`, `terminal_width`, and `KeyReader` are useful homes; no shared progress event type found. |
| Defaults | REPL falls back to cooked input; `jet ?` falls back to one-shot output; notebook falls back to JSONL; build/dev/test success floors need proof. |
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
4. **Concrete surfaces:** covered with source proof and bounded captures: REPL,
   dev entry, live inspect, test entry, build/run entry, diagnostics,
   explain/inspect entry, `jet ?`, debug entry, notebook protocol, usage, and
   the absent Canvas command. Worth checking: successful progress variants,
   SIGWINCH behavior, and every `--json`/`--quiet` pair. Missing: shared
   progress event type, shared terminal lifecycle owner, and a confirmed
   terminal Canvas client.

## Open proof and strongest assumption

Criterion 1 is done for the current checkout: every named surface has an owner,
current-state source evidence, and a bounded command capture or an explicit
absence capture. Criterion 2 is done as a source-plus-capture scorecard; `U`
marks the successful compiler, resize, and full key matrices not run here.
Criterion 3 is open: five ranked card-ready follow-ups and the principles
reference are present, but the brief forbids Tower writes, so no board IDs can
be recorded.

Strongest unverified assumption: “Canvas/scene CLI” may mean a future terminal
surface rather than the current browser Canvas server. This audit records the
current fact: `jet canvas` does not exist; notebook `--protocol` is JSONL, not a
TUI.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| `TUI-TERMINAL-LIFECYCLE` | card | #2049 |
| `TUI-ANSI-OWNERSHIP` | card | #2049 |
| `TUI-PROGRESS-STATUS` | card | #2049 |
| `TUI-INTERACTIVE-DISCOVERY` | card | #2049 |
| `TUI-PROGRESSIVE-DISCLOSURE` | card | #2049 |
| `TUI-CANVAS-NOTEBOOK-BOUNDARY` | card | #2049 |
| `TUI-NONTTY` | card | #2049 |
<!-- /audit-dispositions -->
