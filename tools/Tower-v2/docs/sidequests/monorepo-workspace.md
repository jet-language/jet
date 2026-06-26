# Monorepo: `workspace.jet` index + package addressing (c156)

**Status:** Ratified, not implemented. Gated on the computed-modules / comptime-block work (D-CTMARKER1=C, c155-adjacent). Build plan only — do not implement until the gate lands and the workspace keyword/filename ballot (Owner Q4) is confirmed.

## Goal

One grammar for the whole project. Replace the root `jetpack.toml` monorepo index with a Jet `module workspace` (in `workspace.jet`, parallel to `env.jet`/`config.jet`) whose `members:` field may run arbitrary `comptime`, and let any package address another by `source.package` (dot) / `infra/logging` (path) / bare sugar — resolved index-first with a sparse subtree fetch so you can pull one package out of a 40-package repo without cloning the rest.

## Current state (verified, file:line)

**Module surface (the `module <name> { … }` typed config syntax):**
- Keyword `module` = U3 at `Source/Syntax.rs:935-936`; reserved namespaces `env`/`system`/`image` at `Syntax.rs:954-957`; field names `sources`/`imports` = U8 at `Syntax.rs:1031-1032`; filenames `ENV_FILE="env.jet"` / `CONFIG_FILE="config.jet"` at `Syntax.rs:1001-1002`. No `workspace` keyword or `workspace.jet` anywhere.
- Item dispatch: `Source/Parser/Items.rs:377-380` routes `KwModule` to `code_module` vs `module_decl()` via `is_code_module_at` (`Source/Parser/Modules.rs:61-116`, syntactic — `sources`/`imports`/`ident .` → typed module).
- Typed-module parser: `module_decl()` at `Source/Parser/Modules.rs:7-50`; sub-parsers `module_sources()` `:248-293`, `module_import()` `:297-305` (only `imports: find(...)`), `contribution()` `:310-358`.
- AST: `ModuleDecl` (`Source/AST.rs:581`) holds `sources: Vec<SourceDecl>`, `imports: Vec<Expr>`, `contributions: Vec<Contribution>`; `SourceDecl` `:602`, `Contribution` `:614`, `ContribValue` `:629`. **No `members:` field exists today.**
- File dispatch is at the Jetpack/CLI layer, not the parser: `Source/Jetpack/CLI.rs:280-307` (`load_project_plan` reads `env.jet`) → `ModuleEval::is_module_surface` (`Source/Jetpack/ModuleEval/Source.rs:27-39`) → `evaluate_env` (`Source.rs:46`). `config.jet` (system/image) takes the *same* `evaluate_env` path at `Source/Jetpack/JetOS.rs:80-94`.

**Current `jetpack.toml` monorepo index:**
- Parser `Source/Jetpack/ManifestTOML.rs`: `JetpackToml { repo, sources, packages }` at `:37-46`; recognized tables only `[repo]`/`[sources]`/`[packages]` (`:99-103`, `:168-181`); `load(dir)` `:279-283`. `[workspace]`/`[monorepo]` are rejected as unknown tables (E1215, test `:402-409`).
- **The `[packages]` index is parsed but never consumed by resolution.** The only caller, `load_toml_sources` (`Source/Jetpack/CLI.rs:113-139`), reads `manifest.sources` only. Discovery happens independently by module-name scan: `Source/Jetpack/PackageManifest/Discovery.rs:27-53` (`discover_module_in` walks `.jet` files for `module <name>`).
- `workspace` is also reserved-and-rejected at the `pkg.jet` manifest layer: `Source/Jetpack/PackageManifest/mod.rs:191-192` (E1209).

**Resolver / provider:**
- `Source/Jetpack/RefSpec.rs`: ref classifiers `classify_in` `:196-221` (CLI `source:package` colon form) and `classify_provider_ref` `:250-274` (authoring `provider@target`); `SourceTable` `:104-152`; built-in source labels `nixpkgs`/`github`/`path` `:46-53`; `ProviderKind::{Nix,Core,Infer}` `:65-93`.
- `Source/Jetpack/Provider.rs`: `Provider` trait `:98-109`; `NixProvider` `:114-135`; `CoreProvider` `:143-232`; entry `realize()` `:575-578` → `resolve_kind()` `:542-560` (U9 inference via `infer_remote_kind` `:593-608`). CLI orchestration `realize_ref` at `CLI.rs:172-233`.
- **Resolution is NOT index-first today, and fetch is a FULL clone:** `fetch_remote_repo` (`Provider.rs:399-403` clone, `:418-438` checkout) clones the whole repo, then `discover_module_in` walks all `.jet` (`:159-181`). The only sparse codepath is the kind-probe `remote_has_pack_jet` (`Provider.rs:628-678`: `git init` + `git fetch --depth 1 --filter=tree:0` + `git ls-tree` — peek only, no checkout). This is the seed for the sparse-fetch work.
- Dot-form package sugar already parsed for env package lists: `default.ripgrep` / `unstable.neovim` in `Source/Jetpack/Merge.rs` (`parse_package_list`, ~`:259+`). The monorepo `mono.ranker` / `infra/logging` forms are **not** implemented.

