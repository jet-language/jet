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
carries the dual `payload.jet`+`env.jet` marker; `System`/`Image` types parse but
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

## Latest chunk (2026-06-16): U10 Chunk 1 — rename the manifest (`pack.jet` → `payload.jet`)

The package manifest is now **`payload.jet`** and its identity block is
**`payload: { … }`** — a clean break off `pack.jet`/`package:`, no alias (mirrors
the `jet.toml`→`pack.jet` migration). This is Chunk 1 of the U10 4-chunk arc; the
`packages:` block, recursive discovery, and `library`/`executable` realize
(Chunks 2–4) are untouched and remain next.

Landed this run:
- **`src/syntax.rs`:** retired the `PACK_FILE` constant entirely; all code reads
  `PAYLOAD_FILE` (`"payload.jet"`). `PACK_LOCK_FILE` (`pack.lock`, already
  superseded by `.jet/lock`) left alone — out of scope.
- **Parser (`src/jetpack/packmanifest.rs`):** the identity block is parsed via
  `syntax::MANIFEST_BLOCK_PAYLOAD` (`"payload"`); the `MissingPackage` error
  variant → **`MissingPayload`**; `new_template` emits `payload: {`. The struct
  type `PackageMeta` and the `PackManifest.package` field keep their internal
  names (not user-facing); only the on-disk keyword + filename changed.
