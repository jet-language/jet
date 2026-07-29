# Pragmatism audit — 2026-07-28

**Status:** full multi-domain run. This replaces the same-day draft seed.
Skill: `.agents/skills/pragmatism-audit/SKILL.md`.
Method: live `scripts/agent/jet-env jet run` / `jet build` probes with the
2026-07-28 debug binary, plus four read passes over examples, core-library
docs, specs, and prelude code. Caveat: the tree was dirty with in-flight
typed-text work during the run; every headline finding was re-verified live.

## Thesis

Jet ships real jobs in CLI, packaging, data ingest, text, and concurrency.
The recurring failure shape is the last mile: a mechanism works, then stops
one step before the user can see or ship the result. Quantities compute but
cannot print. Typed CSV decodes but encodes an empty string under `jet run`.
Struct print works but leaks internal `user_` names. Web routing works but
JSON responses and static directories are hand glue. Games have an engine
shape with no playable loop. Most fixes are small defaults on mechanisms
that already exist; almost none need a new mechanism.

## Domain scorecard

| Domain / workload | Job | Grade | Top friction kind | Evidence |
| --- | --- | --- | --- | --- |
| CLI tools | flags + `--help` + env config tool | friction | dead-end-magic | `#CLI` ships; `core.args` help does not exit (`Args.rs:3-12`); no shorts/env on `#CLI` (`CheckerCli.rs:60-87`) |
| Scripting / automation | glob, subprocess, fail on error | friction | missing-default | `process.run` returns Ok on nonzero exit (`Process.rs:260-268`); scores repro shipped in 15 lines after 3 retries |
| Packaging / build / devloop | new → deps → test → fmt → tasks | ships | keep | inline deps, toolchain pin, `#Job`/`#Every`, `jet fmt --check`, `jet dev` |
| Web / HTTP services | two-route JSON API + static | friction | missing-default | routing ships (`http_routes.jet`); no `server.json` / `req.json<T>` / `static_files(root)`; `web.app()` only prints graph OK |
| UI (browser / desktop) | button + reactive count | friction | missing-default | GTK counter real; every path hand-writes measure/layout/paint; flagship web click state lives in host HTML/JS |
| Games | pong-like loop | blocked | dead-end-magic | `core.game` runs 3 headless frames (`Game.rs:246-247`); ECS query returns strings; raylib draws once, no loop example |
| Scientific / numeric | units, vectors, stats, print results | friction | dead-end-magic | `print(quantity)` = E0112; `.raw()` drops the unit; `Vec3` prints `user_Vec3 { user_0: … }` |
| Data / serde | CSV in → transform → CSV out | friction | dead-end-magic | `csv.to_string([T])` returns `""` under `jet run`; AOT binary prints `item,qty\npen,3` (verified twice) |
| Networking | call JSON API into structs | friction | missing-default | `resp.body().text(limit)` + separate `json.decode<T>`; no typed body helper |
| Text / parsing | log file → records → summary | friction | dead-end-magic | all pieces ship; no example composes file → pattern → aggregate end to end |
| Embedded / low-level | decode header; MMIO under `#Unsafe` | friction | domain-blind | binary patterns ship; MMIO lives only in `tests/target_profiles.rs`, not in examples; uninit buffer fill unfinished |
| Concurrency | fan out, collect, cancel | ships | keep | `taskgroup`, `para_map`, `#Shield`, deadlines; `parallel_scan.jet` still teaches the long channel path first |

## Findings

Ranked by damage to a person finishing a real job.

1. **Typed CSV encode silently empty under `jet run`** — kind
   `dead-end-magic`, domain data/serde. Evidence: live probe, `#Codable`
   list + `csv.to_string` → `LEN=0` under `jet run`, `LEN=14` with correct
   CSV from the AOT binary; helper exists in
   `crates/jet-codegen/src/Prelude/CoreLib/Top/DataFmt.rs:482-508`;
   `examples/features/serde/csv_typed.jet` dodges it by re-emitting JSON.
   Beginner impact: silent data loss on the default run path. Expert paths:
   none makes the JIT honest. Smallest fix: implement or fail closed in
   the resident path — an empty success is never correct. Owner-gate: no
   (defect). Title if balloted: `D-CSV-ENCODE-DEV1`.

2. **Default struct/Vec3 print leaks mangled `user_` names** — kind
   `dead-end-magic`, domain everywhere. Evidence: live probe,
   `print(Point.{x:1, y:2})` → `user_Point { user_x: 1, user_y: 2 }`; same
   for built-in `Vec3`. Beginner impact: the free Debug print is the first
   magic a new user meets, and it exposes rustc-side name mangling (hurts
   the I2 story). Expert path: hand Display. Smallest fix: strip the
   `user_` prefix in derived debug output. Owner-gate: no (defect).

