# Roadmap

**Current epoch: Epoch 3.** (owner, 2026-06-19) Epoch 1 (v1.0) and Epoch 2 (GA)
are complete — their development highlights are below under "Completed"; nothing
in the Epoch 2 GA scope remained open. Active work is now Epoch 3
([`tools/Tower/docs/plans/epoch-3/`](../../tools/Tower/docs/plans/epoch-3/)); its remaining-from-E2 loose ends are
tracked as cards in the Tower dashboard board.

Each milestone is done when its exit criteria pass as tests. Examples are the
executable spec: a milestone ships with new `examples/` programs and new
`tests/ui` fixtures, all green.

> **Naming canon (owner, 2026-06-15):** **jet** is the language + compiler;
> **jetpack** is the package-manager engine/binary; **jetos** is the operating
> system (working title), built on jetpack. Single-file `jet run` stays
> ceremony-free forever (R9).

**Where detail lives (single source of truth):**

| Topic | Authoritative doc |
|---|---|
| Ratified syntax & owner decisions | [`syntax-decisions.md`](syntax-decisions.md) |
| Language behavior today | [`spec.md`](spec.md) |
| Open owner ballots | [`decision-ballots.md`](decision-ballots.md) |
| Epoch 1 highlights (done) | See "Epoch 1 — development highlights" below |
| Epoch 2 highlights (done) | See "Epoch 2 — development highlights" below |
| Jetpack & jetos sequencing + live status | [`tools/Tower/docs/plans/jetpack-jetos/`](../../tools/Tower/docs/plans/jetpack-jetos/) |
| Implementing-agent protocol | [`tools/Tower/docs/plans/README.md`](../../tools/Tower/docs/plans/README.md) |

Plans are gated on ratified decisions in `syntax-decisions.md` (see
`decision-ballots.md` for what is still open).

---

## Completed

**Epoch 1 — v1.0** verified 2026-06-14 (M0–M14).

**E2-M1 — Concurrency** verified 2026-06-14.

**E2-M2 — Release policy, editions, epoch contract** verified 2026-06-16. Ratified
compatibility/release policy ([`release-policy.md`](release-policy.md));
`edition:` marker in `pkg.jet`; enriched `jet --version` banner; E2001
reachable, E2002/L2001 registered (honestly empty pre-1.0 deprecation registry).

**E2-M3 — Developer command UX** verified 2026-06-16. Stable exit-code table,
TTY-aware color (NO_COLOR/FORCE_COLOR/--color), versioned `--json` schema,
`jet explain` for every code, `jet doctor` (offline + `--fix` + C-FFI section),
no-args greeting + did-you-mean (E2101/E2102/L2101), completions + man page
from one registry, unified CLI/LSP fix engine, external `jet-<name>` discovery,
OSC 8 hyperlinks, `jet build -v`. Digit separators (S67) already shipped.

**E2-M4 — `jet dev` (watch + interpreter loop)** verified 2026-06-17. `jet dev <file>` (D-DEV4)
watches and re-runs on save via the M9.5 comptime evaluator extended to whole
programs; a 15-program differential battery proves interpreted stdout ==
compiled stdout byte-for-byte (I2); honest boundaries E2201/E2202 name
`jet build` for FFI/tasks/`#Unsafe`/native-std (`--try-anyway` to attempt
anyway, D-DEV1); <200ms latency budget tested (D-DEV3). No release path uses the
interpreter (I2/I3); JIT deferred to Epoch 3 (D-DEV2). Std-only file watching
(I6).

**E2-M13 — Expert low-level tier (S58)** verified 2026-06-17. `use
core.mem` discovery gate, `#Unsafe("reason") { … }` / `#Unsafe fn` audit
gate, `Ptr<T>` (→ `*mut`), `mem.volatile_read`/`address_of`/`from_addr`;
diagnostics E3101/E3102/E3103 + lint L3101; the I1 amendment (D-LL1) recorded in
`architecture.md` (generated `unsafe` only inside user gates; safe Jet emits
none, enforced by `tests/golden.rs`). Deferred (open ballots): arenas (D-REF2),
wider `std.mem` API (D-LL3, name TBD).

