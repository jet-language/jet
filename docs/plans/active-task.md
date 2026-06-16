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
- `docs/plans/jetpack-jetos/README.md` — sequencing + D-JPK gates (§3.3 surface superseded by unified-ecosystem.md)
- `docs/plans/jetpack-jetos/jetos-design.md` — jetos tier (D-OS* superseded by U3/U4)
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

### A. Blocked on owner ratification (syntax gate — STOP, do not code yet)

These appear only as examples in `unified-ecosystem.md` / owner ballots, not in
the ratified U-series. Per I7: add an Open Decisions row, get the owner's
ratification, *then* implement test-first.

1. **gap #5 — `System` / `Image` semantics.** Both types parse and validate
   syntactically but are **inert**: no field checking, no realize path. Only
   `env` / `Env` means anything today. Needs the field-level surface ratified
   (e.g. `services:` / `options:` / `set(…)` / `from:`) before any code.
2. **gap #4 — `config.jet` + the entire `jetos` tier (Scale-3).** `CONFIG_FILE`
   is a `src/syntax.rs` constant only — never loaded; there is no `jetos` binary
   and no `src/jetos/`. Needs the `config.jet` surface ratified, then a loader +
   `jetos switch/build`. Sits on top of gap #5.
3. **D-CFFI2-SYN — C FFI surface syntax.** Owner direction **K**
   (`@extern module c.<lib> { … }` + `use "<header>" / use c.<lib> [as alias]`)
   is recorded as **draft, NOT ratified** in `docs/spec/decision-ballots*.md`
   (link resolution is already ratified). When the owner ratifies: amend S59/S16
   in `syntax-decisions.md`, move the decision out of the ballot, unblock E2-M14.
4. **Consumer-side "import a lib vs install an exec" syntax.** A `library`
   package realizes (staged source) but **nothing consumes that source yet** —
   there is no import-a-Jet-library path at the call site. Needs a syntax
   decision for how a project pulls a `library` into code vs putting an
   `executable` on PATH.

### B. Buildable without new syntax

5. **gap #6 remainder — richer interactive dev-shell story.** The two named
   Scale-2 commands (`jet dev` / `jetpack enter`) ship; what's left is a fuller
   interactive shell experience, which rides the existing `shell::enter` path.
   Low priority.
6. **The real Jet→binary compiler.** Both package kinds stage source / prebuilt
   bytes today; there is no compile step. Large; needs its own design pass
   (no ratified syntax dependency, but a major architecture arc).

Entry points for the jetpack side: `src/jetpack/{packmanifest,provider,modeval,
envfile,cli}.rs`, `src/syntax.rs`, `docs/spec/diagnostics.md`,
`examples/jetpack-typed/`, `tests/jetpack.rs`.

---

## Working-tree state (2026-06-16)

`master` head is `188c0f3` (gap #6) + `59b81ee` (this handoff doc). The D-S16-USE
`import → use` rename is **complete and green in the working tree** (full
`nix develop -c cargo test` passes, `tests/decisions.rs` passes) **but not yet
committed** as of this note.

**⚠ Stage selectively — never `git add -A`/`git add .`.** Bundled in the same
uncommitted working tree as the rename are **owner C-FFI ballot drafts**
(`docs/spec/decision-ballots.md`, `docs/spec/decision-ballots-owner.md`,
`docs/plans/epoch-2/m14-c-ffi.md`, `docs/plans/epoch-3/c-header-bindings.md`) —
these record the **draft, unratified** D-CFFI2-SYN direction and should **not**
be committed with the rename. Separate the rename (src + examples + tests +
`tests/ui` snapshots + the `use`-related doc/editor updates) from the C-FFI
draft material when committing.
