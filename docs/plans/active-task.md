# Active task — staging & moving-forward guidance

**Updated:** 2026-06-16. This file is the live handoff/staging doc. A fresh agent
should read this first, then the docs it points at. Keep it current: when a
chunk lands, update "Where we are" and "Next up", and move finished items into
"Done".

---

## North star (the goal we are implementing toward)

`docs/plans/jetpack-jetos/unified-ecosystem.md` — the **owner-ratified
design-of-record** for the `jet` + `jetpack` + `jetos` ecosystem. It is the
TARGET surface; it explicitly says Phase-1 directive scanning is the shippable
bootstrap that evolves into the typed surface. Do **not** treat ratified
(U1–U7) as implemented — see the gap list below.

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
- **Syntax gate:** only implement syntax that is Ratified in
  `docs/spec/syntax-decisions.md`. If a chunk needs an OPEN decision, STOP and
  follow the syntax-decision protocol (add an Open Decisions row, build
  something else). Owner has final say on all user-facing syntax. Measure twice,
  cut once — full design pass before code, no "fix it later" seams.
- Pipeline order when adding syntax: `src/syntax.rs → lexer → parser → sema →
  codegen`. Never skip sema into codegen.

---

## Latest chunk (2026-06-16): U8 — nested `sources:`/`imports:` (parser layer)

Owner decided `module` stays the single outermost construct: `sources:` and
`imports:` **nest inside the module body** as siblings of the `env.dev: Env { … }`
contribution (NOT file top-level fields — the parser rejects those, E0003). This
dissolved the blocker that the typed `env.jet` surface needed a top-level-field
grammar. Ratified as **U8** (amends U4).

