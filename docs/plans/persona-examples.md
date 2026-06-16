# Jet Persona Use Cases & CEO Brief

Nine personas across three skill levels, each building something different in Jet. For each: what "magic" looks like at that level, what Jet delivers today (pull), what blocks them (push), and consolidated recommendations.

**Status:** snapshot as of 2026-06-16 (v1.0 shipped; Epoch 2 in progress).

---

## Beginners (first compiled language)

### 1. Maria — high school student, "Guess the Number" game

**Project:** A single-file terminal game: random target, hints ("too high/low"), score counter, play again loop. Uses `std.random`, `std.io.input`, `if`/`while`, `print`.

**Magic she needs:** Run one file instantly; errors that read like a teacher; no setup ceremony; see output change fast when she edits a line.

| Pull (feels magic) | Push (friction) |
|---|---|
| `jet run game.jet` with no manifest or project folder (R9) | Full compile via Rust on every run — slow vs Python/JS REPL |
| Teaching errors if she types `let`, `println`, `match` (S14) | Semicolons everywhere — unlike Python/JavaScript habits |
| Plain-language diagnostics (what/why/fix) | `mut` / ownership when she passes scores between functions |
| `for i in 1..5` inclusive ranges feel natural | No `jet repl` yet (E2-M18) for "try one line" exploration |
| `when` exhaustiveness catches missing game states | LSP helps if she uses VS Code; otherwise docs-only |

---

### 2. James — college student, CSV grade tracker

**Project:** Read `grades.csv`, parse rows, compute averages, print a report. One file first; later split into `parse.jet` + `report.jet` with `import`.

**Magic he needs:** Read a file without scary APIs; clear errors on bad data; a path from "one script" to "small project" without new tooling.

| Pull | Push |
|---|---|
| `fs.read` + `String` methods + `split`/`trim` | Whole-file reads only — large CSVs are awkward (streaming E2-M7) |
| `T ? E` + `?` keeps happy path readable | `main` cannot return `T ? E` — must use `?? return` or `panic` |
| Multi-file `import "path" as alias` (M6) | No `csv` std module — hand-parse or wait for ecosystem |
| `jet test` blocks for unit tests on parsers | Package story exists (M12.1) but registry/resolver (M12.2) immature |
| `jet fmt` — zero config style | Error types don't convert across modules (`?` only same `E` in v1) |

---

### 3. Priya — hobbyist, photo folder organizer

**Project:** CLI that scans a directory, renames `IMG_2024-01-15.jpg` → `2024-01-15.jpg`, moves into year folders. Uses `std.fs`, `std.io.args`, `Map`, closures.

**Magic she needs:** "It just works" on her machine; safe defaults; copy-pasteable examples; confidence she won't corrupt files.

| Pull | Push |
|---|---|
| Ownership + `mut`/`take` prevent accidental double-use of paths | `take` vs default + L0201 clone lint — subtle for beginners |
| Friendly runtime reports on I/O failure (not Rust concepts) | No atomic rollback / `defer` — partial rename on mid-run failure |
| `jetgrep`-class showcase shows real directory walks | Recursive walk is manual (no `walk_dir` in std — she copies patterns) |
| Single native binary after `jet build` | First build waits on `rustc` + hidden cargo bridge if she adds FFI |
| Tasks exist for parallel scan (`34_parallel_scan.jet`) | `jet dev` watch mode not shipped — edit/run loop is compile-heavy |

---

## Intermediate (comfortable with CLI tools, one other language)

### 4. Carlos — DevOps engineer, log tail analyzer

**Project:** `logscan`: flags for `-e` regex, `-c` count, time window filter; reads stdin or files; exit codes like grep. Extends the `jetgrep` showcase pattern.

**Magic he needs:** Fast enough for daily use; familiar CLI ergonomics; tests; multi-file structure; optional deps without bloat.

| Pull | Push |
|---|---|
| Showcase `jetgrep.jet` (253 lines) is a credible grep-lite | No regex std module — string `contains` / hand-rolled matching |
| `std.process.exit(code)` + `io.args` | No streaming stdin reader — buffering whole input (E2-M7) |
| Performance ~Rust class (M14 benchmark story) | Compile time hurts "quick script" habit vs `go run` |
| `jet test` + golden fixtures | No named/default args (owner todo §23) — flag parsing is verbose |
| `--small` profile for smaller binaries (S15) | No `jet dev` for rapid iteration (E2-M4) |

