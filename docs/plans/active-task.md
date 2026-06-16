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
is built for `path@…` but the `github@…`/git remote probe is still a follow-up;
`System`/`Image` types parse but are inert).

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

## Latest chunk (2026-06-16): U9 infer the source provider kind

A typed `sources: { name: provider@target }` source now realizes through the
right backend with **no marker** — the kind is inferred from the resolved
target (U9). Today `path@…` is fully wired; `github@…`/git remote probing is the
one remaining follow-up (see Next up).

Landed this run:
- `modeval::build_source_table` no longer hard-codes `ProviderKind::Nix`. New
  `infer_provider_kind(pref, base_dir)`: a `path@…` target with a **`pack.jet`**
  → **core**; `nixpkgs@…` → **nix** (never probed); `github@…`/git → **nix** for
  now. The probe resolves `path@./local` against the **declaring file's**
  `base_dir` (so a discovered module's relative source resolves where it was
  written), and stats locally — offline, no clone. Kinds are recorded per source
  name and threaded into `SourceTable::from_decls`; the §6 merge still
  guarantees one upstream per name (E0967), so the kind is consistent.
- The provider boundary was already kind-aware: `provider::provider_for` /
  `uses_nix_provider` read `table.provider(name)`, so once the table carries
  `Core` the typed surface picks `CoreProvider` with no further change.
- **Marker reconciliation (U9 step 3):** discovery keys on **`pack.jet`** (the
  ratified marker), but `CoreProvider` still reads the source repo's **`env.jet`**
  `pkg.package(...)` index to map a package name → subpath. A typed `core` repo
  therefore carries both today. *Follow-up:* converge so the core provider reads
  `pack.jet`'s `exports:` and the dual marker retires.
- Test-first e2e: `tests/jetpack.rs::typed_core_source_inferred_from_pack_jet` —
  a typed `module dev { sources: { mine: path@<dir> } env.dev: Env { packages:
  [mine.hello] } }` whose target has a `pack.jet`, run with **no nix on PATH**,
  realizes the first-party `hello` and prints its output. The nix fallback stays
  proven by `typed_module_example_builds_offline_end_to_end`.

