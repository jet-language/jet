# c123 — Ship flagship vertical slices per domain
**Decision:** none required for the plan itself. See per-slice gate notes below.
**Gate:** each slice notes its specific language-feature gates.

---

## What a vertical slice proves

Each slice is an end-to-end program that a real user would ship: real I/O, real error
handling, packaged, tested, diagnosed, documented. A slice is not a toy example — it
demonstrates that Jet's platform story is coherent for that domain. Each slice must:

1. Compile with `nix develop -c jet build`.
2. Have a golden test (I5): expected output committed; CI enforces it.
3. Use only ratified language features (no syntax speculation).
4. Include at least one `#Test` block.
5. Be packaged (`pkg.jet`).
6. Include `///` doc comments on every public fn.

---

## Slice 1 — CLI: `jetgrep` (already started; harden it)

**File:** `examples/showcase/jetgrep.jet` (exists)
**Domain:** command-line tools

**What it proves:** argument parsing (D-ARGS1), streaming stdin (D-STDIN1), `fs.list_dir`
(D-LSDIR1), error handling (`?`), `#Test` blocks.

**Plan:**
- Add `pkg.jet` manifest to `examples/showcase/jetgrep/`.
- Add `#Test` blocks for the core match logic (unit tests; D-TEST1).
- Add `///` doc comments on all fns.
- Add `--count` flag (count matching lines) using D-ARGS1 builder spec.
- Add `--recursive` flag that walks directories via `fs.list_dir` (D-LSDIR1).
- Expected output golden test: `tests/showcase/jetgrep.rs`.
- Gate: D-ARGS1 (ratified), D-STDIN1 (ratified), D-LSDIR1 (ratified). Unblocked.

---

## Slice 2 — Server: `wordfreq-server`

**File:** `examples/showcase/wordfreq-server/` (new; `examples/showcase/wordfreq.jet` exists
as a CLI version — extend it to a server)
**Domain:** HTTP server

**What it proves:** `jet.http` route registration (D-ROUTE1), JSON output (D-JSONOUT1 / serde
model), streaming request body, error types, hot-reload (D-HOTSWAP1 when `jet dev`).

**Plan:**
- Create `examples/showcase/wordfreq-server/main.jet`.
- Routes: `POST /analyze` — accepts a text body, returns `{"top": [["word", 5], …]}` (top-10
  words by frequency).
- `GET /health` — returns `{"ok": true}`.
- `GET /stats` — returns aggregate stats since server start.
- Use `#[Serialize]` (S55, existing) for the response structs.
- `#Test` blocks for the word-counting logic.
- `pkg.jet` with `jet.http` as a dep.
- Golden test: spin up server, `curl`, assert JSON output shape.
- Gate: D-ROUTE1 (ratified 2026-06-22). Partially gated on the `jet.http` ring library being
  functional — check current state in `examples/showcase/http_service.jet` (it lives under
  `showcase/`, not `features/`) and the `jet.http` ring impl in `Source/Prelude/CoreLib.rs` +
  its registration in `Source/Loader.rs` (there is **no** `Source/ring/http/` directory) before
  starting.

**Dependency (not a ballot):** the hot-reload demo needs `jet dev` to detect this as a resident
program and hot-swap — that is c77 *implementation* (D-DEVMODE1/D-HOTSWAP1 are already ratified),
not an open decision. The core slice lands in parallel with c77; the full hot-reload demo waits
on c77.

---

## Slice 3 — Low-level / freestanding: `freestanding` (already started; harden it)

**File:** `examples/showcase/freestanding.jet` (exists)
**Domain:** systems / embedded / no-OS

**What it proves:** `@unsafe`, `core.mem`, `#layout(c)` (D-REPRC1), absence of stdlib,
no-alloc mode, `#[Serialize]` binary (D-SERDE1 binary adapter when available).

**Plan:**
- Add `pkg.jet` with `no_std: true` (check `Source/Manifest.rs` for existing field or add).
- Demonstrate: a C-ABI struct (`#layout(c)`), a raw memory write (`@unsafe`), a fixed array
  (`[U8#N]`, D-FIXARR1).
