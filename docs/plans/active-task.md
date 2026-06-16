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
(U1–U8) as implemented — see the gap list below (e.g. U4 `find` is ratified but
not built).

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

## Latest chunk (2026-06-16): wire `modeval` into the `jetpack` CLI

The typed `module { … }` env surface is now **evaluated and realized** end to
end, not just unit-tested. Closes gap #1; gap #3 (typed surface loaded by the
CLI) is now true for `env.jet` `build`/`run`.

Landed this run:
- `modeval::evaluate_env(src, base_dir) -> EnvPlan` (`{ table, package_refs,
  prompt }`): builds the §6-merged `SourceTable` from every module's `sources:`
  (each `provider@target` ref sliced from `SourceDecl.ref_span`, translated
  at-form→colon-form, U6), merges all `env.*` contributions, expands `Pkg` sugar
  to `<source>:<package>` refs (bare/`default` → the `default` source), and
  takes the merged `prompt` as the label. Source conflicts surface as **E0967**
  (U5); a non-`provider@target` source ref is the new **E0968**.
- `modeval::is_module_surface(src)` — the CLI routes on it: a file that parses
  with ≥1 `module` decl goes through `evaluate_env`; everything else falls back
  to the tolerant Phase-1 `pkg.*` directive scanner.
- `src/jetpack/cli.rs::load_project_plan` reads `env.jet` text once and branches
  (typed via `typed_plan` → modeval; else `envfile::parse`). Shared
  `classify_all` helper.
- Diagnostic **E0968** added to `docs/spec/diagnostics.md` (index + detail).
- Example + offline e2e: `examples/jetpack-typed/env.jet` (+ `fixtures/`) and
  `tests/jetpack.rs::typed_module_example_builds_offline_end_to_end` mirror the
  directive `committed_example_builds_offline_end_to_end`.

