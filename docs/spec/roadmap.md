# Roadmap

**Current epoch: Epoch 3.** (owner, 2026-06-19) Epoch 1 (v1.0) and Epoch 2 (GA)
are complete — their development highlights are below under "Completed"; nothing
in the Epoch 2 GA scope remained open. Active work is now Epoch 3
([`docs/plans/epoch-3/`](../../docs/plans/epoch-3/)); its remaining-from-E2 loose ends are
tracked as cards in the Tower dashboard board.

Each milestone is done when its exit criteria pass as tests. Examples are the
executable spec: a milestone ships with new `examples/` programs and new
`tests/ui` fixtures, all green.

> **Competitive lens (owner, 2026-06-26, D-TSSWIFT1=B):** the "replace
> TypeScript / Swift" gap analysis (typed client/server protocols, reactive UI,
> interop, web/app backends) stays folded into these milestone descriptions —
> no separate gap doc. When prioritizing Epoch 3 work, weigh each feature
> against what a credible TS/Swift replacement needs.

> **Naming canon (owner, 2026-06-15):** **jet** is the language + compiler;
> **jetpack** is the package-manager engine/binary; **jetos** is the operating
> system (working title), built on jetpack. Single-file `jet run` stays
> ceremony-free forever (R9).

**Where detail lives (single source of truth):**

| Topic | Authoritative doc |
|---|---|
| Ratified syntax & owner decisions | [`syntax-decisions.md`](syntax-decisions.md) |
| Language behavior today | [`spec.md`](spec.md) |
| Open owner decisions | Tower (`node Tower/tower.mjs decision list --open`) |
| Epoch 1 highlights (done) | See "Epoch 1 — development highlights" below |
| Epoch 2 highlights (done) | See "Epoch 2 — development highlights" below |
| Epoch 5 metaprogramming plan | [`docs/plans/epoch-5/`](../../docs/plans/epoch-5/) → [`metaprogramming.md`](../../docs/plans/epoch-5/metaprogramming.md) |
| Jetpack sequencing + live status | [`docs/plans/epoch-4/`](../../docs/plans/epoch-4/) |
| jetos + visual configuration | Epoch 7; first runtime slice ratified in `syntax-decisions.md` (`jet os`, host selection, generations, init/image proof) |
| Implementing-agent protocol | [`docs/plans/README.md`](../../docs/plans/README.md) |

Plans are gated on ratified decisions in `syntax-decisions.md`; Tower owns the
live open-decision queue.

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
`jet explain` for every code, `jet self doctor` (offline + `--fix` + C-FFI section),
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
gate, `*T` (→ `*mut`), `p.*` dereference, and raw address interop;
diagnostics E3101/E3102/E3103 + lint L3101; the I1 amendment (D-LL1) recorded in
`architecture.md` (generated `unsafe` only inside user gates; safe Jet emits
none, enforced by `tests/golden.rs`). Deferred (open ballots): arenas (D-REF2),
wider `core.mem` API (D-LL3, name TBD).

**Post-v1 language features already shipped on `master`:** fan-out `f.[…]` (S75)
and fixed-size lists `[T#N]` (S76) — ratified and implemented 2026-06-16; see
`spec.md` and `syntax-decisions.md`.

---

**E2-M17 — Epoch 2 GA** partial 2026-06-17. All 6 D-GA1=B
showcases were retired from `examples/`; milestone coverage lives in
`examples/features/` and `examples/canon.jet` (E2 GA verified 2026-06-18; jetgrep,
lowlevel, freestanding, http_service) and pass the front end. Hard size budgets
(D-GA2=B) enforced in `tests/release_gates.rs`. Every E2 diagnostic has `jet explain`
(enforced). Single-file `jet run` needs no manifest.

Moved to Epoch 3 (owner, 2026-06-18): DAP step-through / full source-level
debugger — out of compiler scope for the E2 GA bar. `tests/observe.rs` keeps the
source-map markers and rich panics as the pre-cursor.

**Debugger step 1 SHIPPED (2026-06-25, c52, D-DBG3):** `jet debug <file>` is a
source-level step debugger over the existing tree-walking interpreter (the same
engine as `jet dev`/`jet repl`) — `(jet)` prompt, lldb-familiar
`step`/`next`/`continue`/`finish`, `break`/`print`/`locals`/`backtrace`, `<- here`
caret, all in Jet terms (I2). It declines unsteppable native features with E2203,
pointing at the real build. Step 2's native DAP/lldb backend shipped; remaining
full-feature stepping and editor work is tracked by Tower #12.

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
- **M10** — Core library: `core.files`, `core.io`, `core.env`, `core.process`, `core.math`, `core.random`, `core.time`, `core.encoding.json`. Frozen API in `docs/reference/core-library.md`.
- **M11** — tasks and channels (Epoch-2 concurrency work; shipped as part of the v1 arc).
- **M12** — package manager: `pkg.jet`, `.jet/lock`, content-addressed store (D-PM1…8). M12.1 verified; M12.2 (registry/semver) is Epoch 1 tail.
- **M13** — LSP: incremental front end, go-to-definition, diagnostics, hover.
- **M14** — v1.0 GA: showcase programs, diagnostics polish, binary size budgets.