3. **Raw Rust panic prints during a normal error run** — kind
   `dead-end-magic` (defect), domain toolchain. Evidence: deterministic
   repro kept at `/tmp/pragma_panic.jet`; `jet run` prints
   `thread 'main' panicked at crates/jet-codegen/src/Codegen/TIR/lower/builtins.rs:580`
   (assertion `Read == EagerWrite`) above ordinary E0102/E0107 output.
   Small variants do not trigger it; the struct + push + `sort_by` mix
   does. Relates to commit b022c23f2 "avoid lowering rejected bodies";
   dirty-tree caveat applies. Beginner impact: internal chatter beside
   diagnostics breaks trust. Smallest fix: never lower rejected bodies;
   assert becomes ICE path. Owner-gate: no (defect).

4. **Quantity and unit values cannot print their unit** — kind
   `dead-end-magic`, domain science/UI. Evidence: live probe,
   `print(recovered)` → E0112 in
   `examples/features/types/dimensional_quantities.jet:15`; `.raw()` prints
   `12.0` with no unit; distinct/unit types make users retype the unit in
   the string (`distinct_types.jet:19`). The seed note said "bare number";
   the truth today is a hard error. Beginner impact: the algebra teaches,
   then the demo cannot show the answer. Smallest fix: default Display with
   units (`12 meter`, `4 meter/second`), hand Display as override, `.raw()`
   as reject. Owner-gate: yes — `D-QUANTITY-PRINT1` on card #1268 (open).

5. **`core.args` recognizes `--help` but does not finish it** — kind
   `dead-end-magic`, domain CLI. Evidence:
   `crates/jet-codegen/src/Prelude/CoreLib/Top/Args.rs:3-12,520-523`;
   `examples/features/io/args_spec.jet:12-24` hand-rolls the branch; the
   `#CLI` path already prints help and returns
   (`crates/jet-codegen/src/Codegen/Items.rs:723`). Beginner impact: the
   documented library floor feels broken next to `#CLI`. Smallest fix:
   `spec.parse_or_exit(argv)`; keep non-exiting `.parse` as the testable
   override. Owner-gate: no (stdlib method).

6. **`#CLI` lacks shorts, env fallbacks, and repeats the builder has** —
   kind `missing-default`, domain CLI. Evidence:
   `crates/jet-sema/src/Sema/CheckerCli.rs:60-103`;
   `.flag_short`/`.option_env`/`.repeat` exist on `core.args`
   (`docs/reference/core-library.md:856-867`). Beginner impact: real tools
   needing `-v` or `PORT=` abandon the typed path whole. Smallest fix:
   `#Short("v")` / `#Env("PORT")` field markers that lower to the same
   builder — one mechanism. Owner-gate: yes (new markers) —
   `D-CLI-FIELD-MARKERS1`.

7. **`process.run` reports Ok on a failed command** — kind
   `missing-default`, domain scripting. Evidence:
   `crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs:260-268`; docs
   show the manual `if !status.success` dance
   (`docs/reference/core-library.md:1066-1067`). Beginner impact: "run or
   die" scripts silently continue. Smallest fix: `run_checked` (or `.ok()`)
   that errors on nonzero; keep `run` as the inspect override. Owner-gate:
   no.

8. **HTTP JSON needs hand glue on both sides** — kind `missing-default`,
   domain web/net. Evidence: client reads
   `resp.body().text(8 * 1024 * 1024)` then decodes separately
   (`examples/features/net/http_get.jet:19-20`,
   `serde/json_typed.jet:20-25`); the server REST demo echoes text
   (`http_rest_service.jet`); no `application/json` use anywhere in
   `examples/features/net/`. Beginner impact: the single most common
   service job re-glues what `#Codable` already knows. Smallest fix:
   `resp.json<T>(limit)`, `req.json<T>()`, `server.json(status, value)` on
   the one `#Codable` path; raw text/body stays the override. Owner-gate:
   yes (core.http surface) — `D-HTTP-JSON1`.

9. **Diagnostics know the answer and hint the wrong fix** — kind
   `missing-default`, domain everywhere. Evidence: live probes. Unresolved
   `fs` → E0107 says "declare it first: `fs :: ...`" instead of "add
   `use core.files as fs`". `Iter` has no `get` → E0102 says "define it
   inside `struct Iter`" instead of "call `.to_list()` first". Beginner
   impact: my own 15-line script took three round trips; each fix text
   pointed away from the real fix. Smallest fix: import suggestion for
   known core modules; stdlib-aware method hints. Owner-gate: no
   (diagnostic copy, snapshot-tested per I4).

