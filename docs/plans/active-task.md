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
is fully built for both `path@…` and `github@…`, but a `core` source still
carries the dual `pack.jet`+`env.jet` marker; `System`/`Image` types parse but
are inert).

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

## Latest chunk (2026-06-16): U9 remote probe (`github@…` core sources)

A typed `github@…` source now realizes as **core** when its remote repo carries
a `pack.jet` — completing the U9 rule for the remote case (the `path@…` local
case shipped the prior chunk). The kind is decided by a lightweight git peek at
realize time, never cloning a nixpkgs-sized repo to classify it.

Landed this run:
- **New `ProviderKind::Infer`** (`refspec.rs`): a third, *unresolved* kind the
  table records for `github@…` sources. `modeval::infer_provider_kind` now
  returns `Infer` for `Source::Github` (was `Nix`) — its core-vs-nix kind can't
  be known during pure `evaluate_env` (no offline flag / source cache there).
  `path@…` stays resolved eagerly (local stat); `nixpkgs@…` stays `Nix`.
- **Realize-time resolution** (`provider.rs`): `resolve_kind(spec, table,
  offline, cache_dir)` turns `Infer` into a concrete `Core`/`Nix`; it never
  reaches a provider. Order: (1) reuse a prior source-cache checkout (offline-
  safe), (2) offline + no cache → `Nix` (never hits the network), (3) online →
  `infer_remote_kind`/`remote_has_pack_jet`: `git init` a throwaway repo, add
  `origin`, `git fetch --depth 1 --filter=tree:0 origin <rev>` (one commit
  object; trees/blobs deferred), then `git ls-tree FETCH_HEAD pack.jet`. Present
  → `Core`; absent or any peek failure → `Nix` (safe default — a github *flake*
  still realizes through nix). `git fetch <rev>` resolves a branch, tag, **or**
  commit SHA uniformly, so the rev's exact `pack.jet` is peeked no matter how it
  was pinned — no nixpkgs-sized clone to classify.
- **Dispatch threads the resolved kind**: `provider_for(kind)` (was
  `provider_for(spec, table)`), `uses_nix_provider(spec, table, offline,
  cache_dir)`, and `realize` resolves once from `ctx`. `cli.rs::realize_ref`
  hoists `store_dir` above the fixtures check so the probe's cache lookup is
  seeded; an inferred `core` source is no longer wrongly asked for nix fixtures.
- **Tests** (offline; the typed `github@` CLI path can't be reached without
  network, so the probe is proven at the library boundary with `file://` repos):
  `provider::resolve_kind_probes_remote_pack_jet` (pack.jet → Core, none → Nix,
  offline+no-cache → Nix), `provider::remote_probe_resolves_a_commit_sha_rev`
  (SHA-pinned rev → Core), `provider::realize_resolves_inferred_remote_to_core`
  (full Infer→CoreProvider→fetch→build), `modeval::
  github_source_kind_is_left_to_inference` / `nixpkgs_source_kind_stays_nix`. No
  new diagnostic (behavior-only, like U9).

**Still open (the marker reconciliation, now the headline Next up):** a remote
`core` repo still needs **both** `pack.jet` (for the probe) and `env.jet` (the
`pkg.package(...)` index `CoreProvider` reads) — the dual marker. Converging
these has an unresolved manifest-design question (see Next up).

## Prior chunk (2026-06-16): U9 infer the source provider kind (`path@…`)

A typed `sources: { name: provider@target }` source realizes through the
right backend with **no marker** — the kind is inferred from the resolved
target (U9). `path@…` fully wired (kind from a local `pack.jet`).

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

