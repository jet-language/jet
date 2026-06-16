# Active task — staging & moving-forward guidance

**Updated:** 2026-06-16. This is the live handoff/staging doc. A fresh agent
reads this first, then the docs it points at. Keep it current: when a chunk
lands, move it into **Done (shipping)** and trim **Remaining work**. Per-chunk
narrative history lives in git — do not re-accrete it here.

---

## North star (the goal we are implementing toward)

`docs/plans/jetpack-jetos/unified-ecosystem.md` — the **owner-ratified
design-of-record** for the `jet` + `jetpack` + `jetos` ecosystem. It is the
TARGET surface; Phase-1 directive scanning was the shippable bootstrap that has
since grown into the typed `module { … }` surface. Ratified (U1–U10) ≠
implemented — check **Remaining work** below before assuming a feature exists.

Supporting design docs (own their detail):
- `docs/plans/jetpack-jetos/README.md` — sequencing, milestones, provider roadmap (R3/tvix pending), jetos parity baseline
- `docs/plans/jetpack-jetos/jetos-design.md` — jetos tier (D-OS1/7 superseded by U3/U4; D-OS2-6 still open)
- `docs/plans/IMPLEMENTATION.md` — the implementing-agent kickoff prompt + chunk protocol
- `docs/plans/README.md` — repo protocol: **one chunk per run, test-first, stop and report**

---

## Operating constraints (non-negotiable)

- Run everything through the Nix dev shell: `nix develop -c cargo test`, etc.
  (CLAUDE.md). One `nix develop` at a time — it serializes.
- Invariants I1–I8. Especially: no `unsafe` (I1); rustc never speaks to users
  (I2); codegen is dumb, all checks in sema/comptime (I3); every diagnostic has
  a code + what/why/fix + a `tests/ui` snapshot (I4); examples are the
  executable spec (I5); zero new compiler crates without owner approval (I6).
- **Syntax gate (I7):** only implement syntax that is **Ratified** in
  `docs/spec/syntax-decisions.md`. If a chunk needs an OPEN decision, STOP and
  follow the syntax-decision protocol (add an Open Decisions row, build
  something else). Owner has final say on all user-facing syntax. Measure twice,
  cut once — full design pass before code, no "fix it later" seams.
- Pipeline order when adding syntax: `src/syntax.rs → lexer → parser → sema →
  codegen`. Never skip sema into codegen.

---

## Done (shipping) — the Phase-1 bootstrap + typed surface

The typed `module { … }` `env.jet` surface builds and realizes end-to-end. What
is built and green today:

- **Two binaries:** `jet`, `jetpack` (Cargo.toml `[[bin]]`).
- **`payload.jet` manifest** (U10): `payload:` / `deps:` /
  `packages: { name: library|executable }` / `edition` parsed in
  `src/jetpack/packmanifest.rs`. Packages are top-level `module <name>`s,
  **discovered** by walking the tree; the `core` provider realizes by kind
  (`executable` → prebuilt `bin/` on PATH; `library` → staged source, no PATH
  entry). `env.jet` is never read by the provider — it is the dev-shell only.
  Diagnostics E1210–E1213.
- **Typed env evaluation** (`src/jetpack/modeval.rs`): `module dev { sources: {…}
  imports: find(…) env.dev: Env { packages: [...] } }` → `EnvPlan`; routed by
  `is_module_surface` (Phase-1 `pkg.*` directive scanner is the fallback).
  `find("./modules")` walks one level deep and folds discovered modules into the
  same merge (E0969–E0971; liftability law). Source conflicts → E0967.
- **Sources** (U8/U9): `sources: { name: provider@target }` with `github@` /
  `path@` / `nixpkgs@`. Provider **kind is inferred, no marker**:
  `path@` from a local `payload.jet`; `github@` via a realize-time shallow git
  peek for `payload.jet` (`ProviderKind::Infer` resolved in `provider.rs`).
- **Providers** (`src/jetpack/provider.rs`): `core` (first-party Jet, no nix) +
  `nix`, realizing into the hangar store `/etc/jet/hangar`. Unified lockfile
  `.jet/lock`; `.jet/` managed folder (`store.rs`).
- **Commands:** `jetpack run/enter/build/list/clean/add/remove`; `jet dev` →
  `jetpack enter` (Scale-2 front door). `enter` is project-scoped (always loads
  the project `env.jet`).
- **`Pkg` sugar:** `default.ripgrep` / `default.[ripgrep, fd]` / `unstable.neovim`.
- **`use` keyword** (D-S16-USE, ratified 2026-06-16, amends S16): file/module
  imports are `use "<path>"` / `use name [as alias]`; `import` is now a teaching
  error (E0015). Renamed across compiler, examples, tests, snapshots, docs.
- **Ratified & enforced:** U1–U10 + S16/D-S16-USE in `docs/spec/syntax-decisions.md`,
  guarded by `tests/decisions.rs`.

**Landed in the `jetos-ratified-arc` pass (2026-06-16):**
- **gap #5 — `System`/`Image`/`Service` are live** (U11–U14, U13, U18). They
  parse → elaborate (bare `{…}` inferred constructors) → field-check → are
  captured into `EnvPlan.systems`/`.images` (`SystemPlan`/`ServicePlan`/
  `OptionPlan`/`ImagePlan` in `modeval.rs`). Diagnostics E0972–E0978.
