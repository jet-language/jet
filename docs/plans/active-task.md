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
(U1–U9) as implemented — see the gap list below (e.g. U9 inferred provider kind
is ratified but not built; `System`/`Image` types parse but are inert).

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

## Latest chunk (2026-06-16): U4 `find(…)` import-tree discovery

`imports: find("./modules")` now **walks the tree and merges** — the headline
"drop a file in `modules/` and it joins the env" feature. Closes gap #2.

Landed this run:
- `modeval::evaluate_env` restructured around an internal `EvalUnit { items,
  src, base_dir }`: the root `env.jet` plus one unit per discovered `*.jet`.
  Each unit owns its own source (spans index into it) and base dir, so
  `embed_file`/relative refs resolve per-file. `build_source_table` and module
  evaluation now run across **all** units, so a discovered module's `env.dev`
  packages and `sources:` fold into the **same** §6 merge — cross-file source
  conflicts still surface as **E0967**.
- `discover_imports` walks each root module's `ModuleDecl.imports`: detects
  `Expr::Call { name == BUILTIN_FIND }`, slices the string-literal dir arg,
  resolves it against `base_dir`, lists `*.jet` (sorted, by `syntax::FILE_EXT`),
  parses each, and emits an `EvalUnit`. Discovery is **one level deep**
  (liftability law, U4): a discovered file with its own `imports:` is **E0971**.