**Lock handling (two systems):**
- Compiler dep lock `.jet/lock`: `Source/Lock.rs` — `LockFile`/`LockedPackage`/`LockSource{Root,Path,Git}` `:18-55`; `write` `:61-115`, `parse` `:125`, `load` `:302`, `--locked` verify `:310-345`, fingerprint `:373-422`. Const `UNIFIED_LOCK_FILE=".jet/lock"` `Syntax.rs:1121`; read by `Source/Loader.rs:147-159`. `LockSource::Git{url,selector}` has no subtree notion.
- Jetpack store/hangar: `Source/Jetpack/Store.rs` — `lock_path` `:47-49`, `managed_dir` (`.jet/`) `:42-44`, hangar `:51-96`, `StoreEntry` `:99-163`, `entry_id` (name-version-fingerprint) `:131-139`.

**Comptime surface (the gate dependency):**
- `Source/Comptime/`: entry `evaluate`/`evaluate_with_imports` (`mod.rs:48-80`) = `check_purity` + `Interp::eval`; expr dispatch `Interpreter.rs:564`; call dispatch `Methods.rs:104`. Purity walk `Purity.rs:15` → E0951.
- D-CTCORE1 pure-Core whitelist enforced by `apply_core_call` (`Methods.rs:388-491`): `core.math` `:401-434`, `core.string` `:436-472`, IO modules → E0958 `:474-484`, else E0956. Docs: `docs/spec/syntax-decisions.md:2332-2336`, diagnostic `docs/spec/diagnostics.md:300`.
- **What exists today: comptime *bindings* and `comptime if` only** (wired into sema at `CheckerCore.rs:1622` and `:2267` via `evaluate_owned_with_imports`). There is **no `comptime { … }` execution block** and **no general expression evaluation inside module fields** — module fields are parsed as typed contributions or `find(...)` only.
- `find()` is the **U4 import-discovery directive, not a comptime builtin** — implemented in module-eval: `Source/Jetpack/ModuleEval/mod.rs:331` (E0969 if not `find("<lit>")`), liftability E0971 `:562-571`, purity walk via `crate::Comptime::walk_calls` at `ModuleEval/Eval.rs:134`. So the restricted `members: find("./packages")` case is already expressible with existing machinery; arbitrary `comptime gen()` is not.

## Decision (ratified, verbatim references)

- **D-WORKSPACE1=B** (`docs/spec/syntax-decisions.md:2947`): retire the root `jetpack.toml` monorepo index for a fully-computable `module workspace` in `workspace.jet`; `members:` may run arbitrary `comptime` (e.g. `find("./packages") + comptime gen()`). Trade accepted: external tools must evaluate Jet — mitigated by a resolver-emitted generated lock for the common case. Keyword/filename confirmed at build (Owner Q4).
- **D-MONOREF1=A** (`docs/spec/syntax-decisions.md:2943`): address a named-source member as `source.package` (dot, `mono.ranker`); an in-repo sibling as path-style `infra/logging` with bare `logging` as sugar when unambiguous. Resolution index-first: fetch the source's manifest only, then sparse-fetch just that package's subtree + transitive in-repo deps; full-clone fallback when the provider lacks sparse checkout.

Card: `tools/Tower/board.json:526-539`.

## Implementation (staged)

Each stage is independently testable; later stages assume earlier ones. New diagnostic codes start at **E1219** (E1218 is the last used jetpack code; verify free at build time).

### Stage 0 — gate confirmation (no code)
Confirm the workspace keyword + filename (Open Owner-Q below). Until then use the placeholder "the confirmed workspace keyword/filename" throughout; do not bake `module workspace`/`workspace.jet` into `Syntax.rs`.