- **Compiler manifest (`src/manifest.rs`):** `E1206`/`E1208` copy now says
  `payload`/`payload.jet` ("no `payload: { … }` block", "the `jet` field in
  `payload`", etc.). `loader.rs`/`fetch.rs`/`lock.rs`/`main.rs` read
  `PAYLOAD_FILE` (incl. user-facing "no `payload.jet` found" messages and the
  `jet new` scaffold, which now writes `payload.jet`).
- **U9 probe stays correct:** `modeval::infer_provider_kind` and
  `provider::remote_has_pack_jet` peek `syntax::PAYLOAD_FILE`; the offline
  `provider.rs` probe tests now seed `payload.jet` fixtures with `payload:`
  content (they would silently mis-classify against the old name).
- **Tests/examples:** all on-disk + inline `pack.jet`/`package:` fixtures in
  `tests/{pkg,ffi,jetpack}.rs` and the orphan `tests/ui/manifest_*/` dirs renamed
  to `payload.jet`/`payload:`; the manifest `tests/ui/*/stderr` snapshots
  re-blessed to the new copy. (The `tests/ui/manifest_*` dirs are not run by
  `tests/ui.rs` — it only loads dirs with a `main.jet` — but were updated for
  consistency.) No new diagnostic (rename only).
- **Docs:** `unified-ecosystem.md` §2.1 + file/scale/ledger tables → `payload.jet`
  (U1/S52 decision-log lines kept historical); `diagnostics.md` E1208 row.

Verified: `nix develop -c cargo test` fully green; `cargo test --test decisions`
passes; `jet run examples/features/01_hello.jet` prints `hello, world`.

## Prior chunk (2026-06-16): U9 remote probe (`github@…` core sources)

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

**The marker reconciliation is now RATIFIED as U10** (see Next up): the manifest
becomes `payload.jet` with a `packages: { name: library|executable }` block;
packages are top-level modules discovered by name; `CoreProvider` reads that
index, not `env.jet`. Retires the dual marker.

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

Last **committed**: `819b756` ("Updating jetpack ecosystem") — the U9 **remote
probe** (`ProviderKind::Infer` in `refspec.rs`; `provider::{resolve_kind,
infer_remote_kind, remote_has_pack_jet}`; dispatch rewired to `provider_for(kind)`
/ `uses_nix_provider(…, offline, cache_dir)` / `cli.rs::realize_ref`) landed
**bundled with owner doc edits** (epoch-2, decision-ballots). U10 was then
**ratified** (`payload.jet` manifest + `packages:` model) in
`docs/spec/syntax-decisions.md` + `src/syntax.rs`. **This session implemented U10
Chunk 1** (the `pack.jet`→`payload.jet` / `package:`→`payload:` rename — see the
Latest chunk above). U10 Chunks 2–4 (`packages:` parser, recursive discovery,
`library`/`executable` realize) are **not** started — START HERE in Next up.

**⚠ Working tree:** the U10 Chunk-1 rename is **uncommitted** (src + tests +
`tests/ui/manifest_*` renames + `unified-ecosystem.md`/`diagnostics.md` doc
edits). Full `nix develop -c cargo test` is green; `cargo test --test decisions`
passes; `jet run examples/features/01_hello.jet` prints `hello, world`. When you
commit, stage selectively — owner doc noise (epoch-2, decision-ballots,
owner-todo) may reappear; never `git add -A` (see the noise section).

Recent arc: `pack.jet`→`env.jet` clean break + computed modules + manifest
reshape (`98ff3be`) → U8 nested `sources:`/`imports:` parser → `modeval` wired
into the CLI (`11e7df8`) → U4 `find(…)` discovery (`4054b2b`) → U9 docs
(`bf1c645`) → U9 inferred provider kind for `path@…` (`4d5aca6`) → U9 remote
probe for `github@…` (`819b756`) → U10 ratified (`payload.jet` + `packages:`) →
**U10 Chunk 1 implemented: manifest renamed `pack.jet`→`payload.jet` (this
session, uncommitted).** The typed `module { … }`
`env.jet` surface builds and realizes end-to-end, including `find(…)`
import-tree discovery and first-party `core` sources from both local (`path@`)
and remote (`github@`) repos.

### Implemented & shipping (Phase-1 bootstrap)
- Two binaries: `jet`, `jetpack` (Cargo.toml `[[bin]]`).
- `payload.jet` manifest: `payload:`/`deps:`/`exports: [module …]`/`edition`
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

## Next up — START HERE: U10 `payload.jet` arc (manifest rename + `packages:` model)

**Ratified 2026-06-16 as U10** (`docs/spec/syntax-decisions.md`; tokens in
`src/syntax.rs`: `PAYLOAD_FILE`, `MANIFEST_BLOCK_PAYLOAD`,
`MANIFEST_BLOCK_PACKAGES`, `PACKAGE_KIND_LIBRARY`/`_EXECUTABLE`,
`PACKAGE_FIELD_KIND`). Design is **frozen — do not re-litigate**; read U10 + the
memory `payload-multi-package-core-source` before starting. This completes the
U9 marker convergence and replaces the old "converge the core marker" next-up.

**The model:** payload → packages → modules. The manifest is renamed
`pack.jet` → **`payload.jet`**; its identity block is `payload: { … }` (was
`package:`). A **package is a top-level `module`** a dev exports; the manifest
lists packages in a **`packages: { name: kind }`** block, where `name` is a
top-level module name and `kind` is **`library`** (imported for code) or
**`executable`** (installed as a binary on PATH — the devshell case). Value is a
bare keyword *or* a `{ kind: …, … }` block (extension point). A package's
**module name is its identity**; its file is **discovered** by recursively
walking the tree for `module <name>`. This `packages:` index is what
`CoreProvider` reads — **`env.jet` is never read by the provider** (it stays the
dev-shell only). No Jet→binary compiler yet: `library` stages module source,
`executable` stages a prebuilt `bin/` (compiler slots in later).

**Do it as a 4-chunk arc (one chunk per run, test-first, stop and report):**

- **Chunk 1 — rename the manifest. ✅ DONE (2026-06-16).** `pack.jet` →
  `payload.jet`, `package:` → `payload:`; `PACK_FILE` retired onto `PAYLOAD_FILE`;
  parser keys on `MANIFEST_BLOCK_PAYLOAD` (`MissingPackage`→`MissingPayload`);
  E1206/E1208 copy + the U9 probe + all `tests/`/`tests/ui` fixtures + §2.1 docs
  updated. Clean break, no alias. See the Latest chunk above.
- **Chunk 2 — `packages:` block parser. ← START HERE.** Parse `packages: { name: <bare-kw |
  { kind, … }> }` in `packmanifest.rs`; ratify-token spellings already in
  `syntax.rs`. Fold the old `exports: [module …]` into `packages:` (remove
  `parse_exports`/`exports` field or repoint it). Diagnostics for a bad kind /
  malformed entry (new E-codes → `diagnostics.md` + `tests/ui` snapshot, I4).
  *Exit:* manifest with a `packages:` block parses to a typed structure; bad
  entries diagnose; snapshots blessed.
- **Chunk 3 — recursive package discovery.** Resolve each `packages:` name → its
  `module <name>` declaration by walking the source tree (sorted, bounded — skip
  `.jet/`/hidden/build dirs). Exactly-one-or-error: **two new diagnostics** —
  "package declared but no `module <name>`" and "ambiguous: `module <name>` in A
  and B" (I4: code + what/why/fix + `tests/ui` snapshot each). Wire into
  `CoreProvider::realize` to replace the `envfile::provided` lookup
  (`provider.rs:160`). *Exit:* a typed `core` source resolves a package by module
  name with **no `env.jet` index**; the dual marker is gone.
- **Chunk 4 — `library` vs `executable` realize.** `executable` stages the
  package dir's prebuilt `bin/` (today's behavior, now keyed by kind);
  `library` stages module source for import. Retire the `env.jet`
  `pkg.package(...)` index entirely (`PACK_DIRECTIVE_PACKAGE`, `envfile::provided`).
  *Exit:* both kinds realize offline e2e (extend `tests/jetpack.rs`); example
  updated as the executable spec (I5).

Entry points: `src/jetpack/{packmanifest,provider,modeval,envfile,cli}.rs`,
`src/syntax.rs`, `docs/spec/diagnostics.md`, `examples/jetpack-typed/`,
`tests/jetpack.rs`. **Out of this arc** (separate, dependent): consumer-side
`env.jet` "import a lib vs install an exec" syntax; the real Jet→binary compiler.

**Larger, later (gaps #4/#5/#6), each its own multi-chunk arc:**
- #5 `System`/`Image` semantics — parsed/validated but inert; only `Env` means
  anything. Needs field checking + a realize path.
- #4 `config.jet` + the `jetos` tier — no loader, no `jetos` binary, no
  `src/jetos/`. Scale-3.
- #6 Scale-2 commands — `jet dev` / `jetpack enter` (the devshell `executable`
  packages from U10 land here).

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