---

### 5. Elena — data analyst, JSON report pipeline

**Project:** Read JSON files (exported from a BI tool), aggregate metrics, emit formatted summary + optional `jsonfmt` output. Pipeline with `map`/`filter`/`reduce`.

**Magic she needs:** JSON without fighting types; pipeline style; reproducible runs; share a script with teammates.

| Pull | Push |
|---|---|
| `std.json` parse/stringify with `JSONError` | No HTTP client — she must download JSON elsewhere (E2-M10) |
| Closure-powered `map`/`filter`/`sort_by` (`33_pipeline.jet`) | JSON API is v1-frozen — no schema/types from JSON |
| `jsonfmt` showcase (56 lines) proves the shape | No `toml`/`yaml` std — configs stay external |
| `jet.toml` + git deps for shared `report.jet` lib (M12.1) | M12.2 registry/resolver not complete — sharing is git-path heavy |
| `derive Serialize` for her structs (S55) | Error conversion across package boundaries still painful |

---

### 6. Tom — indie dev, terminal roguelike (no graphics yet)

**Project:** Grid map, entities, turn loop, save/load to JSON. Uses structs/enums, `when` exhaustiveness, `Map` for dungeon tiles, maybe `tasks` for async input later.

**Magic he needs:** Expressive data modeling; enums + pattern matching; save files; eventually real-time loop without fighting the borrow checker.

| Pull | Push |
|---|---|
| Enums + exhaustive `when` catch missing tile types | No game loop / timing std — manual `while` + blocking `input` |
| Recursive enums for AST-like structures (`13_recursive_enum`) | Can't store references in structs (tier 1) — awkward entity graphs |
| `tasks.spawn` + channels (E2-M1) for future concurrency | No TUI library (no C FFI yet for ncurses/SDL) |
| Comptime lookup tables (`28_comptime_table.jet`) | Raylib/SDL path blocked until `extern c` (E2-M14) |
| Ownership without `&`/`'a` syntax | Large grid as nested `[Tile]` lists — performance tuning unclear |

---

## Experts (Rust/Go/C/Zig experience)

### 7. Marcus — graphics engineer, Raylib prototype game

**Project:** Small 2D game: window, sprite, input, score. Wants `extern c` to Raylib, 60fps loop, ship a binary.

**Magic he needs:** C ABI interop; predictable performance; safe-by-default with an escape hatch; fast iteration while prototyping.

| Pull | Push |
|---|---|
| `extern rust` proves FFI tier works (`22_ffi.jet` + base64) | **No `extern c`** — Raylib blocked until E2-M14 |
| Rust backend = native speed when it compiles | Compile latency kills "tweak sprite position" flow |
| Expert tier plan (S58 `std/mem` + `unsafe`) on roadmap | S58 not shipped — no gated low-level path yet |
| Single binary output + LTO story (R8) | Hidden cargo bridge for Rust FFI — opaque vs Zig's `zig build` |
| Philosophy explicitly targets kernels/embedded post-v1 | No freestanding/`no_std` profile yet |

**Verdict:** Marcus is the clearest "almost, but not yet" expert persona. Jet's identity pitch resonates; the C ecosystem gate is the blocker.

---

### 8. Aisha — senior backend engineer, internal HTTP metrics service

**Project:** Small HTTP server: health check, POST metrics JSON, sqlite persistence, TLS, concurrent requests via tasks/channels.

**Magic she needs:** Go-like networking std; structured errors; concurrency without data races; deploy one binary.

| Pull | Push |
|---|---|
| Tasks + channels, no shared mutable state (E2-M1) | **No `std.http` / sockets** — entire project is staged (E2-M10) |
| Fallible everything matches her Go `if err != nil` mental model | Blocking-only networking plan — no async/await (non-goal v1) |
| JSON + process + fs enough for CLI sidecars today | No streaming I/O — large POST bodies problematic |
| Generics + traits for handler traits | No Postgres/sqlite std yet in v1 std reference |
| Error-as-values beats exception stack traces for services | `?` error conversion across modules — multi-crate services hurt |

**Verdict:** Aisha would choose Go today. Jet's concurrency model is ahead of its networking std.

