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

Offline e2e coverage in `tests/jetpack.rs` (incl.
`committed_example_builds_offline_end_to_end`, the typed-module example, and the
`enter`/`dev` tests). `jet run examples/features/01_hello.jet` prints
`hello, world`.

---

## Remaining work to finish the implementation

Ordered roughly by leverage. Each is its own multi-chunk arc. **Confirm the
relevant decision is Ratified before coding, and write the failing test/example
first.**

### A. Ratified, ready to implement (test-first; syntax now settled)

The jetpack/jetos surface decisions are **all ratified** as **U11–U18** in
`docs/spec/syntax-decisions.md` (owner picks in `decision-ballots-owner.md`,
2026-06-16). End-state worked `config.jet` is in `decision-ballots.md` (§ "gaps
#4/#5 — ratified surface"). Per the workflow loop: write the failing ui
fixture/example first, spec it, then parser → sema → codegen. Note **U18 (inferred
constructors)** and **U13 (typed `target` / bare-vs-quoted option values)** touch
the record-literal path broadly — land them deliberately.

1. **gap #5 — `System` / `Image` / `Service` semantics.** Today both types parse
   but are **inert** (no field checking, no realize). Implement per **U11**
   (`System` = `target`/`packages`/`services`/`options`), **U12** (`Service` open
   record), **U13** (`options:` ordered `key: value` list, no `set()`; typed
   `target` platform value; bare words vs quoted free-form strings), **U14**
   (`Image { from: system.X, format: }`, target/packages inherited from the
   system), and **U18** (bare `{…}` elaborates to the expected type). Do gap #5
   first — gap #4 sits on top.
2. **gap #4 — `config.jet` + the `jetos` tier (Scale-3).** `CONFIG_FILE` is a
   `src/syntax.rs` constant only — never loaded. Implement per **U15** (`jetpack os
   <verb>` subcommand group — switch/build — **not** a separate binary, **not**
   under `jet`) and **U16** (positional `[<config-path>]@<host>`, path defaults
   `~/.jet/config.jet`, `@host` selects the `System`). Then the loader + activate.
3. **E2-M14 — C FFI implementation.** Surface syntax ratified (**D-CFFI2-SYN-1…4**,
   **S59**); bind engine picks ratified (**D-CBIND2/3/5/6**); **`use`** keyword
   ratified (**D-S16-USE**); **`@audit("…")`** required (**D-LL2**). Agent spec:
   [`docs/plans/epoch-2/m14-c-ffi.md`](epoch-2/m14-c-ffi.md). No owner ballot
   blockers remain — implement parser/sema/codegen + `jet bind`.
4. **Consumer-side library import.** A `library` package realizes (staged source)
   but **nothing consumes it yet**. Implement per **U17**: a realized `library` is
   brought in with the ordinary `use <pkg>` form (reuse S16/D-S16-USE);
   `executable` packages still go on PATH.

### B. Buildable without new syntax

5. **gap #6 remainder — richer interactive dev-shell story.** The two named
   Scale-2 commands (`jet dev` / `jetpack enter`) ship; what's left is a fuller
   interactive shell experience, which rides the existing `shell::enter` path.
   Low priority.
6. **The real Jet→binary compiler.** Both package kinds stage source / prebuilt
   bytes today; there is no compile step. Large; needs its own design pass
   (no ratified syntax dependency, but a major architecture arc).
7. **Verify/fix the compiler bugs found during the jetpack/config bring-up**
   (carried over from the retired `jetpack-config-brief.md`; confirm each is
   still open before fixing — the typed `module {}` surface may already have
   worked around or fixed some). Each is an I2/I3 violation (rustc ICE instead of
   a sema diagnostic) with a one-line repro:
   - **B1** — `JSON.Text(x)` where `x` is a *view* param moves it → rustc ICE.
     Sema should insert a clone or reject; never ICE.
   - **B2** — field access on a **std** struct mangles the name
     (`result.code` → `user_code`) → rustc ICE. Tracked also in
     `docs/plans/capstone/PROGRESS.md` (`ProcessResult`).
   - **B3** — `.get(k)` on a `Map` bound via an `Object(root)` pattern lowers to
     **list indexing** (`"k".to_string() as usize`) → rustc ICE.
   - **B4** — `for k, v in recv.field { … }` parses `recv.field {` as a **struct
     literal**; ending the subject in `()` disambiguates.
   - **Cross-file language walls** (v1, may want lifting): a module file cannot
     (1) name another file's struct type in a signature, (2) call another file's
     methods on an imported value, or (3) `use`-import with `..`. These are why
     the legacy Phase-1 surface funneled everything through `[JSON]` directives.

Entry points for the jetpack side: `src/jetpack/{packmanifest,provider,modeval,
envfile,cli}.rs`, `src/syntax.rs`, `docs/spec/diagnostics.md`,
`examples/jetpack-typed/`, `tests/jetpack.rs`.

---

## Working-tree state (2026-06-16)

`master` head: the **D-S16-USE `import → use` rename is COMMITTED** as `380b6a5`
(72 files; full `nix develop -c cargo test` green, `tests/decisions.rs` green).
Handoff-doc commits: `59b81ee` + `f546444`.

**Still uncommitted (owner's in-flight doc sync — leave for the owner; ⚠ never `git add -A`):**
C-FFI / decision-ballot doc updates may remain on disk from this session; C FFI
surface + CBIND + LL2 + S16-USE are **ratified** in `syntax-decisions.md` and
[`m14-c-ffi.md`](epoch-2/m14-c-ffi.md).