- **gap #4 — `jetpack os build/switch [<config>]@<host>`** (U15/U16). `config.jet`
  loader reuses `modeval`; `@host` selects the `System`; default path
  `~/.jet/config.jet`. Realizes packages into the hangar, assembles a
  content-addressed generation dir + `manifest.json`, flips `current`/boot
  `default` symlinks. `src/jetpack/jetos.rs`. Diagnostics E0979–E0981.
- **U17 — `library` packages consumed with `use`.** A realized library is an
  extra search root in the `use <pkg>` resolver (`src/loader.rs`); executables
  stay on PATH. Diagnostics E0982 (exe-not-importable), E0983 (unrealized lib).
- **B1–B4 ICE bugs fixed** (I2/I3): JSON view-param move (clone + L0201), std-
  struct field-name mangling, `Map.get` lowering via `Object(root)` pattern, and
  the `for … in recv.field { }` struct-literal misparse. Locked by
  `tests/ice_regressions.rs`.
- **E2-M14 C FFI (Phases 1–2 + compile-time hook).** `@extern`/`@bindgen module
  c.<lib>`, bindgen∪overlay merge (overlay wins), `use c.<lib>` / `use "hdr.h"`,
  hangar/pkg-config link discovery, `jet bind` CLI. `src/cffi.rs`. Diagnostics
  E3201–E3208. **WAIVED/STUBBED** (honest): real bind backend (no bindgen crate
  added per I6) → `jet bind`/auto-bind report E3208 with a hand-overlay
  workaround; Phase-3 cache hash-regen; E3202 registered but unreachable until
  **E2-M13** (pointer/`@unsafe` tier) lands; raylib pong replaced by a
  deterministic small-C-lib e2e in `tests/cffi.rs`.

Offline e2e coverage in `tests/jetpack.rs` (incl.
`committed_example_builds_offline_end_to_end`, the typed-module example, and the
`enter`/`dev` tests). `jet run examples/features/01_hello.jet` prints
`hello, world`.

---

## Remaining work to finish the implementation

Ordered roughly by leverage. Each is its own multi-chunk arc. **Confirm the
relevant decision is Ratified before coding, and write the failing test/example
first.**

### A. Ratified arc — DONE (see "Landed in the `jetos-ratified-arc` pass" above)

gap #5, gap #4, U17, and E2-M14 C FFI (Phases 1–2 + hook) all shipped on
`jetos-ratified-arc`. What remains under the C FFI banner is its **waived tail**,
gated on an external prerequisite, not on owner syntax:

1. **E2-M14 tail — real bind backend + E2-M13.** Wire the actual C-header→Jet
   translation (D-CBIND3 bindgen helper — needs an owner-approved crate added to
   `Cargo.toml`, currently NOT added per I6) so `jet bind`/auto-bind stop
   reporting E3208; add Phase-3 cache hash-invalidation/regen. Separately,
   **E2-M13** (pointer / `@unsafe` / `core.mem` tier, S58) must land before
   E3202 is reachable from real source. Both are their own arcs.

### B. Buildable without new syntax

5. **gap #6 remainder — richer interactive dev-shell story.** The two named
   Scale-2 commands (`jet dev` / `jetpack enter`) ship; what's left is a fuller
   interactive shell experience, which rides the existing `shell::enter` path.
   Low priority.
6. **The real Jet→binary compiler.** Both package kinds stage source / prebuilt
   bytes today; there is no compile step. Large; needs its own design pass
   (no ratified syntax dependency, but a major architecture arc).
7. **B1–B4 ICE bugs — DONE** (fixed on `jetos-ratified-arc`, locked by
   `tests/ice_regressions.rs`). What remains from this cluster:
   - **Cross-file language walls** (v1, may want lifting): a module file cannot
     (1) name another file's struct type in a signature, (2) call another file's
     methods on an imported value, or (3) `use`-import with `..`. These are why
     the legacy Phase-1 surface funneled everything through `[JSON]` directives.
     (U17 library import did **not** hit these — a realized library is found by
     the ordinary resolver — but they remain open for general cross-file code.)

Entry points for the jetpack side: `src/jetpack/{packmanifest,provider,modeval,
envfile,cli}.rs`, `src/syntax.rs`, `docs/spec/diagnostics.md`,
`examples/jetpack-typed/`, `tests/jetpack.rs`.

---

## Working-tree state (2026-06-16)

Branch **`jetos-ratified-arc`** (off `master` @ `3e6be24`) holds the ratified-arc
pass — five commits, full `nix develop -c cargo test` green (348 passed, 0 failed),
`tests/decisions.rs` green:

- `4e44146` gap #5 — System/Image/Service live
- `be82d01` gap #4 — `jetpack os build/switch …@host`
- `d885585` U17 — library consumed with `use`
- `c4fff23` B1–B4 ICE fixes
- `cec262a` E2-M14 C FFI (Phases 1–2 + hook)

Not yet merged to `master` (awaiting owner review/merge). New **open decision
S61** (hyphens in contribution/package/image names) added to `syntax-decisions.md`
— `image.halcyon-iso` does not lex today; gap #5 used `halcyon_iso`. Recommendation
is to allow hyphens in name positions; until ratified, names stay underscored.