### Stage 1 — `workspace.jet` parsing + `members:` comptime evaluation
- Add the confirmed keyword constant + filename to `Source/Syntax.rs` (decision id D-WORKSPACE1), next to U3/U8 entries (`Syntax.rs:935-1002`).
- Extend the typed-module surface to recognise a workspace block. Two shapes to settle (see Open Owner-Q): the simplest is a new namespace handled in `contribution()` / `module_decl()` (`Source/Parser/Modules.rs:7-50`) producing a `members:` field whose value is a general `Expr` (list of package addresses). Add a `members: Vec<Expr>` (or a `WorkspaceDecl`) to `Source/AST.rs` near `ModuleDecl:581`.
- File dispatch: teach the loader to read the workspace file alongside `env.jet` (`Source/Jetpack/CLI.rs:280-307`) and route it through module-eval (`ModuleEval/Source.rs:27-46`).
- Evaluate `members:` through comptime. Common case `find("./packages")` reuses the U4 directive machinery (`ModuleEval/mod.rs:331`, `Eval.rs:134`). Full power (`+ comptime gen()`) requires the comptime-block work — see Sequencing. The result is a list of resolved package roots (paths to `pkg.jet` / `module <name>` dirs), validated by the existing module-name discovery (`Discovery.rs:27-53`).
- Diagnostics: E1219 workspace `members:` value isn't a list of package addresses; E1220 a member path/glob resolves to no package (no `module <name>` / `pkg.jet`). Snapshots in `tests/ui_*`.

### Stage 2 — index + addressing (dot / path / bare sugar + ambiguity)
- Build the in-memory workspace index from Stage 1's member list: `{ package-name → root, source-name → members }`.
- Extend ref classification (`Source/Jetpack/RefSpec.rs:196-274`) to parse the three D-MONOREF1 forms:
  - `source.package` (dot) — named-source member, mirrors `default.ripgrep` sugar already in `Merge.rs`.
  - `infra/logging` (path) — in-repo sibling by repo-relative path.
  - bare `logging` — sugar; resolve against the index when exactly one match.
- Ambiguity diagnostic: E1221 bare package name matches more than one member (list candidates, suggest the path or dotted form). Unknown member: E1222 (did-you-mean over index names).
- Keep `provider@target` (U6) and CLI `source:package` (D-JPK7) untouched — index addressing is additive (I8: dot/path/bare are sugar over the same resolved root, not a second mechanism).

### Stage 3 — index-first resolution + sparse subtree fetch + transitive in-repo deps
- Make resolution index-first in `Provider.rs` (`realize`/`resolve_kind` `:542-578`): for a monorepo source, fetch only the source manifest first, read its package index, then materialize just the named package.
- Sparse fetch: generalise the `remote_has_pack_jet` probe (`Provider.rs:628-678`) into a real sparse checkout — `git init` + `--filter=tree:0` + `git sparse-checkout set <subtree>` for the package subtree and its transitive in-repo deps (walk the workspace index, not the whole tree).
- Full-clone fallback: when the provider lacks sparse/partial-clone support, fall back to today's `fetch_remote_repo` (`:399-438`) and select the subtree locally.
- Transitive in-repo deps: resolve each member's `pkg.jet`/`module` deps against the same workspace index; pull only the reachable subtrees.
- Diagnostics: E1223 sparse fetch failed and full-clone fallback also failed (provider/network); E1224 a transitive in-repo dep names a member outside the workspace.

### Stage 4 — generated lock (the static-readable mitigation)
- Emit a generated lock capturing the evaluated workspace layout (member name → resolved root → pinned rev/tree-hash), so external tools and `--locked` builds need not re-evaluate Jet. Extend `Source/Lock.rs` (`LockSource`, `:18-55`) with a subtree/member selector; reuse `Store.rs` entry fingerprints (`:131-139`).
- Filename: settle in Open Owner-Q (rec A had `.jet/workspace.lock`; the unified `.jet/lock` may absorb it). Wire read into `Loader.rs:147-159` and `--locked` verify (`Lock.rs:310-345`).

### Stage 5 — migration from `jetpack.toml` monorepo index
- The `[packages]`/monorepo role of `jetpack.toml` moves entirely to `workspace.jet`. Since `[packages]` is currently parsed-but-unconsumed (`CLI.rs:113-139` ignores it), removing the index role is low-risk.
- Add a teaching diagnostic E1225: a project carries a `jetpack.toml` `[packages]` table — point to `workspace.jet` and show the equivalent `members:`. Keep `[repo]`/`[sources]` handling per their own decisions (sources are also moving per the env.jet consolidation — coordinate with the pack.jet/env.jet sequencing memo; do not double-home `sources:`).
- Update `examples/jetpack-mono/` to the new surface.