**Still open:** `imports: find(…)` parses but is **not walked** (gap #2) — kept
separate per the note below. `System`/`Image` namespaces still inert (gap #5).
The typed surface lacks a `via: core` source marker (the directive surface has
one); core-provider sources in a typed `env.jet` need an owner syntax decision
before they work — deferred.

## Prior chunk (2026-06-16): U8 — nested `sources:`/`imports:` (parser layer)

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

Last commit: **`11e7df8`** ("Wire modeval into jetpack: typed env.jet build/run
end-to-end"). `nix develop -c cargo test` green (278 passed, 0 failed); `jet run
examples/features/01_hello.jet` prints `hello, world`.

Recent arc (committed): `pack.jet`→`env.jet` clean break + computed modules +
manifest reshape (`98ff3be`) → U8 nested `sources:`/`imports:` parser (prior
chunk) → `modeval` wired into the CLI (latest chunk). The typed `module { … }`
`env.jet` surface now builds and realizes end-to-end.

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
- U1–U8 ratified in `docs/spec/syntax-decisions.md`, enforced by
  `tests/decisions.rs`.

---

## The gaps (ratified/designed but NOT implemented)

Ordered roughly by leverage. Each is a candidate chunk; confirm the relevant
decision is Ratified (not just designed) before coding, and write the failing
test/example first.

1. ~~**`modeval` is not wired into the `jetpack` binary.**~~ **DONE
   (2026-06-16).** `jetpack build`/`run` now evaluate a typed `env.jet` via
   `modeval::evaluate_env`, routed by `modeval::is_module_surface`. Directive
   scanner remains the fallback.
2. **`find("./modules")` auto-discovery — not implemented.** `BUILTIN_FIND`
   constant exists (`src/syntax.rs:442`) but is referenced nowhere; no directory
   walk. Needed for the import-tree surface (U4).
3. **Typed `env.jet` surface loaded by the CLI — DONE for `sources:`/`env.*`
   (2026-06-16).** `module dev { sources: {…} env.dev: Env { … } }` now builds
   and realizes. Remaining: `imports: find(...)` is parsed but ignored (folds
   into gap #2), and `config.jet`/`system`/`image` (gaps #4/#5).
4. **`config.jet` + the entire `jetos` tier — not implemented.** `CONFIG_FILE`
   constant only (`src/syntax.rs:438`); never loaded; no `jetos` binary, no
   `src/jetos/`. Scale-3 (`jetos switch/build`).
5. **`System`/`Image` semantics inert.** Parsed/validated syntactically but no
   field checking or runtime; only `env`/`Env` is meaningful today.
6. **Scale-2 commands missing:** no `jet dev` / `jetpack enter`. jetpack CLI is
   `run/build/list/clean/add/remove` only.

---

## Next up — START HERE: gap #2, `find(...)` auto-discovery

This is the highest-leverage chunk and it is **ratified (U4) and ready** — no
syntax gate to clear. The typed surface evaluates today, but the import-tree
(the headline "drop a file in `modules/` and it merges" feature) is inert.

**Concrete plan (test-first, one chunk):**

1. **Failing e2e first** (`tests/jetpack.rs`, mirror
   `typed_module_example_builds_offline_end_to_end`): extend the
   `examples/jetpack-typed/` project with a `modules/tools.jet` that declares
   `module tools { env.dev: Env { packages: [default.jq] } }`, keep
   `imports: find("./modules")` in the root `env.jet`, add a `default-jq.json`
   fixture, and assert `jetpack build` realizes `jq` too (→ "built 3 package(s)").
2. **Implement the walk in `modeval`.** `evaluate_env` already parses the root
   file and has `base_dir`. After parsing, for each module's
   `ModuleDecl.imports` (a `Vec<Expr>`), detect `Expr::Call { name == "find", .. }`
   (`syntax::BUILTIN_FIND`, currently referenced nowhere), resolve its string
   arg relative to `base_dir`, walk that dir for `*.jet`, parse each, and feed
   their `Item::Module`s into the **same** `evaluate_modules` + `merge_all`
   pass (and their `sources:` into `build_source_table`). Reuse, don't fork, the
   existing merge so cross-file source conflicts still surface as **E0967**.
3. **Guardrails:** skip `_`-disabled modules (already handled by
   `evaluate_modules`); **liftability law** — discovered modules may not import
   each other (a `find` inside a discovered file is an error; pick/define a
   diagnostic, add it to `diagnostics.md` + a `tests/ui` snapshot per I4).
   Missing `find` dir and non-literal `find` arg each need a diagnostic too.
4. Update `unified-ecosystem.md` §4 if behavior nuances emerge; update this doc.

Entry points: `src/jetpack/modeval.rs` (`evaluate_env`, `evaluate_modules`,
`build_source_table`); `Expr::Call` shape in `src/ast.rs`; `syntax::BUILTIN_FIND`.

### Also queued (do NOT start without owner sign-off)

- **`via: core` for typed sources** — *blocked on owner syntax decision.* The
  directive surface has a third `pkg.source(name, upstream, "core")` arg; the
  typed `sources: { name: provider@target }` has no slot for the provider kind,
  so `build_source_table` hard-codes `ProviderKind::Nix`. A typed env can't yet
  declare a first-party `core` source. **Add an Open Decisions row to
  `syntax-decisions.md` and STOP** until ratified.
- Gaps #4 (`config.jet`/jetos tier), #5 (`System`/`Image` semantics), #6
  (`jet dev`/`jetpack enter`) — larger, later.

---

## Pre-existing working-tree noise (not part of the arc; leave unstaged)

`.claude/settings.local.json`, deleted `.github/workflows/release.yml`, deleted
`scripts/gen_errors.sh`, `docs/plans/owner-todo.md`,
`docs/spec/decision-ballots.md`, and untracked
`docs/plans/{fan-out-and-fixed-size-lists,persona-examples}.md`,
`docs/spec/decision-ballots.html`. Confirm with the owner before touching these.