**Still open:** the `github@…`/git remote `pack.jet` peek (needs realize-time
`Ctx`; see Next up); `config.jet`/jetos tier (gap #4); `System`/`Image`
semantics (gap #5); `jet dev`/`jetpack enter` (gap #6).

## Prior chunk (2026-06-16): U4 `find(…)` import-tree discovery

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

Last **committed**: the U9 implementation chunk (`build_source_table` infers the
kind; `tests/jetpack.rs::typed_core_source_inferred_from_pack_jet`). Preceding
arc commits this session: the **U4 `find(…)` chunk** (`4054b2b`) and the **U9
docs** (`bf1c645`), both on top of the owner's `fec9230` "Planning" commit.

**⚠ Working tree:** only owner noise remains uncommitted (see the noise section
below) — leave it. Full `nix develop -c cargo test` is green (0 failed across
the suite); `jet run examples/features/01_hello.jet` prints `hello, world`.

Recent arc: `pack.jet`→`env.jet` clean break + computed modules + manifest
reshape (`98ff3be`) → U8 nested `sources:`/`imports:` parser → `modeval` wired
into the CLI (`11e7df8`) → U4 `find(…)` discovery (`4054b2b`) → U9 docs
(`bf1c645`) → **U9 inferred provider kind for `path@…` (this chunk).** The typed
`module { … }` `env.jet` surface builds and realizes end-to-end, including
`find(…)` import-tree discovery and first-party `core` sources.

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
   `sources:`/`imports:`/`env.*` (2026-06-16), incl. U9 inferred provider kind
   for `path@…` core sources.** `module dev { sources: {…} imports: find(…)
   env.dev: Env { … } }` builds and realizes, including discovered modules and
   first-party `core` sources. Remaining: U9 `github@…`/git remote probe (see
   Next up); `config.jet`/`system`/`image` (gaps #4/#5).
4. **`config.jet` + the entire `jetos` tier — not implemented.** `CONFIG_FILE`
   constant only (`src/syntax.rs:438`); never loaded; no `jetos` binary, no
   `src/jetos/`. Scale-3 (`jetos switch/build`).
5. **`System`/`Image` semantics inert.** Parsed/validated syntactically but no
   field checking or runtime; only `env`/`Env` is meaningful today.
6. **Scale-2 commands missing:** no `jet dev` / `jetpack enter`. jetpack CLI is
   `run/build/list/clean/add/remove` only.

---

## Next up — START HERE: U9 remote probe (`github@…`/git core sources)

U9 for `path@…` shipped this chunk (kind inferred from a local `pack.jet`). The
one remaining piece of the same rule: a `github@…`/git source whose target is a
Jet package repo should also realize as **core**. Today `infer_provider_kind`
returns `Nix` for every non-`path@` provider, so a typed `github@owner/jet-pkgs`
source can't be first-party.

**The rule is unchanged (U9; syntax-decisions.md + unified-ecosystem.md §6):**
peek at **only** `pack.jet` over the network (GitHub raw, e.g.
`https://raw.githubusercontent.com/<owner>/<repo>/<rev>/pack.jet`, or a shallow
`git archive`) — never clone a nixpkgs-sized repo — and choose `Core`/`Nix` from
whether that file exists. `nixpkgs@…` stays unconditionally nix.

**Why it's a separate chunk:** the probe needs realize-time context the pure
`modeval` evaluation doesn't have — the `--offline` flag (offline must not hit
the network; fall back to a cached checkout or default to nix) and the source
cache dir. So this likely moves the kind decision **lazy**, into
`provider::provider_for`/`uses_nix_provider` (which already take the
`SourceTable` and run where `Ctx` is available), or threads a probe result from
`source_repo`'s fetch. Entry points: `src/jetpack/provider.rs`
(`provider_for`, `uses_nix_provider`, `source_repo`, `fetch_remote_repo`),
`src/jetpack/modeval.rs::infer_provider_kind` (today's local path probe).

**Also reconcile the marker (carried over from U9 step 3):** discovery keys on
**`pack.jet`** but `CoreProvider::realize` still reads the repo's **`env.jet`**
`pkg.package(...)` index to map package → subpath, so a core repo carries both.
Converge them — have the core provider read `pack.jet`'s `exports:` — so a Jet
package repo needs only `pack.jet`. This pairs naturally with the remote chunk
(the remote peek already fetches `pack.jet`).

**Larger, later (gaps #4/#5/#6), each its own multi-chunk arc:**
- #5 `System`/`Image` semantics — parsed/validated but inert; only `Env` means
  anything. Needs field checking + a realize path.
- #4 `config.jet` + the `jetos` tier — no loader, no `jetos` binary, no
  `src/jetos/`. Scale-3.
- #6 Scale-2 commands — `jet dev` / `jetpack enter`.

---

## Pre-existing working-tree noise (not part of the arc; leave unstaged)

**⚠ Stage selectively — never `git add -A`/`git add .`.** The tree holds
substantial **unrelated owner edits** alongside the arc work. The arc commits
this session (U4 find, U9 docs, U9 impl) are already landed; leave everything
below untouched and confirm with the owner before touching any of it:

- **Owner's own in-progress doc edits** (do not commit with the arc):
  `docs/plans/epoch-2/*.md` (ratification annotations — m2…m11, README),
  `docs/plans/owner-todo.md`, `docs/spec/decision-ballots.md`,
  `docs/spec/decision-ballots.html`.
- **Other noise:** `.claude/settings.local.json`, deleted
  `.github/workflows/release.yml`, deleted `scripts/gen_errors.sh`, untracked
  `docs/plans/{fan-out-and-fixed-size-lists,persona-examples}.md`.

Note: `docs/spec/syntax-decisions.md` and
`docs/plans/jetpack-jetos/unified-ecosystem.md` were part of the arc (U9 edits)
and are already committed (`bf1c645`) — not noise.