---

### 9. Dr. Chen — embedded/tooling engineer, C firmware test harness

**Project:** Host-side Jet tool: read hex firmware image, parse sections, verify checksums, emit C header constants via comptime `embed_file`, call vendor C parser via FFI.

**Magic he needs:** C FFI; byte buffers (`U8`); comptime constants; deterministic builds; cross-compile story eventually.

| Pull | Push |
|---|---|
| `embed_file` bakes assets into binary (`29_embed.jet`) | C FFI deferred — can't call vendor `.a` cleanly |
| `U8` byte buffers + `read_bytes` | Tier-1 ownership — zero-copy parsing needs tier-2 refs |
| Comptime table generation without macros | No `volatile`/MMIO (S58 expert tier) |
| `jet build --small` for deployable tools | No freestanding profile for on-device Jet |
| Purity / `jet eval --pure` on roadmap (S60) for config | Package audit/vendor offline story still maturing (E2-M8) |

**Verdict:** Dr. Chen is a long-horizon fit (philosophy 2026-06-12). v1 delivers the safe CLI half; the embedded half needs C FFI + tier-2 + freestanding.

---

## Cross-persona matrix

| Need | Beginners | Intermediate | Experts | Jet today |
|------|-----------|--------------|---------|-----------|
| Zero-ceremony run | Critical | Nice | Nice | **Strong** (`jet run`) |
| Teaching diagnostics | Critical | Nice | Low | **Strong** (best pull factor) |
| Fast edit-run loop | Critical | Critical | Critical | **Weak** (no `jet dev`/REPL) |
| File/string tooling | Critical | Critical | High | **Good** (whole-file only) |
| JSON/data pipelines | Medium | Critical | High | **Good** |
| HTTP/networking | Low | High | Critical | **Missing** |
| C / game / embedded libs | Low | Medium | Critical | **Missing** (`extern c`) |
| Package ecosystem | Low | High | High | **Partial** (M12.1 done, M12.2 next) |
| Zero-copy / graphs | Low | Medium | Critical | **Weak** (tier-1 refs) |

---

## Push factors — full inventory (broccoli before dessert)

Every push factor from the persona tables above, deduplicated and sorted by **layer of the stack**. Fix core language semantics and surface **before** std modules, tooling, or ecosystem — wrong choices at the language layer compound across every library and tool built on top.

Legend: personas cited as **M** Maria, **J** James, **P** Priya, **C** Carlos, **E** Elena, **T** Tom, **Ma** Marcus, **A** Aisha, **D** Dr. Chen.

### 1. Core language (syntax, semantics, types, ownership, errors)

These are compiler/sema/spec changes. Highest blast radius — get them right before shipping more std or DX.

| Push factor | Personas | Notes / existing hooks |
|-------------|----------|------------------------|
| **`?` error conversion across modules** — propagation only works when `E` matches; multi-file and multi-package programs force manual `when` mapping | J, E, A | Roadmap committed addition #3; Generics v1.5 / `From`-equivalent TBD |
| **`main` cannot return `T ? E`** — CLI entry must use `?? return`, `when`, or `panic` | J | Forces awkward top-level error handling in the most common beginner program shape |
| **Ownership ergonomics for beginners** — `mut`/`take` confusion when passing values between functions; L0201 clone lint on `take` vs default | M, P | Tier-1 model is correct; may need better diagnostics, more teaching examples, or call-site sugar — not a different borrow checker |
| **`defer` / cleanup primitive** — no rollback on failure mid-I/O batch (file organizer leaves partial renames) | P | Owner todo §0.1; pairs with `?` and proposed `transact` |
| **Named + default arguments** — flag parsers and multi-arg calls are verbose | C | Owner todo §23 |
| **Optional-chaining / unwrap ergonomics** (`?.`, guard/`if let` spellings) — round out `T?` / `??` | — | Owner todo §12; not cited by a persona table but recurring in owner backlog |
| **Pipelines (`\|>`)** — collection transforms readable without nested calls | — | Owner todo §15 |
| **Tier-2 stored/returned references** — entity graphs, parent pointers, zero-copy parse buffers | T, D | E2-M5; biggest unlock for Rust-territory programs; tier 1 intentionally blocks this |
| **Expert low-level tier (S58)** — `volatile`, MMIO, layout, allocators behind `import std/mem` + `unsafe` | Ma, D | Ratified; E2-M13; not in onboarding |
| **`extern c` surface** — manual `extern c` blocks (C ABI imports) | Ma, T, D | E2-M14; distinct from `extern rust`; gateway to Raylib/SDL/vendor `.a` |
| **Fixed-size lists `[T#N]` + fan-out `.[…]`** — grids and homogenous literals without nested dynamic lists | T | Ratified in `fan-out-and-fixed-size-lists.md`; in progress |
| **JSON ↔ typed struct bridge** — no schema inference or typed decode at the language/stdlib boundary today | E | Could be std `json` enhancement *or* a comptime/reflection layer; decide at language boundary before bolting on libs |
| **Semicolons required** — unlike Python/JS habits | M | Intentional (S6); not a bug — teaching errors help; optional semicolons would be a major syntax ratchet decision |
| **Async/await absent** — blocking-only concurrency story | A | Intentional non-goal v1; tasks+channels are the Jet model; document clearly so Aisha doesn't expect Go-style net stack + async |