**Post-v1 language features already shipped on `master`:** fan-out `f.[…]` (S75)
and fixed-size lists `[T#N]` (S76) — ratified and implemented 2026-06-16; see
`spec.md` and `syntax-decisions.md`.

---

**E2-M17 — Epoch 2 GA** partial 2026-06-17. All 6 D-GA1=B
showcases exist in `examples/showcase/` (jetgrep, jsonfmt, wordfreq, library,
lowlevel, freestanding, http_service) and pass the front end. Hard size budgets
(D-GA2=B) enforced in `tests/ga.rs`. Every E2 diagnostic has `jet explain`
(enforced). Single-file `jet run` needs no manifest.

Moved to Epoch 3 (owner, 2026-06-18): DAP step-through / full source-level
debugger — out of compiler scope for the E2 GA bar. `tests/observe.rs` keeps the
source-map markers and rich panics as the pre-cursor.

**Debugger step 1 SHIPPED (2026-06-25, c52, D-DBG3):** `jet debug <file>` is a
source-level step debugger over the existing tree-walking interpreter (the same
engine as `jet dev`/`jet repl`) — `(jet)` prompt, lldb-familiar
`step`/`next`/`continue`/`finish`, `break`/`print`/`locals`/`backtrace`, `<- here`
caret, all in Jet terms (I2). It declines unsteppable native features with E2203,
pointing at the real build. Step 2 — the native DAP/lldb backend for the full
native feature set + editor wiring — remains (see `tools/Tower/docs/sidequests/dap-debugger.md`).

**E2-M18 — REPL** verified 2026-06-17. `jet repl` interactive
session; 16 transcript tests green.

---

## Epoch 1 — development highlights

M0–M14, v1.0 arc, verified 2026-06-14.

- **M0** — bootstrap: lexer, parser, Rust codegen, `jet run`, hello-world golden test.
- **M1–M2** — functions, variables, control flow, basic types.
- **M3** — structs and enums (data types).
- **M4** — error handling: `T ? E`, `?` propagation, `??`, `panic`.
- **M5** — collections: lists, maps, strings.
- **M6** — tooling: `jet fmt`, `jet test`, multi-file imports.
- **M7** — FFI: `extern rust` inline crate deps.
- **M8** — closures and lambdas.
- **M9** — generics and traits; **M9.5** — comptime evaluation and `@embed`.
- **M10** — Core library: `core.fs`, `core.io`, `core.env`, `core.process`, `core.math`, `core.random`, `core.time`, `core.json`. Frozen API in `docs/reference/core-library.md`.
- **M11** — tasks and channels (Epoch-2 concurrency work; shipped as part of the v1 arc).
- **M12** — package manager: `pkg.jet`, `.jet/lock`, content-addressed store (D-PM1…8). M12.1 verified; M12.2 (registry/semver) is Epoch 1 tail.
- **M13** — LSP: incremental front end, go-to-definition, diagnostics, hover.
- **M14** — v1.0 GA: showcase programs, diagnostics polish, binary size budgets.

---

## Epoch 2 — development highlights

18 milestones, production-platform arc, GA verified 2026-06-18.