- New diagnostics (I4, index + detail in `diagnostics.md`, inline `render_all`
  snapshots in `modeval` tests): **E0969** (an `imports:` directive isn't
  `find("<dir>")` with a literal path), **E0970** (`find` dir doesn't exist),
  **E0971** (discovered module imports — liftability). A discovered file's source
  error is span-less (the CLI only renders the root `env.jet`).
- Example is now the executable spec for discovery: `examples/jetpack-typed/`
  gained `imports: find("./modules")`, `modules/tools.jet` (adds `default.jq`),
  and `fixtures/default-jq.json`. `tests/jetpack.rs::
  typed_module_example_builds_offline_end_to_end` now asserts **"built 3
  package(s)"** incl. `jq`.

**Still open:** typed `core` sources — now **ratified as U9** (kind inferred from
the target's `pack.jet`, no marker) and queued as the next chunk (see Next up),
**not** blocked anymore; `config.jet`/jetos tier (gap #4); `System`/`Image`
semantics (gap #5); `jet dev`/`jetpack enter` (gap #6).

## Prior chunk (2026-06-16): wire `modeval` into the `jetpack` CLI

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
before they work — deferred. *(Superseded 2026-06-16: resolved as **U9** — the
kind is inferred from the target's `pack.jet`, no marker; see Next up.)*

## Earlier chunk (2026-06-16): U8 — nested `sources:`/`imports:` (parser layer)

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

Last **committed**: **`11e7df8`** ("Wire modeval into jetpack: typed env.jet
build/run end-to-end").

**⚠ Uncommitted in the working tree (this session — intended, ready to commit):**
1. The **U4 `find(…)` chunk** — `src/jetpack/modeval.rs`, `tests/jetpack.rs`,
   `docs/spec/diagnostics.md` (E0969–E0971), and new example files
   (`examples/jetpack-typed/modules/tools.jet`, `fixtures/default-jq.json`,
   edited `env.jet`).
2. The **U9 decision** (docs only, no code): `docs/spec/syntax-decisions.md`
   (U9 entry + ledger), `docs/plans/jetpack-jetos/unified-ecosystem.md` §6, and
   this file.

A fresh agent should **commit these first** (the find chunk as one commit, the
U9 docs as another) before starting the U9 implementation chunk. Full `nix
develop -c cargo test` was green after the find chunk; the edits since are
markdown-only (re-running `cargo test --test decisions` stays green). `jet run
examples/features/01_hello.jet` prints `hello, world`.

Recent arc: `pack.jet`→`env.jet` clean break + computed modules + manifest
reshape (`98ff3be`) → U8 nested `sources:`/`imports:` parser → `modeval` wired
into the CLI (`11e7df8`) → **U4 `find(…)` discovery (uncommitted)** → **U9
inferred provider kind ratified (uncommitted; not yet built)**. The typed
`module { … }` `env.jet` surface builds and realizes end-to-end, including
`find(…)` import-tree discovery.

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
- U1–U9 ratified in `docs/spec/syntax-decisions.md`, enforced by
  `tests/decisions.rs` (U9 is behavior-only — no `syntax.rs` constant).

---

## The gaps (ratified/designed but NOT implemented)

Ordered roughly by leverage. Each is a candidate chunk; confirm the relevant
decision is Ratified (not just designed) before coding, and write the failing
test/example first.

1. ~~**`modeval` is not wired into the `jetpack` binary.**~~ **DONE
   (2026-06-16).** `jetpack build`/`run` now evaluate a typed `env.jet` via
   `modeval::evaluate_env`, routed by `modeval::is_module_surface`. Directive
   scanner remains the fallback.
2. ~~**`find("./modules")` auto-discovery — not implemented.**~~ **DONE
   (2026-06-16).** `imports: find("./modules")` now walks the dir, parses each
   `*.jet`, and folds its modules into the same `evaluate_env` merge. Liftability
   enforced (E0971); bad/missing `find` is E0969/E0970.
3. **Typed `env.jet` surface loaded by the CLI — DONE for
   `sources:`/`imports:`/`env.*` (2026-06-16).** `module dev { sources: {…}
   imports: find(…) env.dev: Env { … } }` now builds and realizes, including
   discovered modules. Remaining: `config.jet`/`system`/`image` (gaps #4/#5).
4. **`config.jet` + the entire `jetos` tier — not implemented.** `CONFIG_FILE`
   constant only (`src/syntax.rs:438`); never loaded; no `jetos` binary, no
   `src/jetos/`. Scale-3 (`jetos switch/build`).
5. **`System`/`Image` semantics inert.** Parsed/validated syntactically but no
   field checking or runtime; only `env`/`Env` is meaningful today.
6. **Scale-2 commands missing:** no `jet dev` / `jetpack enter`. jetpack CLI is
   `run/build/list/clean/add/remove` only.

---

## Next up — START HERE: U9, infer the source provider kind

**Now ratified (U9, 2026-06-16) — no syntax gate.** The owner dissolved the old
`via: core` blocker by deciding the kind is **inferred, never declared**: a
source stays `name: provider@target` with no marker, and core-vs-nix is
discovered from the target. This is the last gap between the typed and directive
source surfaces. Today `modeval::build_source_table` hard-codes
`ProviderKind::Nix`, so a typed env can't realize a first-party `core` source.

**The rule (U9; see syntax-decisions.md + unified-ecosystem.md §6):** for a
source's resolved target — target has a **`pack.jet`** → **core**; else → **nix
flake**. The probe must not clone a nixpkgs-sized repo: `path@…` stats the dir
locally; `nixpkgs@…` is unconditionally nix (never probed); `github@…`/git URLs
peek at **only** `pack.jet` (GitHub raw / shallow `git archive`) before any full
fetch.

**Concrete plan (test-first, one chunk):**
1. **Failing e2e first** (`tests/jetpack.rs`): a typed `examples/jetpack-typed/`
   variant (or a scratch project) whose `sources:` includes a `path@./jet-pkgs`
   target containing a `pack.jet` → assert it realizes a first-party package with
   **no nix** (mirror `core_provider_runs_first_party_package_without_nix`, but
   through the typed surface). Keep a nix-backed source alongside to prove the
   fallback still works.
2. **Stop hard-coding the kind.** `build_source_table` currently passes
   `ProviderKind::Nix` for every decl. Replace with a probe: resolve the target,
   detect `pack.jet`, choose `Core`/`Nix`. Decide eager (at table build) vs lazy
   (at realize, in `provider::pick`/`source_repo`) — lazy is cheaper (only probes
   sources actually used) and co-locates with the fetch the core provider already
   does. Entry points: `src/jetpack/modeval.rs::build_source_table`,
   `src/jetpack/refspec.rs` (`ProviderKind`, `classify_provider_ref`,
   `SourceTable::provider`), `src/jetpack/provider.rs` (`source_repo`, `pick`).
3. **Probe guardrails:** `nixpkgs@` short-circuits to nix; the `github@`/git
   manifest peek fetches only `pack.jet`. Reconcile the marker: the core provider
   today reads the source repo's **`env.jet`** `pkg.package(...)` index, but U9
   keys discovery on **`pack.jet`** — pin which file marks a Jet package repo and
   note any follow-up if they must converge.
4. Update this doc; unified-ecosystem.md §6 already carries the rule.

**Larger, later (gaps #4/#5/#6), each its own multi-chunk arc:**
- #5 `System`/`Image` semantics — parsed/validated but inert; only `Env` means
  anything. Needs field checking + a realize path.
- #4 `config.jet` + the `jetos` tier — no loader, no `jetos` binary, no
  `src/jetos/`. Scale-3.
- #6 Scale-2 commands — `jet dev` / `jetpack enter`.

---

## Pre-existing working-tree noise (not part of the arc; leave unstaged)

**⚠ Stage selectively — never `git add -A`/`git add .`.** The tree holds
substantial **unrelated owner edits** alongside the arc work. Commit only the
files listed under "Where we are" (the U4 find chunk, then the U9 docs); leave
everything below untouched and confirm with the owner before touching any of it:

- **Owner's own in-progress doc edits** (do not commit with the arc):
  `docs/plans/epoch-2/*.md` (ratification annotations — m2…m11, README),
  `docs/plans/owner-todo.md`, `docs/spec/decision-ballots.md`,
  `docs/spec/decision-ballots.html`.
- **Other noise:** `.claude/settings.local.json`, deleted
  `.github/workflows/release.yml`, deleted `scripts/gen_errors.sh`, untracked
  `docs/plans/{fan-out-and-fixed-size-lists,persona-examples}.md`.

Note: `docs/spec/syntax-decisions.md` and
`docs/plans/jetpack-jetos/unified-ecosystem.md` **are** part of the arc (U9
edits) — those go in the U9-docs commit, not the noise pile.