Landed this run (parser foundation only):
- `ModuleDecl.sources: Vec<SourceDecl>` + `ModuleDecl.imports: Vec<Expr>`
  (`src/ast.rs`). `SourceDecl` records the `provider@target` ref as a **span**
  (the ref isn't a single token); modeval will slice + `classify_provider_ref`.
- Parser dispatch in `module_decl` (`src/parser.rs`): `sources`/`imports` field
  parsers; `imports: find("./modules")` parses as an ordinary call expr.
- `syntax::MODULE_FIELD_SOURCES`/`MODULE_FIELD_IMPORTS` (U8, I7).
- U8 ratified in `docs/spec/syntax-decisions.md` + ledger; unified-ecosystem
  §2.2/§2.3/§11 amended to show the nesting. `tests/decisions.rs` green.
- Tests: `tests/modules.rs::parses_nested_sources_and_imports` (+ empty-fields).

**Not yet wired:** `modeval` still ignores `sources`/`imports`; the CLI still
reads the Phase-1 `pkg.*` scanner. That is the next chunk (see Next up).

## Where we are (verified 2026-06-16)

Recent arc (committed): the `pack.jet`→`env.jet` clean break + computed modules
+ manifest reshape, finishing with `examples/jetpack` made runnable end-to-end.
Last commit on the arc: **`98ff3be`** ("Step 4: make examples/jetpack runnable
as env.jet, end-to-end test"). `cargo test` green; `jet run
examples/features/01_hello.jet` prints `hello, world`.

### Implemented & shipping (Phase-1 bootstrap)
- Two binaries: `jet`, `jetpack` (Cargo.toml `[[bin]]`).
- `pack.jet` manifest: `package:`/`deps:`/`exports: [module …]`/`edition`
  parsed in `src/jetpack/packmanifest.rs` (tested).
- Hangar store `/etc/jet/hangar` (`syntax::HANGAR_DIR`); unified lockfile
  `.jet/lock` (`syntax::UNIFIED_LOCK_FILE`); `.jet/` managed folder via
  `src/jetpack/store.rs`.
- `module name {}` + leading-`_` disable: parser (`src/parser.rs:3255`), AST
  `ModuleDecl.disabled`, skipped in `src/jetpack/modeval.rs` (tested).
- Namespaces `env`/`system`/`image` parsed + validated (diagnostic **E0960**,
  `src/parser.rs:3278`).
- `Pkg` sugar `default.ripgrep` / `default.[ripgrep, fd]` / `unstable.neovim`
  (`src/jetpack/merge.rs`) and `provider@target` refs `github@`/`path@`/
  `nixpkgs@` (`src/jetpack/refspec.rs`).
- Phase-1 `env.jet` directive surface (`import jetpack as pkg; pub fn shell() ->
  [JSON] { pkg.source/packages/prompt/package }`) — parsed structurally by
  `src/jetpack/envfile.rs`, loaded by `src/jetpack/cli.rs`. Commands:
  `run/build/list/clean/add/remove`.
- `core` provider (first-party Jet packages, no nix) + `nix` provider, realize
  into the hangar (`src/jetpack/provider.rs`). Offline e2e tests in
  `tests/jetpack.rs` (incl. `committed_example_builds_offline_end_to_end`).
- U1–U7 ratified in `docs/spec/syntax-decisions.md`, enforced by
  `tests/decisions.rs`.

---

## The gaps (ratified/designed but NOT implemented)

Ordered roughly by leverage. Each is a candidate chunk; confirm the relevant
decision is Ratified (not just designed) before coding, and write the failing
test/example first.

1. **`modeval` is not wired into the `jetpack` binary.** The typed
   module-evaluation engine (`src/jetpack/modeval.rs`) parses `module {}`,
   validates namespaces, expands `Pkg` sugar, and merges contributions — but is
   **only unit-tested**; the sole non-test reference is `pub mod modeval;`
   (`src/jetpack/mod.rs:16`). The CLI still reads the Phase-1 `pkg.*` scanner.
   *Highest-leverage next step: make `jetpack build`/`run` evaluate the typed
   `env.jet` surface via `modeval`.*
2. **`find("./modules")` auto-discovery — not implemented.** `BUILTIN_FIND`
   constant exists (`src/syntax.rs:442`) but is referenced nowhere; no directory
   walk. Needed for the import-tree surface (U4).
3. **Typed `env.jet` surface not loaded by the CLI.** `sources: {}` /
   `imports: find(...)` / `module dev { env.dev: Env { … } }` parses but jetpack
   ignores it in favor of the bootstrap directives. (Depends on #1 + #2.)
4. **`config.jet` + the entire `jetos` tier — not implemented.** `CONFIG_FILE`
   constant only (`src/syntax.rs:438`); never loaded; no `jetos` binary, no
   `src/jetos/`. Scale-3 (`jetos switch/build`).
5. **`System`/`Image` semantics inert.** Parsed/validated syntactically but no
   field checking or runtime; only `env`/`Env` is meaningful today.
6. **Scale-2 commands missing:** no `jet dev` / `jetpack enter`. jetpack CLI is
   `run/build/list/clean/add/remove` only.

---

## Next up (pick one chunk; owner to confirm)

**Continue #1 — wire `modeval` into `jetpack`** (U8 parser foundation now landed).
Next chunk:
1. `modeval`: evaluate `ModuleDecl.sources` (slice `ref_span`, validate via
   `refspec::classify_provider_ref`) into a `refspec::SourceTable`; surface
   conflicts as the existing E0967 (U5 merge).
2. `modeval`/CLI: map merged `env.*` `MergedEntry` → `RunPlan` — each `Pkg`
   (source filled from the sources table; bare/`default` → the table's default)
   becomes a `<source>:<package>` ref; the `prompt` scalar → the label.
3. `jetpack build`/`run`: load `env.jet` via `modeval` when it declares
   `module {}` blocks (keep the `pkg.*` directive path as fallback for the
   Phase-1 surface until the typed example replaces it).
4. Example + offline e2e: a typed `examples/jetpack/env.jet` resolving via
   fixtures (mirror `committed_example_builds_offline_end_to_end`).

`imports: find(...)` parses but the directory walk is still gap #2 — decide with
owner whether to fold `find` into this chunk or keep it separate (it uses
`BUILTIN_FIND`, `src/syntax.rs`). Write the failing e2e/example first.

---

## Pre-existing working-tree noise (not part of the arc; leave unstaged)

`.claude/settings.local.json`, deleted `.github/workflows/release.yml`, deleted
`scripts/gen_errors.sh`, `docs/plans/owner-todo.md`,
`docs/spec/decision-ballots.md`, and untracked
`docs/plans/{fan-out-and-fixed-size-lists,persona-examples}.md`,
`docs/spec/decision-ballots.html`. Confirm with the owner before touching these.