- **E2-M1** — tasks and channels without data races; ownership proves sendability.
- **E2-M2** — release policy, editions/epochs, `edition:` in `pkg.jet`, deprecation policy.
- **E2-M3** — developer CLI polish: TTY color, `jet explain`, `jet doctor`, fix engine, man pages, completions.
- **E2-M4** — `jet dev`: watch server, interpreter-backed dev loop, <200ms latency budget.
- **E2-M5** — tier-2 references: `view`/`ref` hardening, zero-copy patterns.
- **E2-M6** — library authoring: associated types, error conversion for `?`, argument labels/defaults (S61), trait delegation (S62).
- **E2-M7** — streaming I/O: file handles, `Reader`/`Writer`, RAII cleanup (S63), `Path`.
- **E2-M8** — supply chain: `jet publish` (pre-publish gate), `jet vendor`, `jet audit`, SBOM; PubGrub resolver. Registry upload and `jet gc` deferred (D-PKGS1, M12.2).
- **E2-M9** — first-party library ring: `jet.regex`, `jet.csv`, `jet.toml`, `jet.yaml`, `jet.json`, `jet.log`, `jet.time`, `jet.crypto`. (`jet.archive`, `jet.db` are reserved names but staged — not yet available.)
- **E2-M10** — networking: blocking TCP/UDP, HTTP client/server (`jet.http`, plain HTTP). TLS is delivered as the `jet.tls` package (separate from the core binary, I6).
- **E2-M11** — testing/docs/bench: doctests, coverage, `jet bench`, property testing.
- **E2-M12** — debug/observe: DAP prep, panic locals, structured logging/tracing/metrics.
- **E2-M13** — expert low-level tier: `use core.mem`, `#Unsafe("reason")` gates, `Ptr<T>`, volatile; I1 amendment (D-LL1).
- **E2-M14** — C FFI: `@bindgen`/`@extern module`, `use c.<lib>`, link discovery.
- **E2-M15** — cross-compilation + freestanding: `jet build --target`, `--freestanding`, QEMU smoke.
- **E2-M16** — pure evaluation + layer 3: `#Pure fn`, `jet eval --pure`, package recipes, sandboxed builds.
- **E2-M17** — Epoch 2 GA: six showcase programs, diagnostics audit, size/perf budgets.
- **E2-M18** — REPL: `jet repl`, interpreter-backed, 16 transcript tests.

---

## Active / not yet verified

### Epoch 2 — production platform

**Epoch 2 GA is complete** (owner, 2026-06-18): all 18 milestones landed on
`master`, and the last in-scope language gaps closed this session — the Jet
**module system** (D-MOD1–4) and a functional **`jet bind`** (native std-only
backend). Moved to Epoch 3: DAP step-through debugging, adoption documentation,
**package build-from-source + M9 wave-2**, and **M11 property testing / doctests
/ coverage** (syntax-gated ergonomics).

**Deferred registry ops (D-PKGS1):** `jet publish` runs the pre-publish gate
(build + tests + API diff) but registry upload is not implemented — use
git-based dependencies. `jet gc` is a stub pending M12.2. TLS requires the
`jet.tls` package; the built-in HTTP client (`jet.http`) is plain HTTP only.

### Jetpack & jetos

**jetos is deferred to post-Epoch-3** (owner, 2026-06-18) — research track only;
do not ratify its config/surface syntax during Epoch 2 or 3. **Jetpack** Phase 1
environments and the typed `module { … }` surface stay active: see
[`jetpack-jetos/README.md`](../../tools/Tower/docs/plans/jetpack-jetos/README.md). **Live
built-vs-pending status:**
[`jetpack-jetos/IMPLEMENTATION-STATUS.md`](../../tools/Tower/docs/plans/jetpack-jetos/IMPLEMENTATION-STATUS.md).

### Epoch 1 tail

**M12.2** — registry, semver resolver, `jet publish` / `vendor` / `audit`
(architecture: [`unified-ecosystem.md`](../../tools/Tower/docs/plans/jetpack-jetos/unified-ecosystem.md) §10). M12.1 verified
2026-06-13.

---

## Deferred unless owner promotes

Items with Epoch 2/3 plans are tracked in those plan directories — not
duplicated here:

- Async/await, Go-scale networking → [`tools/Tower/docs/plans/epoch-3/`](../../tools/Tower/docs/plans/epoch-3/)
- DAP step-through / full source-level debugger → Epoch 3 (owner, 2026-06-18)
- Full adoption documentation (migration, services, debugging guides) → Epoch 3
  (owner, 2026-06-18); per-milestone docs stay as written
- User token macros (rejected by S26; sanctioned path is S56 comptime derives)
- Self-hosting; jetos as a shipped OS product → **post-Epoch-3** research track
  (owner, 2026-06-18); jetos surface syntax is not ratified in Epoch 2/3
- Comptime layer 3 / user-defined derives (S56) → Epoch 3

When a deferred item is promoted, add a milestone slot in the appropriate epoch
README and ratify any new syntax in `syntax-decisions.md` before implementation.