**Core language — recommended broccoli order** (dependencies first):

1. Error conversion for `?` (unblocks every multi-module program)
2. `defer` / cleanup (pairs with errors; reduces beginner I/O anxiety)
3. Named + default arguments (high read impact, localized surface)
4. Optional-chaining / guard ergonomics for `T?`
5. Fixed-size lists + fan-out (ratified — ship)
6. Tier-2 references (large sema project — schedule after error/defer ergonomics)
7. `extern c` (language surface before C-ecosystem libs)
8. Expert tier S58 (gated; after `extern c` boundary is stable)
9. Pipelines, `transact`, purity (S60) — owner-flagged; after baseline ergonomics land

---

### 2. First-party std library (`std.*` / `jet.std`)

Built on the language. Wrong language choices here get copied into every module signature — but these are still *less* foundational than §1.

| Push factor | Personas | Module / milestone |
|-------------|----------|-------------------|
| **Whole-file reads only** — large CSVs, big logs awkward | J, C, A | Streaming I/O — E2-M7 |
| **No streaming stdin reader** — must buffer entire input | C | E2-M7 |
| **No `walk_dir` / recursive directory helper** — every tool copies manual walk logic | P, C | `std.fs` extension or thin `std.walk` |
| **No `std.regex`** — grep-class tools hand-roll matching | C | E2-M9 first-party libs or `std.regex` |
| **No `std.csv`** — tabular data hand-parsed | J | E2-M9 or ecosystem |
| **No HTTP client / sockets** | E, A | E2-M10 networking |
| **No `std.http` server** | A | E2-M10 |
| **No sqlite / database std** | A | E2-M10 showcase deps |
| **No `toml` / `yaml` std** | E | E2-M9 first-party libs |
| **No game-loop / timing helpers** — manual `while` + blocking `input` | T | `std.time` exists; loop/sleep/input polling patterns not packaged |
| **JSON API frozen v1** — no typed decode path documented for analysts | E | Extend `std.json` after language error/typing hooks clear |

**Stdlib — recommended order** (after §1 error conversion + streaming language hooks):

1. Streaming I/O (`std.io` readers/writers) — E2-M7
2. `fs.walk` / directory recursion — small, unblocks showcases
3. `std.regex` — unblocks jetgrep-class tools
4. HTTP client + minimal server — E2-M10
5. `std.csv`, `std.toml`, `std.yaml` — E2-M9
6. sqlite / service persistence — E2-M10 showcase
7. JSON typed decode — after §1 JSON/struct bridge decision

---

### 3. Tooling & compiler driver (not language semantics)

Faster iteration and transparent builds. High user impact but **does not** fix wrong semantics — ship after core language is stable.

| Push factor | Personas | Milestone |
|-------------|----------|-----------|
| **Full Rust compile on every `jet run`** — slow vs REPL/script languages | M, Ma, all | E2-M4 `jet dev` + interpreter |
| **No `jet repl`** — can't explore one expression at a time | M | E2-M18 (blocked D-REPL1…21) |
| **No `jet dev` watch** — edit/run loop compile-heavy | P, C | E2-M4 |
| **First build slow** — `rustc` + hidden cargo bridge for FFI | P, Ma | E2-M3 `jet doctor`, build transparency |
| **Hidden cargo bridge opaque** — vs Zig's explicit build | Ma | E2-M3 / FFI docs |
| **No freestanding / `no_std` profile** — on-device or kernel Jet | Ma, D | E2-M15 |
| **LSP only helps in VS Code** — otherwise docs-only for Maria | M | Docs + editor extension distribution (M13 shipped; outreach) |

