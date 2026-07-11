# Flagship vertical slices — Epoch 3 exit criterion (c123 / card #9)

**Owner conversion (2026-07-02):** each pillar ships its flagship slice as an
Epoch 3 exit criterion — "a slice epoch scheduled after everything is a slice
that never happens." Pillars per the card body: **CLI, server,
low-level/freestanding, web, game.**

**Supersedes** `../../sidequests/flagship-vertical-slices.md` (pre-conversion
draft; referenced the retired `examples/showcase/` tree and a data-pipeline
slice in place of the owner's web pillar).

**Ballots:** which app each pillar ships is owner-facing product identity —
D-FLAGSHIP1 (CLI), D-FLAGSHIP2 (server), D-FLAGSHIP3 (web), D-FLAGSHIP4 (game)
are drafted ballot-ready. The freestanding slice has one credible shape
(bare-metal QEMU) and is decided here. Recommended candidates below; no slice
code is written until its ballot is ratified.

---

## The bar — what "slice shipped" means

A slice is a program a real user would ship, not a feature demo. Every slice
meets all seven proofs before its pillar counts:

| Proof | Concretely |
|---|---|
| **Docs** | `README.md` in the slice dir (build / run / test / deploy walkthrough); `///` on every public fn; doctests green via `jet test` |
| **Tests** | `#Test` blocks on the core logic; property tests where the domain has invariants; all in the suite |
| **Golden (I5)** | a deterministic mode with committed expected output, enforced by `tests/slices.rs` |
| **Packaging** | `pkg.jet` manifest; `jet registry publish` pre-publish gate green (build + tests + API diff); SBOM/`jet registry vendor` where deps exist |
| **Diagnostics** | ≥1 new `tests/ui` fixture per slice: a realistic mistake from that domain, with the code/what/why/fix voice (I4) |
| **Performance** | a `jet bench` target with a pinned regression budget, plus a binary/bundle size budget in `tests/release_gates.rs` style |
| **Deployment** | a working deploy artifact proven by test: cross-compiled binary, QEMU boot, static web `build/`, or native binary with system-dep story |

No partial slices (philosophy: do it right the first time). A slice blocked on
an unratified upstream decision names the gate and stops; it does not stub.

## Location and harness (decided here — engineering, not owner-taste)

- Slices live at **`examples/apps/<name>/`**: `main.jet` (+ modules),
  `pkg.jet`, `README.md`, `expected/` outputs, fixtures. `examples/features/`
  stays single-feature examples; `canon.jet` stays the syntax showcase.
- New **`tests/slices.rs`** harness. `tests/golden.rs` stays pinned to
  `examples/features/`; slices need domain drivers golden.rs can't express:
  stdout-golden (CLI), HTTP probe loop (server), QEMU serial capture
  (freestanding), web artifact + bundle check (web), headless replay (game).
- The raylib plan's `examples/showcase/` reference
  (`../../sidequests/raylib-graphics.md` step 5) lands under
  `examples/apps/` instead.
- Nondeterminism is engineered out for goldens: fixed RNG seeds, `--replay`
  input files, injectable clocks (D-MOTIONTIME1 pattern), ephemeral ports.

---

## Slice 1 — CLI (ballot D-FLAGSHIP1; rec: `jetgrep`)

**App (recommended):** `jetgrep` — recursive regex search: patterns, globs,
ignore rules, `--count`/`--files`/`--context`, colored TTY output, piped-mode
detection, correct exit codes. Grown from the E2 GA showcase into a tool
someone would alias over `grep`.

**Why credible:** ripgrep is the canonical proof a language is fast and
pleasant for CLI work; a head-to-head on a pinned corpus is the
highest-signal CLI benchmark that exists. Heritage code already GA'd once
(E2-M17), so the slice measures polish, not feasibility.

**Platform surface:** args spec (D-ARGS1), streaming stdin (D-STDIN1),
`fs`/dir walk, `core.regex`, TTY color detection (E2-M3 conventions), exit
codes, `core.log`, parallel directory scan (tasks), `--small` profile.

**Gaps it will expose:** regex engine throughput vs ripgrep; dir-walk +
ignore-file ergonomics; large-file/mmap story; arg-spec UX at real flag
counts.

**Slice-specific proofs:** golden = fixed corpus dir committed, outputs
diffed; bench = wall-clock vs GNU grep on the corpus (reported) + pinned
self-regression budget; deploy = `jet build --release --target <triple>`
cross artifacts + `--small` size budget; ui fixture = a bad regex literal
diagnostic.

## Slice 2 — Server (ballot D-FLAGSHIP2; rec: `jetpaste`)

**App (recommended):** `jetpaste` — a pastebin service: `POST /paste` returns
a short id, `GET /p/<id>` serves raw or HTML, expiry via TTL, `GET /stats`,
`GET /health`. SQLite persistence, structured logs, graceful shutdown.

**Why credible:** persistence + CRUD + expiry + concurrent clients is the
minimum honest web service; every behavior is observable with `curl`, so the
whole slice goldens cleanly.

**Platform surface:** `core.http` server + routes (D-ROUTE1), tasks/taskgroup
(#126 model), `#[Serialize]`/serde JSON, `core.db` SQLite, `core.time` TTL,
`core.log` structured output, `#Context(deadline:)` per request, `jet dev`
loop during development.

**Gaps it will expose:** graceful-shutdown/signal API; request-scoped
deadline ergonomics; connection scale before the D-MNIO1 native parkers land;
TLS is `core.tls` package-only (plain HTTP in-core); registry-less dep story
(`jet registry publish` upload still deferred, D-PKGS1).

**Slice-specific proofs:** golden = probe script drives a scripted
curl-sequence against an ephemeral port, responses diffed (timestamps/id
normalized via fixed seed); bench = requests/s + latency smoke with pinned
budget — the 100k-connection target stays on #126's exit criteria, not this
slice's; deploy = single static binary cross-compiled, run-from-scratch test;
ui fixture = sending a non-`Serialize` type as a JSON response.

## Slice 3 — Low-level / freestanding: `metal` (decided; no ballot)

**App:** `examples/apps/metal/` — a bare-metal QEMU image: boots without OS
or stdlib, prints a banner over UART via volatile MMIO, then streams a fixed
number of Game-of-Life generations as serial frames and halts. Deterministic
end to end.

**Why credible:** boot-to-serial-output on hardware-shaped I/O is the
canonical embedded/kernel proof; Game-of-Life gives real computation and a
byte-exact serial transcript to golden.

**Platform surface:** `jet build --freestanding` + `--target` (E2-M15),
`#Unsafe("reason")` gates + `use core.mem` + `*T`/volatile (E2-M13, I1
amendment D-LL1), `#layout(c)`, fixed arrays `[U8#N]` (S76), panic-handler
story, no-alloc code.

**Gaps it will expose:** `core.mem` API breadth (D-LL3 — open ballot; this
slice is its forcing function), arenas/allocators (D-REF2 — open), panic
behavior without an OS, linker-script/entry-point ergonomics.

**Slice-specific proofs:** golden = QEMU serial transcript committed and
diffed (E2-M15 QEMU smoke extended); size budget = image bytes pinned;
`nm`-style check proves no stdlib symbols; deploy proof = the QEMU boot
itself; ui fixture = raw pointer deref outside an `#Unsafe` gate (E310x
voice); host-runnable `#Test`s on the pure Life logic.

## Slice 4 — Web (ballot D-FLAGSHIP3; rec: `jettasks`, a TodoMVC)

**App (recommended):** `jettasks` — the TodoMVC spec in Jet: add/toggle/edit/
delete/filter/clear-completed, count footer, keyboard interaction, persisted
to browser storage.

**Why credible:** TodoMVC is the cross-framework benchmark — the one app
readers can compare line-for-line against React/Svelte/Swift UI code they
already know. Nothing else buys that comparison.

**Platform surface:** browser target `wasm32-unknown-unknown` + HTML pairing
(D-WEBDEFAULT1/D-HTMLPAIR1), `jet dev --target=web` live reload (c134 Phase
7), reactive signals/derived/effect, typed view tree (D-UITREE1), typed
styles (D-STYLESHAPE1), component kit, `jet lint --a11y` (D-A11YGATE1=B),
DOM events via `Prelude/DomRuntime.js`.

**Gaps it will expose:** UI-stack phases 2–6 under a full app (the pillar's
whole point); browser storage bridge; WASM bundle size; JS-interop seams;
event-model completeness (keyboard, focus).

**Slice-specific proofs:** golden = view-tree render against the null/TUI
backend for committed frames + built `build/` artifact check; perf = WASM
bundle size budget; deploy = static `build/` folder served as-is, verified by
the dev-server test; a11y = `jet lint --a11y` green is part of the slice's
suite; ui fixture = a reactive write outside a `#Reactive` scope (E291x
voice).

## Slice 5 — Game (ballot D-FLAGSHIP4; rec: `jetfighter`)

**App (recommended):** `jetfighter` — an asteroids-style 2D shooter on
`core.raylib` (D-RAYLIB1=A): window, sprite rendering, keyboard input, sound
effects, collision, score, fixed-timestep loop, `--replay` deterministic
mode.

**Why credible:** a playable, audible, windowed game is the highest-signal
"the language is real" demo there is, it is on-brand for Jet, and raylib's
own showcase culture makes the comparison legible to game programmers.

**Platform surface:** `core.raylib` FFI-bridge package (stdlib bridge
pattern, I6-safe), RAII resource wrappers (`Window`/`Texture`/`Sound`),
`@embed` for assets, fixed-timestep loop with injectable clock, fixed arrays
for entity pools, input handling, `core.random` seeded.

**Gaps it will expose:** bridge completeness beyond hello-window (textures,
audio, timing); headless/CI rendering mode; asset embedding at real sizes;
frame-time behavior of generated code; native system-dep packaging (nixpkgs
raylib path now, jetpack later — non-Nix users get a clear message).

**Slice-specific proofs:** golden = `--replay moves.txt` with a fixed seed
under the headless/null renderer produces committed frame/event transcript;
perf = per-frame budget in replay mode; deploy = native binary on Linux/Nix
with the documented raylib sourcing story; ui fixture = using a `Texture`
after its window closed (RAII misuse voice); pure logic (collision, spawn)
under `#Test` + property tests.

---

## Sequencing

Gate order, not preference order. Unblocked slices proceed in parallel where
they don't contend.

1. **CLI** — no platform gates; starts at D-FLAGSHIP1 ratification. Ships
   first and sets the proof-matrix template + `tests/slices.rs` harness.
2. **Freestanding `metal`** — no owner gate; starts immediately, parallel
   with CLI. Feeds concrete API needs into the open D-LL3 ballot.
3. **Server** — starts at D-FLAGSHIP2 ratification; core slice needs only
   shipped E2-M10/M9 surface. Scale numbers re-run when #126 native parkers
   land; the slice does not wait for them.
4. **Web** — starts at D-FLAGSHIP3 ratification **and** #134 phases 2–4
   (view tree, typed styles, components — all gates ratified, build
   in-flight). The slice is #134's acceptance app.
5. **Game** — starts at D-FLAGSHIP4 ratification **and** the `core.raylib`
   package build (`../../sidequests/raylib-graphics.md`, ready). The slice is
   that package's acceptance app.

Exit: all five rows of the proof matrix green in CI for all five slices →
c123 closes → the e3 exit criterion is met.

## Gates

| Slice | Owner gate | Platform gate |
|---|---|---|
| CLI | D-FLAGSHIP1 | none |
| Server | D-FLAGSHIP2 | none for core; #126 for scale re-run |
| Freestanding | none | none (feeds D-LL3) |
| Web | D-FLAGSHIP3 | #134 phases 2–4 |
| Game | D-FLAGSHIP4 | `core.raylib` package (D-RAYLIB1=A, ready) |