**Open at the time:** the `github@…` remote `pack.jet` peek — **since shipped**
as the Latest chunk above. Still open beyond it: the dual marker (Next up);
`config.jet`/jetos tier (gap #4); `System`/`Image` semantics (gap #5); `jet
dev`/`jetpack enter` (gap #6).

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

**Followed by:** typed `core` sources — **ratified and built as U9** for
`path@…` (kind inferred from the target's `pack.jet`, no marker; the chunk above).

## Earlier committed chunks (history; full detail in git)

- **`11e7df8` — wire `modeval` into the `jetpack` CLI.** `modeval::evaluate_env`
  → `EnvPlan { table, package_refs, prompt }`; `is_module_surface` routes a
  `module`-bearing `env.jet` through it (else the Phase-1 `pkg.*` scanner).
  `cli.rs::load_project_plan` branches on it. Diagnostics **E0967** (source
  conflict, U5), **E0968** (non-`provider@target` source ref). Closed gap #1.
- **`29ca551` — U8 nested `sources:`/`imports:` (parser layer).** Owner kept
  `module` as the single outermost construct: `sources:`/`imports:` nest **inside**
  the body (top-level fields are rejected, E0003). `ModuleDecl.sources` /
  `ModuleDecl.imports` in `src/ast.rs`; `syntax::MODULE_FIELD_SOURCES`/`_IMPORTS`.
  Ratified as U8 (amends U4).
- **`98ff3be` and earlier** — `pack.jet`→`env.jet` clean break, computed
  modules, manifest reshape (S52/U1), `pack.jet` as the compiler manifest.

## Where we are (verified 2026-06-16)

Last **landed** (working tree, **not yet committed** — commit it first): the U9
**remote probe** — `ProviderKind::Infer` (`refspec.rs`) +
`provider::{resolve_kind, infer_remote_kind, remote_has_pack_jet}`, dispatch
rewired (`provider_for(kind)`, `uses_nix_provider(…, offline, cache_dir)`,
`cli.rs::realize_ref`). New tests: `provider::{resolve_kind_probes_remote_pack_jet,
remote_probe_resolves_a_commit_sha_rev, realize_resolves_inferred_remote_to_core}`,
`modeval::{github_source_kind_is_left_to_inference, nixpkgs_source_kind_stays_nix}`.
Touched: `src/jetpack/{refspec,modeval,provider,cli}.rs` + this doc.
Last **committed**: `9c29ef6` (handoff refresh) on top of the U9 `path@…`
implementation (`4d5aca6`).

**⚠ Working tree:** the remote-probe arc edits above are ready to commit;
everything else is owner noise (see the noise section) — stage selectively,
never `git add -A`. Full `nix develop -c cargo test` is green (0 failed across
the suite); `jet run examples/features/01_hello.jet` prints `hello, world`.

Recent arc: `pack.jet`→`env.jet` clean break + computed modules + manifest
reshape (`98ff3be`) → U8 nested `sources:`/`imports:` parser → `modeval` wired
into the CLI (`11e7df8`) → U4 `find(…)` discovery (`4054b2b`) → U9 docs
(`bf1c645`) → U9 inferred provider kind for `path@…` (`4d5aca6`) → **U9 remote
probe for `github@…` (this chunk, uncommitted).** The typed `module { … }`
`env.jet` surface builds and realizes end-to-end, including `find(…)`
import-tree discovery and first-party `core` sources from both local (`path@`)
and remote (`github@`) repos.

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
   for both `path@…` (local `pack.jet`) and `github@…` (realize-time git peek).**
   `module dev { sources: {…} imports: find(…) env.dev: Env { … } }` builds and
   realizes, including discovered modules and first-party `core` sources.
   Remaining: converge the `core` marker onto `pack.jet` (see Next up — has an
   open manifest-design question); `config.jet`/`system`/`image` (gaps #4/#5).
4. **`config.jet` + the entire `jetos` tier — not implemented.** `CONFIG_FILE`
   constant only (`src/syntax.rs:438`); never loaded; no `jetos` binary, no
   `src/jetos/`. Scale-3 (`jetos switch/build`).
5. **`System`/`Image` semantics inert.** Parsed/validated syntactically but no
   field checking or runtime; only `env`/`Env` is meaningful today.
6. **Scale-2 commands missing:** no `jet dev` / `jetpack enter`. jetpack CLI is
   `run/build/list/clean/add/remove` only.

---

## Next up — START HERE: converge the `core` marker onto `pack.jet`

The U9 remote probe shipped (chunk above). The remaining U9 loose end: a `core`
source repo carries **two** markers — `pack.jet` (what the probe keys on) and
`env.jet` (the `pkg.package("name", "./subpath")` index `CoreProvider::realize`
reads to map a package name → its source subpath, via `envfile::provided`). A
Jet package repo should need only `pack.jet`.

**⚠ This chunk has an unresolved manifest-design question — STOP and ratify
before coding (measure twice).** The naive "have the core provider read
`pack.jet`'s `exports:`" doesn't type-check against the manifest as designed:
- `pack.jet`'s `exports:` is `[module web, module cli]` — the *public modules of
  one package* (`packmanifest.rs::parse_exports`).
- `CoreProvider` needs a *package-name → source-subpath* map for a repo that
  provides **many** packages (`hello` → `./pkgs/hello`).
These are different shapes. A single-package `pack.jet` doesn't currently
express "this repo is a multi-package `core` source." Options to put to the
owner (none ratified): (a) a workspace-style `pack.jet` that lists member
package dirs; (b) the core source *is* one package and `exports:`-modules map to
bin subpaths; (c) keep a small index block in `pack.jet` mirroring
`pkg.package(...)`. This is owner-facing manifest surface → syntax-decision
protocol applies. Entry points once decided: `packmanifest.rs` (manifest shape),
`provider.rs::CoreProvider::realize` + `source_repo` (read `pack.jet` instead of
`env.jet`), `envfile.rs::provided` (the index it replaces).

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