- `#Test` blocks for the pure logic (can run on host even in freestanding build).
- Add `///` doc comments.
- Golden test: compile succeeds; output binary has zero stdlib symbols (check with `nm`
  subprocess in the test).
- Gate: D-REPRC1 (ratified). D-SG9 sized integers for `U8#N` (check if D-SG9 is implemented;
  dsg9-sized-integers-impl.md sidequest exists — check its status).

---

## Slice 4 — Data pipeline: `typed-csv-pipeline`

**File:** `examples/showcase/typed-csv-pipeline/` (new)
**Domain:** data processing

**What it proves:** typed CSV reading (D-CSVROW1 / serde model), structured arg parsing
(D-ARGS1), iterator adapters (D-ITER1), JSON output (D-JSONOUT1), error reporting.

**Plan:**
- `jet csv-pipeline input.csv --filter "score > 80" --output result.json`
- Reads a CSV with header row; decodes each row into a typed struct; filters; emits JSON.
- Uses `D-ITER1` adapters (filter, map, collect).
- `#Test` blocks for the filter logic.
- `pkg.jet` with `jet.csv` and `jet.json` deps.
- Golden test: `input.csv` committed; `result.json` committed; CI diffs.
- Gate: D-CSVROW1 folded into D-SERDE1 (serde-model plan Phases 1–3, unblocked). D-ITER1
  (ratified). D-ARGS1 (ratified). Blocked on serde Phase 1–3 being landed.

---

## Slice 5 — Game loop: `terminal-snake`

**File:** `examples/showcase/terminal-snake/` (new)
**Domain:** interactive / game

**What it proves:** `live { }` terminal input (D-TERM1), tasks/channels (if available),
deterministic game loop, fixed arrays (`[U8#N]`), ANSI output.

**Plan:**
- Classic terminal snake game: WASD/arrow keys via `live { }` (D-TERM1), grid as
  `[[Cell#W]#H]` fixed 2D array, draw loop using ANSI escape codes via `print`.
- Score display; game-over screen.
- `#Test` blocks for collision detection and direction logic (pure fns, no terminal I/O).
- `pkg.jet` manifest.
- Golden test: deterministic replay mode (`--replay moves.txt`) produces a fixed output;
  committed and CI-enforced.
- Gate: D-TERM1 (ratified 2026-06-22); `live { }` implementation in
  `terminal-raw-mode.md` sidequest — check its status. D-SG9 for fixed array spelling.

**NEEDS BALLOT:** Does the replay/deterministic mode require a new flag or a standard pattern?
This is not a language decision — `--replay` is a CLI flag on the example program, not a
compiler feature. No ballot needed; implement it directly.

---

## Delivery order

1. `jetgrep` (unblocked; just hardening) — ship first.
2. `typed-csv-pipeline` (unblocked after serde Phase 1–3; parallel with serde-model plan).
3. `wordfreq-server` (unblocked after `jet.http` ring is functional; coordinate with c83).
4. `freestanding` hardening (unblocked; coordinate with D-SG9 status).
5. `terminal-snake` (gated on D-TERM1 impl from terminal-raw-mode.md).

---

## Files touched

| File | Change |
|------|--------|
| `examples/showcase/jetgrep/` | pkg.jet, tests, docs, flags |
| `examples/showcase/wordfreq-server/` (new) | full server slice |
| `examples/showcase/freestanding/` | pkg.jet, tests, docs |
| `examples/showcase/typed-csv-pipeline/` (new) | CSV→JSON pipeline |
| `examples/showcase/terminal-snake/` (new) | terminal game |
| `tests/showcase/` | golden test harness per slice |

---

## Decision verdict

No decision needed for the plan itself.

Per-slice gates:
- `jetgrep`: no decision needed — UNBLOCKED.
- `wordfreq-server`: gated on c77 (three-mode-execution) for hot-reload demo; core slice is unblocked.
- `freestanding`: check D-SG9 sidequest status; otherwise UNBLOCKED.
- `typed-csv-pipeline`: gated on serde-model plan Phases 1–3.
- `terminal-snake`: gated on D-TERM1 implementation (terminal-raw-mode.md sidequest).

**NEEDS BALLOT: D-BENCH1** (from perf-compiletime-dashboards.md) — if benchmark blocks are
added to slices as performance demos. Not required for initial slice delivery.