**Tooling — recommended order** (parallel track once §1 error/defer land):

1. `jet dev` phase 1 (interpreter-backed watch) — E2-M4
2. `jet repl` — E2-M18 after interpreter shares E2-M4 foundation
3. `jet doctor` + clearer FFI/build messages — E2-M3
4. Freestanding profile — E2-M15 (expert horizon)
5. JIT dev execution (optional) — philosophy #10 phase 2; owner-gated

---

### 4. Package manager & distribution (`jetpack`, registry, lockfile)

Opt-in for single files (R9); critical for Elena sharing libs and Dr. Chen reproducible builds.

| Push factor | Personas | Milestone |
|-------------|----------|-----------|
| **Registry / resolver immature (M12.2)** — sharing via git paths only | J, E | M12.2 / E2-M8 |
| **Package audit / vendor offline immature** | D | E2-M8 supply chain |
| **jet.toml → pack.jet migration in flight** | E | `unified-ecosystem.md` |

**Package — recommended order:**

1. Finish M12.2 resolver + registry snapshots
2. Vendor / offline / audit — E2-M8
3. Unified `pack.jet` / `env.jet` ecosystem — jetpack track

---

### 5. Third-party & FFI ecosystem (enabled by language + std, not Jet-owned)

Cannot ship until `extern c` (§1) and often streaming/http (§2) exist. Dessert.

| Push factor | Personas | Depends on |
|-------------|----------|------------|
| **Raylib / SDL blocked** | Ma, T | `extern c` E2-M14 |
| **ncurses / TUI libs blocked** | T | `extern c` |
| **Vendor C `.a` / firmware parsers blocked** | D | `extern c` |
| **Performance tuning unclear for large grids** | T | Fixed-size lists §1 + docs/examples |

---

### 6. Intentional gaps (document, don't "fix" without owner sign-off)

| Item | Why it's not a bug |
|------|-------------------|
| Semicolons (S6) | Readability + fmt simplicity |
| No async/await | Tasks + channels (S53); non-goal v1 |
| Tier-1 no stored refs | Progressive disclosure (C1); tier 2 is the path |
| `main` no fallible return | Ballot **D-ERR3** pending — not ratified yet |

---

## Crosscheck against `docs/spec/decision-ballots.md`

Every push factor mapped to an existing ballot, milestone plan, ratified decision, or a **new pending ballot** (Groups 20–23 below). **Broccoli order** within each status column.