---

## Epoch 2 — development highlights

18 milestones, production-platform arc, GA verified 2026-06-18.

- **E2-M1** — tasks and channels without data races; ownership proves sendability.
- **E2-M2** — release policy, editions/epochs, `edition:` in `pkg.jet`, deprecation policy.
- **E2-M3** — developer CLI polish: TTY color, `jet explain`, `jet self doctor`, fix engine, man pages, completions.
- **E2-M4** — `jet dev`: watch server, interpreter-backed dev loop, <200ms latency budget.
- **E2-M5** — tier-2 references: `view`/`ref` hardening, zero-copy patterns.
- **E2-M6** — library authoring: associated types, error conversion for `?`, argument labels/defaults (S61), trait delegation (S62).
- **E2-M7** — streaming I/O: file handles, `Reader`/`Writer`, RAII cleanup (S63), `Path`.
- **E2-M8** — supply chain: `jet registry publish` (pre-publish gate), `jet registry vendor`, `jet inspect audit`, SBOM; PubGrub resolver. Registry upload deferred (D-PKGS1, M12.2); Jetpack hangar cleanup is `jet clean`.
- **E2-M9** — first-party library ring: `core.regex`, `core.encoding.{csv,toml,yaml,json}`, `core.log`, `core.time`, `core.crypto`, `core.archive` (zip/tar containers) plus `core.compress` (gzip/zstd streams, D-CORE-COMPRESS1), `core.db` (SQLite via rusqlite bundled — D-DEP-DB1).
- **E2-M10** — networking: blocking TCP/UDP, HTTP client/server (`core.http`; client HTTPS became default later under D-TLS1; server HTTPS uses D-TLSSERVE1's named `tls:` option). Advanced client TLS configuration remains `core.tls`.
- **E2-M11** — testing/docs/bench: doctests, coverage, `jet bench`, property testing.
- **E2-M12** — debug/observe: DAP prep, panic locals, structured logging/tracing/metrics.
- **E2-M13** — expert low-level tier: `use core.mem`, `#Unsafe("reason")` gates, `*T`, volatile; I1 amendment (D-LL1).
- **E2-M14** — C FFI: `#Bindgen`/`#Extern module`, `use c.<lib>`, link discovery.
- **E2-M15** — cross-compilation + freestanding: `jet build --target`, `--freestanding`, QEMU smoke.
- **E2-M16** — pure evaluation + layer 3: `@Pure fn`, `jet eval --pure`, package recipes, sandboxed builds.
- **E2-M17** — Epoch 2 GA: six showcase programs, diagnostics audit, size/perf budgets.
- **E2-M18** — REPL: `jet repl`, interpreter-backed, 16 transcript tests.

---

## Active / not yet verified — Epoch 3 and promoted tracks

### Jetpack and jetos

**Jetpack** is Epoch 4. It owns the package-manager and environment substrate:
providers, strict package graphs, catalogs, explainable locks, migration
importers, hangar/cache, signing, build-from-source, and no-Nix behavior.
The 2026-07-09 production audit found that several earlier “done” cards delivered
useful schemas or fixture-backed models without the live store, sandbox, cache,
registry, or provider behavior required for a package-manager completion claim.
The binding parity/acceptance plan is
[`world-class-package-manager.md`](../../docs/plans/epoch-4/world-class-package-manager.md):
full pinned Nix package-manager compatibility plus the best compatible features
from other ecosystems, closed only by live, hostile, cross-platform evidence.

**jetos** is Epoch 7. It builds on jetpack and owns declarative OS activation,
proof-before-switch, generations, installable images, source-backed Studio, and
the Blueprint-class visual editor. The first runtime slice is active: `jet os`
checks/builds/switches hosts from `system.<host>` declarations, records named
generations, rolls back, scaffolds configs, and emits activation proof plus
hybrid-ISO installer media/proof artifacts. VM proof now distinguishes
harness-ready from guest-passed, runs the QEMU create/install/reboot phases,
boots the hybrid ISO installer through Limine, installs a GPT disk with a FAT
ESP and ext4 root, reboots the installed disk through OVMF/Limine, uses
`rdinit=/jetos/init` to enter the JetOS installer/verify overlay dispatcher,
captures the installed guest's serial proof marker, and only accepts a
guest proof bound to the same host, generation, disk, media proof, tool hashes,
and guest assertions, including terminal-login readiness through serial/virtual
getty units, `/etc/profile`, `/etc/shells`, and projected user homes plus
desktop-session readiness through GNOME Wayland session artifacts, display-manager
unit wiring, terminal fallback, the installed jetos Studio app, and
graphical-console readiness through QEMU VNC/stdvga plus guest-visible `fb0`;
the graphical verifier also executes the generated display-manager,
desktop-session, and terminal-fallback launchers in proof mode so the installed
closure proves its GNOME/GDM launch path, not just file presence.
The
interactive `jet os vm run <host> --disk <path>` path launches only a disk
already tied to the latest generation by the same passing VM proof, with a
graphical VNC console exposed and serial output attached to the current process.
`module vmtest.<name>` now declares a VM scenario over `system.<host>` refs, and
`jet os vm test <name> --disk <path>` runs the same install/reboot proof harness
per declared host, recording typed assertion facts and replayable VM-test proof
artifacts under `systems/vm-tests/`.
The
`cachyos-kernel` package can now build missing boot
artifacts from its recorded `source/recipe.jet` via package-internal
`source/build.sh`; that builder is authoritative when present, so stale
pre-dropped boot files do not bypass the source-built path. The installer and VM
runner boot the artifacts produced by the selected first-party package, and the
installer ISO dereferences generation symlinks into a self-contained guest
payload before real-QEMU install/reboot proof runs. Its UEFI path uses a real
FAT ESP boot image (`boot/efiboot.img`) containing `EFI/BOOT/BOOTX64.EFI`, not a
raw EFI binary as the El Torito image. Each generation defaults to the ratified
GNOME-on-Wayland desktop profile, keeps terminal login as fallback, and installs
the first-party jetos Studio app projection into the system profile with a
browser fallback over the same local protocol; `jetos studio --headless` exposes
the installed app path for review flows. The dev-shell smoke path can inject the local CachyOS
kernel/initrd/modules into the first-party `cachyos-kernel` builder, producing a
real-QEMU VM proof while production package recipes continue to harden. Fleet
rollout stays future Epoch 7 work.

### Epoch 1 tail

**M12.2** — registry, semver resolver, `jet registry publish` / `vendor` / `audit`
(architecture: [`epoch-4/README.md`](../../docs/plans/epoch-4/README.md)). M12.1 verified
2026-06-13. `jet registry publish` runs the pre-publish gate, but registry upload is not
implemented; use git-based dependencies. Jetpack hangar cleanup uses `jet clean`.

---

## Deferred unless owner promotes

Items with Epoch 2/3 plans are tracked in those plan directories — not
duplicated here:

- Async/await, Go-scale networking → [`docs/plans/epoch-3/`](../../docs/plans/epoch-3/)
- DAP step-through / full source-level debugger → Epoch 3 (owner, 2026-06-18)
- Full adoption documentation (migration, services, debugging guides) → Epoch 3
  (owner, 2026-06-18); per-milestone docs stay as written
- User token macros (rejected by S26; sanctioned path is S56 comptime derives)
- Self-hosting → **Epoch 9** (Bootstrapping). Hard readiness gate before any
  port work (owner, 2026-07-06): core lang locked-happy (stdlib may flux), a
  dogfood portfolio of complex/fringe Jet projects proving readability and
  reason-about-ability, and the memory model adversarially proven as solid as
  Rust's borrow checker. Tracked in Tower cards #217 (readiness gate) and #218
  (full port).
- Comptime layer 3 / user-defined derives (S56) → Epoch 3
- Jai-style AST mutation/message-loop/user macros → rejected by D-METAMUTATE1=A
  and closed in Tower #15. Practical power stays in the non-mutating stack:
  generated source modules, typed build actions, read-only front-end APIs,
  stdlib DSL blocks, and policy passes.
- Formal core / desugaring map → **Epoch 6** (D-FORMALCORE1=C); placeholder at
  [`docs/spec/formal-core.md`](formal-core.md); enforcement deferred until sema is frozen
- Time-travel variable history (reversible execution / runtime value timeline)
  → **Epoch 6** (D-TIMETRAVEL1=C, c111); prerequisites: D-REPLAY1 runtime replay
  harness shipped, `jet debug` (D-DBG3) mature. No compiler work in Epoch 3.
  Manual workaround today: use `jet debug <file>` (source-level step debugger,
  D-DBG3) with breakpoints/watch, or add temporary `Log.debug(x)` calls at the
  points where history matters — the interpreter re-run is cheap enough for
  most debugging sessions. No standing per-variable history buffer exists.

When a deferred item is promoted, add a milestone slot in the appropriate epoch
README and ratify any new syntax in `syntax-decisions.md` before implementation.