10. **`core.game` is a transcript, not a game** — kind `dead-end-magic`,
    domain games. Evidence: `game.run` hardcodes 3 headless frames
    (`crates/jet-codegen/src/Prelude/CoreLib/Top/Game.rs:246-247`);
    `scene.query<T...>()` returns marker strings
    (`docs/reference/core-library.md:1378-1379`); raylib bridge draws once
    with no loop example and no audio
    (`examples/features/game/raylib_window.jet:6-19`). Beginner impact:
    the engine shape promises what no backend delivers. Smallest fix: one
    backend drives `on_frame` until window close, or stop implying
    playability. Owner-gate: yes — `D-GAME-LOOP1`.

11. **`web.app()` builds a graph and serves nothing** — kind
    `dead-end-magic`, domain web. Evidence:
    `examples/features/web/web_app.jet:13-35` ends in
    `print("web-app-graph-ok")`; runtime records edges only
    (`crates/jet-codegen/src/Prelude/WebApp.rs:1-58`). Companion friction:
    the flagship browser click demo keeps state in hand-written HTML/JS
    (`ui_web_click.html:91-98`) while `web.on` exists in docs. Smallest
    fix: `jet run` serves the graph through `core.http.server`, and the
    flagship click demo owns state in Jet. Owner-gate: yes —
    `D-WEBAPP-SERVE1`, `D-WEB-CLICK-OWN1`.

12. **Every UI path pays measure/layout/paint** — kind `missing-default`,
    domain UI. Evidence: repeated verbatim in
    `docs/reference/core-library.md:2270-2283`, `ui_tui_reactive.jet:9-14`,
    `ui_native_linux.jet:31-35`, `ui_web_reactive.jet:9-15`. Smallest fix:
    `ui.mount(backend, tree)` does the pipeline; manual stages stay for
    experts. Owner-gate: yes — `D-UI-MOUNT1`.

13. **Interpolation has no precision spec** — kind `missing-default`,
    domain everywhere. Evidence: live probe, `"{x:.2}"` → E0003; the path
    is `use core.fmt as fmt` + `fmt.decimal(x, 2)`
    (`examples/features/text/human_format.jet`). S8 ratified one value per
    brace; D-ATTR4 added only `#Debug`. Beginner impact: the most common
    formatting job needs an import and a helper call. Smallest fix: a
    format selector in the brace (for example `{x#Fixed(2)}` on the
    ratified selector rail). Owner-gate: yes (new syntax) —
    `D-FMT-INTERP1`.

14. **Static directory, CORS, and HTML responses stop short** — kind
    `missing-default`, domain web. Evidence: `static_file` ships,
    `static_files(root)` owner-gated open
    (`docs/reference/core-library.md:479-501`); CORS has no policy helper;
    typed `HTML.{}` values never reach a `server.html` response
    (`examples/features/safety/typed_sql.jet`). Smallest fix: ship the
    three helpers with safe defaults. Owner-gate: yes —
    `D-HTTP-STATIC-FILES1`, `D-HTTP-CORS1`.

15. **Examples teach the long path first** — kind `wrong-default`
    (onboarding), domains text/CLI/concurrency. Evidence: `first_hour.jet`
    hand-walks raw `io.args()` while `#CLI` is the ratified zero-ceremony
    path; `parallel_scan.jet:53-75` builds channels + spawn + join where
    `taskgroup.all` or `para_map` is one line; no example composes
    file → pattern → aggregate for the log job; text examples teach
    materializing `String.lines()` for file-scale work. Smallest fix:
    re-point flagship examples at the short path; keep long paths as
    labeled expert overrides. Owner-gate: no.

16. **Embedded examples never touch a register** — kind `domain-blind`,
    domain embedded. Evidence: `examples/features/lowlevel/freestanding.jet`
    is clamp + sqrt; `volatile_read`/`volatile_write` under a board
    profile live only in `tests/target_profiles.rs:~196-242`; the uninit
    buffer example stops before the I/O fill it documents
    (`examples/features/memory/uninit_buffer.jet:1-4`). Smallest fix: one
    executable MMIO example under `#Unsafe` + board profile; finish the
    fixed-array uninit write path. Owner-gate: no.

Smaller notes: `--help` usage line prints the absolute binary path instead
of the program name (wrong-default, cosmetic); no `confirm`/`choose`/
`input_secret` prompts (domain-blind, CLI); `jet tasks` discovery only via
the unknown-task error; literal regex patterns pay a `Result` + panic tax
that a comptime check could remove; length-prefixed binary reads force an
explicit `Int.from_u16` widen; heavy ndarray/FFT work is an accepted
bridge-gated gap (D-WD9).

## Defaults map

