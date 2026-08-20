# Runtime error report: root cause first, then the trail

Card #2044. Ratified law this implements: D-FAIL-BREACH1=A (one renderer for a
running program's breach stop), D-FAIL-EDGE1=A / D-FAIL-EXIT1=A (one report at
the target-selected boundary), D-OBS1/D-OBS2 (E3001 report contents) and
D-FAIL-CTX1 (E3002 journey frames).

Card #1967 pinned the failing `??` panic report in a golden for tier identity.
This change re-blesses that golden, as that card's log allows.

## The defect

`jet_journey_report` put the trail **before** the failure:

```rust
// crates/jet-foundation/src/Outcome.rs:151 (before)
let mut report = format!("{journey}{error}");
```

A three-hop failure therefore opened with three near-identical lines and closed
with the one line that says what broke. Two more places copied that order: the
generated wasm store (`Codegen/Web.rs:8104, 8111`) and the JS projection
(`Prelude/Core/RuntimeStop.js`'s `jet_web_error_frame`).

A second, smaller defect sat on the first line of every stop: E3001, E3010 and
E3012 carried a description of the report format inside their `what` template,
so the first line described the report instead of the failure.

## Inventory before the change

| Shape | Owner | First line |
|---|---|---|
| Program-side stop | `jet_render_runtime_stop_from_row`, `Outcome.rs:584-682` | `Stop [E3001]: <what>` then `--> file:line in fn`, context box, locals, `Why`, `Fix` |
| Escaping failure | `jet_render_err`, `Outcome.rs:202-223` | `Error [code]: msg` then indented `cause:` chain |
| `?` trail | `jet_journey_frame`, `Outcome.rs:158-199` | one `error propagated from: fn (file:line) via ?[: note]` line per hop, printed **above** the failure |
| Transport headers | `Prelude/Scheduler.rs:82-88` | keys on the first line being `Stop [` or `Error [` |

The transport check reads the first line, so the old order also meant a
transported report could open with `error propagated from:` and fail to be
recognised as a report at all.

## The grammar

One order, owned by `jet_journey_compose` in `Outcome.rs`:

1. **Root block** — unchanged in content: the `Stop [code]` / `Error [code]`
   header, its source frame, context box, locals, `Why` and `Fix`, or the
   `Error:` line with its `cause:` chain.
2. **Trail block** — E3002, rendered by `jet_journey_trail`, printed under the
   root block and only when the failure crossed at least one `?`:

```
 Trail [E3002] (N hops via ?, origin first):
  1. fn (file:line) — note
  2. fn (file:line) ×4
```

Rules:

- Hops read origin first, the same order they were claimed at each `?`.
- The header states the mechanism (`via ?`) and the total hop count once, so a
  hop line carries only its own facts.
- Consecutive hops at the same site stay one line and count their repeats as
  `×N`. This is the existing collapse; the count is new, so a re-propagating
  loop now says how many times it went round instead of hiding it.
- A hop note is lazy, as before: a collapsed repeat never evaluates one.
- The block is a registered product and names its code, like `Stop [E3001]`.

## Before and after

Real example, `examples/features/errors/error_context.jet`. Before is the
checked-in golden. **After is a PREDICTION from the source change — it must be
blessed by a real run before anyone treats it as fact.**

Before (`examples/features/expected/errors/error_context.err.out`):

```
error propagated from: parse_config (examples/features/errors/error_context.jet:7) via ?: reading raw config
error propagated from: load_config (examples/features/errors/error_context.jet:12) via ?: loading config app.toml
error propagated from: run (examples/features/errors/error_context.jet:16) via ?
Error: file not found
```

After (predicted):

```
Error: file not found
 Trail [E3002] (3 hops via ?, origin first):
  1. parse_config (examples/features/errors/error_context.jet:7) — reading raw config
  2. load_config (examples/features/errors/error_context.jet:12) — loading config app.toml
  3. run (examples/features/errors/error_context.jet:16)
```

The failure is on line 1 either way a reader scans: top-down, or by first
non-indented line.

A repeating hop, from `tests/observe.rs`'s recursion case (predicted):

```
Error: bottom
 Trail [E3002] (5 hops via ?, origin first):
  1. dive (<fixture>:6) ×4
  2. run (<fixture>:8)
```

A stop's first line, before and after (predicted):

```
Stop [E3001]: `panic: expected condition` — with Jet file, line, function name, source-line context box, and (debug builds only) safe local variable values.
Stop [E3001]: `panic: expected condition`
```

## Terminal state

The report is laid out against two facts and nothing else, resolved once by
`JetReportStyle::for_stderr` at the report edge — the same source text on every
tier, so no engine owns a terminal decision (I9):

| Fact | Rule |
|---|---|
| Colour | `NO_COLOR` presence off, else `FORCE_COLOR` presence on, else stderr is a terminal. `Terminal.rs`'s `ColorChoice::Auto` calls the same ladder. |
| Columns | A terminal's `COLUMNS` when that is a positive integer, else the ratified 80. A pipe, a file and a JSON wire have **no** columns, so nothing elides. |

Colour dims **the trail block and only the trail block**. The root failure is
the one undimmed line, which is the whole point of the redesign; it also means
line 1 carries no SGR, so every transport check that reads the first line still
reads it.

A column budget sheds the trail's disposable parts and never reaches the root
failure, which is not the trail's line. What a hop keeps is its address — the
number, the `fn`, and `file:line`. What it sheds, in order:

1. **Leading path segments**, marked `…/`. Whole segments, never characters
   inside a name, and the file name itself is the floor: a half-spelled file
   name is worse than a line that wraps. One budget is computed for the whole
   block from its widest hop, so one file never renders two ways under one
   header.
2. **The note's tail**, marked `…`. The note is the prose a hop carried; the
   site is the fact. A note with no room for a character plus the ellipsis goes
   entirely rather than leaving ` — …`.

The header is the line that separates the root failure from its trail, so it
must not wrap: when the full form does not fit it drops the mechanism reminder
and keeps the facts — ` Trail [E3002] (3 hops):` is the floor.

So the same failure at 40 columns reads (PREDICTION until blessed):

```
Error: file not found
 Trail [E3002] (3 hops):
  1. parse_config (error_context.jet:7)
  2. load_config (error_context.jet:12)
  3. run (error_context.jet:16)
```

Three addressable sites under the root failure, nothing wrapped, commentary
gone. The root cause is still the first thing on screen and still the only
thing that never degrades.

## Where the change lives

- `crates/jet-foundation/src/Outcome.rs:99-457` — the one owner: hop state,
  `jet_journey_frame`, `JetReportStyle`, `jet_journey_trail`,
  `jet_journey_compose`, `jet_journey_report`.
- `crates/jet-foundation/src/Terminal.rs:26-38` — `ColorChoice::Auto` now reads
  the ladder from `Outcome.rs` instead of spelling its own, because that file is
  the one emitted verbatim into generated programs.
- `crates/jet-codegen/src/Prelude/Diagnostics.jet:389-395` — E3002's registered
  grammar; the format description removed from E3001, E3010 and E3012 `what`.
- Adapters only marshal: `Prelude/Core.rs`, `Codegen/Web.rs`,
  `Codegen/TIR/eval/exprs.rs`, `jet-jit/src/jit/runtime_host.rs`. The wasm store
  now calls `jet_journey_compose` instead of ordering the halves itself.
- `crates/jet-codegen/src/Prelude/Core/RuntimeStop.js` — the JS projection. The
  JS tier runs no Rust, so this grammar is duplicated by construction; the
  cross-tier assertion in `tests/web_build.rs` is what keeps the two identical.
  Making that projection generated from the one grammar is a separate, named
  drift (see below).

## Not in this change

- No channel from the `--color=always|never` flag to the report. That flag is
  CLI state; the report edge is Foundation, which sees the environment and the
  stream and nothing else. Giving it a channel means a per-tier variant, since
  `Prelude/Term.rs`'s colour-mode cell is unreachable from the compiler-side
  build of this file. `NO_COLOR` is the documented way to force a plain report.
- No amendment to E3002's registered row. It states the canonical grammar,
  which is unchanged; a narrow terminal adapts that grammar the way wrapping
  does, and colour is not format.
- No width from `stty`. `io.terminal_width()` may shell out; the report edge
  will not spawn a child to lay out a failing program's last line.
- No change to compile-time diagnostic rendering.
- No new capture: no stack unwinding, no async trace expansion. The trail is
  still exactly the `?` hops the program claimed.
- No dependency on #2041's ballot: hops carry `fn`, `file`, `line` and a note,
  never syntax.

## Known drift found while doing this

1. `Prelude/Core/RuntimeStop.js` re-implements the trail grammar, the
   `Error [code]` header and the `cause:` chain in JavaScript. It also had no
   consecutive-hop collapse before this change. One product, two authors.
2. `llms.text` row E3002 is stale against `Diagnostics.jet` even before this
   change (it still says "Zig-style" and "error-return trace entry"), so
   `llm_digest_regenerates_byte_identically` cannot have been green.
3. `.claude/uisnap_diffs.txt` records `tests/ui/runtime_e3002.stderr` failing
   with **no** trail lines in the actual output — a separate live defect in the
   UI-snapshot path, not something this change causes or fixes.