### Stage 6 — diagnostics, example, golden, tests, docs
- Diagnostics: register E1219–E1225 in `docs/spec/diagnostics.md` (table + what/why/fix rows), each with a `tests/ui_*` snapshot (I4). Bless via `nix develop -c env UPDATE_EXPECT=1 cargo test`.
- Example (I5): a runnable monorepo under `examples/` — a `workspace.jet` with `members: find("./packages")`, ≥2 member packages where one addresses another by `source.package` and a third by path; `examples/.../expected/*.out` golden output (a build/resolve that materializes only the addressed subtree).
- Tests: parser unit tests in `Source/Parser/Modules.rs` test block; ref-classifier tests in `RefSpec.rs`; resolver/sparse-fetch tests; formatter round-trip + a `fmt` STABILITY test for the new `workspace`/`members:` syntax (per the formatter-round-trip rule — idempotence alone is insufficient).
- Docs: `docs/spec/spec.md` (workspace surface + addressing semantics), `docs/spec/syntax-decisions.md` (move D-WORKSPACE1/D-MONOREF1 to implemented, log the confirmed keyword/filename, add Syntax.rs entries per I7).

## Sequencing / gates

1. **Hard gate — comptime execution block.** D-WORKSPACE1=B's "arbitrary comptime in `members:`" requires evaluating general expressions inside a module field. Today comptime is bindings + `comptime if` only; there is no `comptime { … }` block and no field-expression evaluation. That capability is **D-CTMARKER1=C** (`$` splice + `comptime { }` block — ratified, not implemented, `tools/Tower/board.json:624-629`, downstream of c155), plus the effect boundary **D-CTEFFECT1=A** (`find`/`fetch`/`@embed` as Tier-1 hashed-reproducible effects, `board.json:545-549`). **Build c155/D-CTMARKER1 + D-CTEFFECT1 first.**
2. **Soft path — ship the restricted common case early.** `members: find("./packages")` needs only the existing U4 `find()` directive + Stages 2-4, no comptime block. If the gate slips, Stages 1-5 can land for the `find()`-only common case and accept arbitrary-comptime members once the block lands. (Do not advertise this as the final surface; it is the same syntax with a narrower evaluator.)
3. **Confirm keyword/filename (Owner Q4) before any `Syntax.rs` change** — Stage 0.
4. **Coordinate with the env.jet/sources consolidation** (pack.jet→env.jet memo): D-WORKSPACE1 ends the `sources:` double-home; do not implement a second sources home in `workspace.jet`. Sequence after the sources move settles.
5. Stages 1→2→3→4 are strictly ordered (index feeds addressing feeds sparse fetch feeds lock). Stage 5 migration and Stage 6 docs/tests close out.

## Open Owner-Q

### Q1 — Workspace keyword + filename (D-WORKSPACE1 Owner Q4, on a separate ballot)
The board's working name is `module workspace` / `workspace.jet`; not yet ratified. Candidate menu (keyword → filename), aviation/jet themed, original:

| Keyword | Filename | Read |
|---|---|---|
| `module workspace` | `workspace.jet` | plain, matches industry term, longest/most generic |
| `module fleet` | `fleet.jet` | a set of aircraft = a set of packages; on-theme, short |
| `module hangar` | `hangar.jet` | **collision** — "hangar" already names the store in `Store.rs`; avoid |
| `module manifest` | `manifest.jet` | descriptive but overloaded vs `pkg.jet` manifest |
| `module roster` | `roster.jet` | the list of members; reads as an index |
| `module wing` | `wing.jet` | a wing = a formation of aircraft; short, distinctive |

**Recommendation:** `module fleet` / `fleet.jet` — on-theme with jet/jetpack/jetos canon, short, and unambiguous against `pkg.jet`/`env.jet`/`config.jet`; "the fleet" reads naturally as "all the packages in this repo." Falls back cleanly to the generic `module workspace`/`workspace.jet` if the owner prefers the industry term. (Reject `hangar` — name collision with the store.)

### Q2 — `members:` value grammar / workspace record type
The decision names the field `members:` but not its value shape or whether the workspace block carries a typed record (analogous to `Env`/`System`/`Image`). Unsettled:
- (a) `members:` is a bare list expression evaluated at comptime → `[PackagePath]` (simplest; mirrors `imports: find(...)`).
- (b) a typed `Workspace { members: […], … }` record (room for future workspace-level fields: shared deps, default profile).

**Recommendation:** start with (a) — a single comptime-evaluated list field, no record wrapper — to keep the default surface minimal (I8); promote to (b) only if a concrete second workspace-level field is needed. Flagged because (b) would add a reserved type name to `Syntax.rs` that should not be guessed.

### Q3 — Generated-lock filename
Rec A proposed `.jet/workspace.lock`; the owner chose B but kept "a generated lock for the common case." Open: dedicated `.jet/workspace.lock` vs folding member resolution into the existing unified `.jet/lock` (`Syntax.rs:1121`).

**Recommendation:** fold into the unified `.jet/lock` (one lockfile, one read path in `Loader.rs:147-159`) with a `workspace`/member section, unless the owner wants a separately-committable workspace layout file for external tools. Flagged because it adds a user-visible file/path either way.