| Most likely use case | Today | Reject path | Override path | Hole |
| --- | --- | --- | --- | --- |
| Print a struct | Free debug print | n/a | Hand Display | leaks `user_` names (hole) |
| Print a quantity/unit | E0112 error | `.raw()` (drops unit) | Hand Display | no unit Display (hole) |
| Encode `[T]` to CSV | AOT correct; `jet run` empty | — | encode `[[String]]` / JSON | JIT parity (hole) |
| CLI flags + help | `#CLI` struct — ships | plain `fn run()` | `core.args` builder | shorts/env missing on `#CLI` (hole) |
| Run a subprocess, fail on error | Ok + manual check | — | inspect `ProcessResult` | checked default missing (hole) |
| GET JSON into structs | text + `json.decode` | raw body | hand parse | typed helper missing (hole) |
| Serve JSON from a route | hand string + header | — | raw `response` | `server.json` missing (hole) |
| Serve a static directory | per-file helpers | — | hand loop | `static_files(root)` gated (hole) |
| Show a UI tree | manual measure/layout/paint | — | manual stages | `mount` missing (hole) |
| Format a float in a string | `fmt.decimal(x, 2)` + import | — | manual math | brace selector missing (hole) |
| Fan out work | `taskgroup` / `para_map` — ships | serial code | channels + spawn | teaching order only |
| Decode a binary header | `[U8].{…}` pattern — ships | fallible reads | `Reader` calls | width widen tax only |
| Struct equality / compare | free when fields qualify | S55 reject unclear | hand impl | reject story on #1267 |

## Celebrated pragmatism

Keep these; they already ship the job.

- `#CLI` entry: a struct is the CLI spec; flags, defaults, `--help`, and
  subcommand enums come free; raw `io.args()` stays as the escape.
- `Sh.{…}` typed shell: each hole is one argv item; no shell injection by
  construction; subprocess stdin closed by default.
- Toolchain breadth in one binary: run, build, test, fuzz, fmt, fix, repl,
  dev/serve hot-swap, debug, doctor, LSP, package add/remove, trust grants.
- `#Codable` JSON round-trip with rename, defaults, and hand codecs.
- `core.data` typed CSV → filter → join → stats → bar chart with ceilings.
- Binary patterns `[U8].{"{f:U16be}…"}` + `Reader.take_pattern` — header
  decode without a parser DSL, with registered E1007–E1011 diagnostics.
- Structured concurrency without coloring: `taskgroup`, `para_map`,
  `#Shield`, `#Context(deadline:)`; no async/await, no mutex, on purpose.
- HTTPS-by-default client; server TLS as a named option; linear-time regex
  that cannot ReDoS; pinned Unicode 16 text floor.
- Checked sized ints with per-op wrapping/saturating/checked overrides.
- Free struct equality and dictionary/string ergonomics (`wordcount.jet`
  is 8 lines).
- Headless graphics defaults (`JET_RAYLIB_DISPLAY`, `JET_UI_HEADLESS`) so
  CI never needs a display.
- Measurement type already prints `9.8 ± 0.1` — proof the unit-print fix
  fits the house style.

## Next actions

Filed after this run at the owner's request (2026-07-28):

- Owner: decide the open ballots.
  - #1267 `D-AUTODERIVE1` and #1268 `D-QUANTITY-PRINT1` (seed run).
  - #1273 `D-HTTP-JSON1`, `D-HTTP-STATIC-FILES1`, `D-HTTP-CORS1`.
  - #1274 `D-WEBAPP-SERVE1`, `D-WEB-CLICK-OWN1`.
  - #1275 `D-UI-MOUNT1`. #1276 `D-GAME-LOOP1`.
  - #1277 `D-CLI-FIELD-MARKERS1`. #1278 `D-FMT-INTERP1`.
  - #1279 `D-ARGS-EXIT1`. #1280 `D-PROCESS-CHECKED1`.
  - #1281 `D-IO-PROMPT1`. #1282 `D-TASKS-LIST1`.
  - #1283 `D-REGEX-LIT1`. #1284 `D-BINREAD-LEN1`.
  - #1285 `D-EXAMPLES-SHORTPATH1` (first-hour → `#CLI`, parallel scan →
    `taskgroup`, streaming text reads, composed log/MMIO/math examples).
- Defect cards, no gate, ready for a burndown: #1269 (CSV encode empty
  under `jet run`), #1270 (`user_` name leak in derived print), #1271
  (panic prints beside diagnostics), #1272 (import-aware and
  stdlib-aware fix hints), #1286 (help usage line prints an absolute
  path), #1287 (finish uninit fixed-array buffer fill through TIR).
- Everything from this audit is now on the board; nothing is left
  report-only.