Legend: ✅ planned (ballot or plan exists) · 🟡 partial (ratified/in progress, not shipped) · 📋 intentional (document, don't fix) · 🆕 **new ballot** (added to decision-ballots.md Part 3)

### Core language

| Push factor | Status | Where |
|-------------|--------|-------|
| `?` error conversion across modules | ✅ | **D-ERR2**, D-LIB3 → E2-M6 |
| `main` cannot return `T ? E` | ✅ | **D-ERR3** → E2-M6 |
| Ownership ergonomics (`mut`/`take`, L0201) | 🆕 | **Group 20** (D-OWN1…3) |
| `defer` / cleanup on failed I/O batch | ✅ | **D-IO2** + **D-SUGAR5** (Rec: RAII, not `defer` keyword); adjacent **D-TXN1…3** |
| Named + default arguments | ✅ | **D-LIB1** / S61 → E2-M6 |
| Optional-chaining (`?.` methods, guards) | 🟡 | S71 ratified; field `?.` shipped; method `?.` staged (**D-SUGAR6**); refutable binds **D-PAT3** |
| Pipelines `\|>` | ✅ | **D-SUGAR2** (Rec: defer — S69 dot-chains) |
| Tier-2 stored/returned references | ✅ | E2-M5, **D-REF1…3** |
| Expert low-level tier S58 | ✅ | E2-M13, **D-LL1…3** |
| `extern c` surface | ✅ | E2-M14, **D-CFFI1…3** |
| Fixed-size `[T#N]` + fan-out `.[…]` | 🟡 | `fan-out-and-fixed-size-lists.md` — ratify + ship |
| JSON ↔ typed struct bridge | 🆕 | **Group 22** (D-JSON1…2) |
| Semicolons required | 📋 | S6 — optional semicolons = **D-SUGAR7** if ever reopened |
| No async/await | 📋 | **E2-V5**, S53 — document for Aisha persona |

### First-party std

| Push factor | Status | Where |
|-------------|--------|-------|
| Whole-file reads only | ✅ | E2-M7, **D-IO3** (keep as sugar) |
| No streaming stdin | ✅ | E2-M7 (`stdin` as streaming reader) |
| No `walk_dir` / recursive walk | 🆕 | **Group 21** (D-FS1) — or ship in E2-M7 scope |
| No `std.regex` | ✅ | E2-M9 ring, **D-LR1** (second wave after csv/toml) |
| No `std.csv` | ✅ | E2-M9, **D-LR1** first wave |
| No HTTP client / server | ✅ | E2-M10, **D-NET1…3**, **E2-V7** |
| No sqlite | ✅ | **D-LR2**, **D-NET3** → E2-M14 timing |
| No `toml` / `yaml` | 🟡 | `jet.toml` in **D-LR1** first wave; yaml **D-LR4** (defer default) |
| No game-loop / timing packaging | 🆕 | **Group 21** (D-FS2) — examples vs `jet.time` helpers |
| JSON API frozen / no typed decode | 🆕 | **Group 22** (D-JSON1…2) |

### Tooling

| Push factor | Status | Where |
|-------------|--------|-------|
| Rust compile every `jet run` | ✅ | E2-M4 **jet dev**, **D-DEV1…3** |
| No `jet repl` | ✅ | E2-M18, **D-REPL1…21** |
| No `jet dev` watch | ✅ | E2-M4 |
| First build slow (FFI) | 🟡 | **D-DX2** `jet doctor` (partial) |
| Hidden cargo bridge opaque | 🆕 | **Group 23** (D-BUILD1…2) |
| No freestanding / `no_std` | ✅ | E2-M15, **D-CROSS1…3**, **E2-V6** |
| LSP / editor reach | ✅ | **E2-V9**, **D-DX3** (Zed dev extension) |

### Package / distribution

| Push factor | Status | Where |
|-------------|--------|-------|
| Registry / resolver immature | ✅ | M12.2 / E2-M8, **D-PKGS1…4**, **E2-V8** |
| Audit / vendor offline | ✅ | E2-M8, **E2-V8** option B |
| `jet.toml` → `pack.jet` migration | ✅ | **D-JPK*** (Group 11 ratified), `unified-ecosystem.md` |

### Third-party ecosystem (enabled, not Jet-owned)

| Push factor | Status | Where |
|-------------|--------|-------|
| Raylib / SDL / ncurses | ✅ | After **E2-M14** `extern c` |
| Vendor C `.a` | ✅ | **E2-M14**, **D-CFFI2** |
| Grid perf tuning unclear | 🟡 | Fixed-size lists + **D-FS2** showcase guidance |

### Gaps summary — owner queue only

| New group | IDs | Decide before |
|-----------|-----|---------------|
| 20 — ownership teaching | D-OWN1…3 | E2-M6 window (pairs with labels/defaults) |
| 21 — core std helpers | D-FS1, D-FS2 | E2-M7 exit (walk) / E2-M9 (timing) |
| 22 — JSON typed decode | D-JSON1…2 | E2-M6 (errors) + ring `jet.json` |
| 23 — build transparency | D-BUILD1…2 | E2-M3 / E2-M7 FFI path |
| 17 extension | D-SUGAR6, D-SUGAR7 | S71 completion / semicolon reopen |
| 9 extension | D-LR4 | E2-M9 yaml timing |

Everything else in the inventory already has a ballot row or ratified ID. **Decide Groups 14 (errors) and 20 (ownership) first** — they gate the broccoli stack.

---

## Recommendations for vision & roadmap priority (broccoli-first)

Previous ordering led with `jet dev` and networking. **Revised:** language semantics first, then std, then tooling, then ecosystem dessert.

### Broccoli — core language (do first)

1. **Error conversion for `?`** — James, Elena, Aisha; every package
2. **`defer` / cleanup** — Priya; all I/O-heavy CLIs
3. **Named + default arguments** — Carlos; universal readability
4. **Optional-chaining / guard ergonomics** — owner todo §12
5. **Fixed-size lists + fan-out** — ratified; ship in progress
6. **Tier-2 references** — Tom, Dr. Chen; schedule after #1–2
7. **`extern c`** — language surface before Raylib/vendor C
8. **Expert tier S58** — Dr. Chen MMIO; gated

### More broccoli — first-party std (on stable language)

9. **Streaming I/O** — E2-M7
10. **`fs.walk`** — small win, high copy-paste reduction
11. **`std.regex`**
12. **HTTP client + server** — E2-M10
13. **`std.csv`, `toml`, `yaml`, sqlite** — E2-M9 / E2-M10

### Side dish — tooling (high impact, doesn't change semantics)

14. **`jet dev` + interpreter** — E2-M4 (every persona; parallel once §1 #1–2 land)
15. **`jet repl`** — E2-M18
16. **Build transparency / `jet doctor`** — E2-M3

### Dessert — ecosystem & long horizon

17. **Package registry M12.2 + vendor/audit** — E2-M8
18. **Third-party C libs** (Raylib, SDL, ncurses) — after `extern c`
19. **Freestanding profile** — E2-M15
20. **Pipelines, `transact`, purity eval layer 3** — owner-flagged fun stuff

### Protect the pull factors (do not trade away)

- Single-file `jet run` forever (R9).
- Diagnostic voice — no Rust vocabulary leakage (I2, `docs/spec/diagnostics.md`).
- Ownership without `&`/`'a` in tier 1.
- "Pay for what you call" std codegen.
- Teaching errors for foreign syntax (S14).
- Zero-config `jet fmt`.

---

## CEO summary

**Jet already feels magical** for beginners writing first compiled programs and for intermediate users building CLI/JSON tools in one or a few files: diagnostics, ceremony-free run, errors-as-values, and ownership without lifetime syntax are genuine differentiators. The three showcase tools (jetgrep, jsonfmt, wordfreq) are credible proof.

**The push factors cluster into four layers** (see § Push factors — full inventory):

1. **Core language** — error conversion, `defer`, named args, tier-2 refs, `extern c` (fix semantics before building on top)
2. **First-party std** — streaming I/O, regex, HTTP, csv/toml (stdlib on stable language)
3. **Tooling** — `jet dev`, REPL, compile transparency (high impact, doesn't change semantics)
4. **Ecosystem dessert** — registry maturity, Raylib, freestanding

**Persona outcomes today:**

| Persona | Ships in Jet today? |
|---------|---------------------|
| Maria (game) | Yes, with compile-wait pain |
| James (CSV) | Yes |
| Priya (organizer) | Yes, with rollback anxiety |
| Carlos (logscan) | Yes, minus regex/streaming |
| Elena (JSON pipeline) | Yes for offline JSON |
| Tom (roguelike TUI) | Partial — logic yes, TUI/libs no |
| Marcus (Raylib) | **No** — needs `extern c` |
| Aisha (HTTP service) | **No** — needs E2-M10 |
| Dr. Chen (embedded harness) | **Partial** — CLI half yes, C FFI no |

**Strategic framing:** v1 Jet wins the "second language" and "small safe CLI tool" stories. **Broccoli before dessert:** ratify and ship core language ergonomics (error conversion, `defer`, named args) before std expansion and before `jet dev` — wrong error and ownership ergonomics compound into every library signature. Then streaming + HTTP std, then iteration-speed tooling, then C-ecosystem dessert (Raylib, vendor libs). The diagnostic voice and single-file run path remain the brand.

---

## References

- `docs/spec/decision-ballots.md` (crosscheck + new Groups 20–23)
- `docs/spec/philosophy.md`
- `docs/spec/roadmap.md`
- `docs/reference/stdlib.md`
- `docs/plans/owner-todo.md`
- `examples/showcase/` (jetgrep, jsonfmt, wordfreq)
- `docs/plans/persona-examples.md` (persona push-factor crosscheck)
- `docs/plans/epoch-2/README.md`
