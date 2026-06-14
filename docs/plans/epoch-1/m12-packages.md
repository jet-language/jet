# M12 — Package manager

**Status:** M12.1 **verified** 2026-06-13. Manifest layout **ratified** (S52,
D-MF1…5). Architecture **ratified** (D-PM1…8, owner 2026-06-13).
M12.2 (registry, semver ranges, PubGrub resolver, jet publish/vendor/audit)
is the next milestone.
Depends on M6 (multi-file), M7 (FFI deps in manifest), M10 (std imports).
**Error codes:** E1201–E1209 (claim in docs/04 as implemented).

## Goal

Opt-in packages with zero ceremony for the single-file case (R9). Two
phases on one store layout. No install-time code execution (PM-I1).

**Vision:** Nix's engine under Cargo's steering wheel — `jet add`,
`jet.toml`, `jet.lock` on the surface; fingerprint-named immutable store
underneath. Single-file `jet run file.jet` never touches any of this.

---

## Architecture (three layers, one store)

```
LAYER 1 — M12.1 (ships first)
  jet.toml · jet.lock · jet add/remove/fetch/update/build/run/test
  path + git deps · exact pins · ~/.jet/store + hardlinks

LAYER 2 — M12.2
  git-index registry · semver ranges + PubGrub resolver · jet publish
  · --as-of · local compile-once cache · jet vendor · jet audit

LAYER 3 — post-v1 (same store/lockfile; needs ballots + docs/research/jetos.md)
  jet eval --pure recipes · sandboxed builds · signed binary caches
  · generations/rollback for installed tools · packaging non-Jet software
```

Layer 3 is **out of M12 scope**. It re-homes recipe/sandbox work onto this
store. Ordinary projects always use `jet.toml` — never a Jet-code manifest.
A user-facing second tool (e.g. typing `jetpack add`) is forbidden; layer 3
may ship an internal helper binary invoked by `jet` subcommands (D-PM4).

### Glossary

| Term | Meaning |
|---|---|
| **Manifest** | Human-owned `jet.toml` — the only package file users edit |
| **Lockfile** | Tool-owned `jet.lock` — exact graph + fingerprints; never hand-edited |
| **Fingerprint** | Hash of a package's plan (source + dependency plan hashes) |
| **Store** | `~/.jet/store/<name>-<version>-<fingerprint>/` — full fingerprint suffix; append-only, per-user |
| **Hardlink** | One on-disk copy, many project paths — "install" writes links, not copies |
| **Resolver** | Picks versions when ranges disagree; PubGrub in phase 2 |
| **Registry** | Append-only git index of published packages (phase 2) |

### Survey (what we take / refuse)

| Source | Take | Refuse |
|---|---|---|
| **Nix** | Content-addressed store, rollback idea, original+locked split | nixlang, daemon, `/nix/store`, cryptic errors |
| **Cargo** | Daily workflow, lockfile CI mode | `build.rs` / install hooks |
| **pnpm** | Global store + hardlinks into projects | node_modules layout |
| **uv** | Metadata-only resolution, readable conflicts | — |
| **Go modules** | Tree-hash verify every build (E1204), vendoring | MVS surprises |
| **Elm** | Registry enforces semver via API diff at publish | — |
| **npm** | — | install scripts, yanking deletes, duplicate versions silently |

---

## Ratified — manifest (`jet.toml`)

Hand-parsed TOML subset in the compiler (I6). Authority: S52 +
D-MF1…5 below.

```toml
[package]
name = "wordstats"
version = "0.1.0"
jet = ">=0.1.0"
description = "Count words in plain text."
license = "MIT OR Apache-2.0"
repository = "https://github.com/acme/wordstats"

[dependencies]
textkit = "1.2.0"                                              # registry (M12.2)
helpers = { path = "../helpers" }
parsekit = { git = "https://github.com/acme/parsekit", tag = "v0.4.1" }

[dependencies:rust]
base64 = "0.22"

[dependencies:c]
# reserved for v2 C FFI (S59); parse but ignore in v1

[tool.myformatter]
# ignored by Jet; third-party tools may read this
```

### Tables

| Table | Purpose | v1 |
|---|---|---|
| `[package]` | Identity + toolchain | required when manifest present |
| `[dependencies]` | Jet package deps | optional |
| `[dependencies:rust]` | M7 cargo bridge crates | optional |
| `[dependencies:c]` | Future C libs (S59) | reserved; empty OK |
| `[dev-dependencies]` | Test-only deps | reserved; error if non-empty |
| `[patch]` | Root-only overrides | reserved; error if non-empty |
| `[workspace]` | Monorepo members | reserved; error if non-empty |
| `[tool.*]` | Third-party config | ignored; warn on `[tool.jet]` |

Colon suffix on dependency kinds (`[dependencies:rust]`), not separate
top-level names like `[rust-dependencies]`.

### `[package]` fields

| Field | Required | Notes |
|---|---|---|
| `name` | yes | Package name; import root |
| `version` | yes | Semver string |
| `jet` | generated | Toolchain constraint; checked before sema |
| `description` | publish | Empty string OK in dev |
| `license` | publish | SPDX expression |
| `repository` | no | URL string |

`jet new` writes the useful template (D-MF3). `jet new --annotated` adds
commented example dependency lines (D-MF5).

### Dependency spelling (D-MF2)

Name-as-key only — no `[[dependencies]]` array tables.

**Registry (M12.2):** `textkit = "1.2.0"` or `textkit = "^1.2"`.

**Git (M12.1):** inline table with exactly one of:
`tag = "v1.2.0"`, `branch = "main"`, `rev = "abc…"`, or moving
`tag = "@latest"` / `branch = "@latest"`.

**Path (M12.1):** `helpers = { path = "../helpers" }` — relative to
manifest directory.

### Layout decisions (D-MF1…5 — ratified)

| ID | Decision | Choice |
|---|---|---|
| D-MF1 | `[package].jet` toolchain constraint | **A** — add `jet = ">=0.1.0"` |
| D-MF2 | Dependency spelling | **A** — name-as-key in tables |
| D-MF2a | Moving git refs | **allow** branch/tag/`@latest` with lock; `--locked` freezes |
| D-MF3 | `jet new` template | **useful template** — name, version, jet, description, license, repository |
| D-MF4 | Future sections | **reserve** dev-deps, patch, workspace, tool.* |
| D-MF5 | Generated comments | **minimal default**; `jet new --annotated` for teaching |

### Project layout — `.jet/` folder

When `<project-root>/.jet/` exists:

- **Project root** = directory containing `jet.toml`.
- **Source root** = `.jet/` (module search, default entry `main.jet`).
- `jet new` creates `jet.toml` + `.jet/main.jet` + `.gitignore`.

Without `.jet/`, behavior unchanged from M6 (sources beside `jet.toml`).

### Workspaces (reserved)

Parser recognizes `[workspace]` and `members = […]` but returns a clear
"not implemented yet" diagnostic if non-empty. Future: walk members,
resolve nested `jet.toml`, build in graph order.

### Manifest rules (ratified)

- `jet.toml` human-owned; `jet.lock` tool-owned.
- Generator preserves comments and ordering when editing dependencies.
- Unknown Jet-owned keys → error with did-you-mean (reserved names excepted).
- No install hooks, build scripts, or code execution in `jet.toml`.
- Published packages need `name`, `version`, `description`, `license`
  (enforced at `jet publish`, M12.2).
- Post-v1 Jet-code manifest (`.jet` project file) needs separate ballots —
  not a v1 replacement for `jet.toml`.

### Deferred manifest features

Version ranges (M12.2), dev-dependencies content, optional deps/features,
non-empty workspaces, patches. Feature policy ratified: see table in prior
research — all defer with reserved names where noted above.

---

## Lockfile (`jet.lock`)

Tool-owned TOML; never hand-edited. Schema version field mandatory.
Graph-shaped: shared deps appear once; original selector + locked identity
per node.

```toml
version = 1

[[package]]
name = "wordstats"
source = { root = "." }

[[package]]
name = "parsekit"
source = { git = "https://github.com/acme/parsekit", branch = "main" }
locked = { rev = "a8b3c5d82…", tree-hash = "sha256-…", last-modified = 1710000000 }

[[package]]
name = "textkit"
source = { git = "https://github.com/someone/textkit", tag = "@latest" }
locked = { rev = "…", tree-hash = "sha256-…", last-modified = 1710000000 }

[root]
dependencies = ["parsekit", "textkit", "helpers"]
```

Rules:

1. **Graph-shaped** — transitive edges recorded per node.
2. **Original + locked** — `source` echoes manifest selector;
   `locked` holds exact rev + tree-hash.
3. **Plan fingerprint** — each node includes dependency plan fingerprints
   (store key — D-PM1).
4. **Sorted** — stable key order for diff-friendly CI review.
5. **`--locked`** — refuse network; fail if lock ≠ manifest resolution.
6. **`jet update`** — re-resolve `@latest` and branch selectors; rewrite lock.

Tree-hash: canonical hash of the dependency source tree (document algorithm
in user guide when implemented). Verified on every fetch/build (E1204).
No Nix NAR format — documented canonical tree hash only.

---

## Store (M12.1 — D-PM1/5)

`~/.jet/store/<name>-<version>-<fingerprint>/` — append-only, per-user, no
daemon. The **full** plan fingerprint is the path suffix (same value as in
`jet.lock`); lookups use the lockfile, not dirname parsing. Hardlink into
project build dir; fallback to copy cross-device. `jet gc` removes
unreferenced entries. `jet store verify` re-checks hashes.

Example:

```
~/.jet/store/textkit-1.2.0-a7f3d2e91b4c8f1e0d9c6b5a43210fedcba9876543210abcdef/
```

Phase 1 stores **source trees** only; phase 2 adds **compiled artifact**
cache keyed by the same fingerprint.

Fingerprint covers the whole dependency ancestry: change a deep dep → new
identity for everything above it; old and new coexist; nothing mutated in
place.

---

## Ratified architecture (D-PM1…8 — owner 2026-06-13)

| ID | Decision | Ratified |
|---|---|---|
| **D-PM1** | Store architecture | **A** — Nix-style store from M12.1; path layout `<name>-<version>-<full-fingerprint>` |
| **D-PM2** | Manifest language | **A** — `jet.toml` only for deps/manifest; optional `build.jet` later (layer 3 ballot) |
| **D-PM3** | Version picking | **A** — exact pins M12.1; semver ranges + PubGrub resolver M12.2; one version per name |
| **D-PM4** | Tooling binary | **A** — all M12.1 in `jet` (I6 holds); layer 3 may add internal helper binary behind `jet` subcommands |
| **D-PM5** | Store location | **A** — `~/.jet/store`, no root, no daemon |
| **D-PM6** | Registry (M12.2) | **A** — append-only git repo; `--as-of` = older commit |
| **D-PM7** | Generations | **A** — layer 3 for global tools; projects use git on `jet.lock` |
| **D-PM8** | Cross-machine cache | **A** — local reuse M12.2; signed remote cache at layer 3 |

### D-PM1 — Store path layout

Content-addressed store with human-readable names first:

```
~/.jet/store/<name>-<version>-<full-fingerprint>/
```

The full fingerprint (plan hash from `jet.lock`) disambiguates when the same
name+version could differ. Identity for correctness is always the lockfile
entry, not parsing the directory name.

### D-PM2 — Manifest vs build files

`jet.toml` is the **only** manifest for dependencies and package identity in
all phases. A future `build.jet` (layer 3, separate ballot) may describe build
steps; it never replaces or merges with manifest duties.

### D-PM3 — Resolver phasing

M12.1: exact pins only; E1201 on version conflicts. M12.2: registry,
semver ranges, PubGrub resolver, E1207 — per Phase 2 section below.

### D-PM4 — One user-facing tool

M12.1 implements fetch/store/lock entirely inside `jet` (git subprocess, no
HTTP in compiler). Post-v1 layer 3 may ship an **internal** `jetpack` binary
in the same install bundle; `jet add` execs it, users never install or
version-match a second CLI.

---

## Invariants (PM-I1…PM-I8)

- **PM-I1** No code execution at install/fetch time (no install hooks).
- **PM-I2** Store append-only; verify fingerprint on create and on demand.
- **PM-I3** Lockfile + store (or network) reproduces exact tree byte-for-byte.
- **PM-I4** Resolution downloads metadata only; packages fetched after choosing.
- **PM-I5** Registry entries immutable; yank flags, never deletes bytes.
- **PM-I6** One mechanism per job: one manifest, one lockfile, one store layout.
- **PM-I7** Every diagnostic: E12xx code, what/why/fix, ui snapshot (I4).
- **PM-I8** R9 forever: `jet run file.jet` with no manifest works as today.

---

## Commands

| Command | Phase | Action |
|---|---|---|
| `jet new <name>` | 1 | Useful-template `jet.toml` + `main.jet` |
| `jet new <name> --annotated` | 1 | + commented example deps |
| `jet add <dep> --path …` | 1 | Edit manifest, resolve, lock, link |
| `jet add <dep> --git … --tag …` | 1 | Same |
| `jet remove <dep>` | 1 | Edit manifest, resolve, lock |
| `jet fetch` | 1 | Download/link all deps; write/verify lock |
| `jet fetch --locked` | 1 | Verify only; no network |
| `jet update` | 1 | Re-resolve `@latest`/branch selectors |
| `jet update <dep>` | 1 | Update one moving selector |
| `jet run` / `build` / `test` | 1 | Auto-detect `jet.toml` upward; else single-file |
| `jet add <dep>` | 2 | Registry lookup (no version → latest stable) |
| `jet publish` | 2 | Registry PR + semver API check |
| `jet vendor` | 2 | Copy deps into `vendor/` for air-gap |
| `jet audit` | 2 | SBOM from lock graph |

Teaching: E0042 foreign manifest filename → `jet.toml`; E0043 `jet install`
→ `jet fetch`.

---

## Import resolution with packages

```jet
import words;                         // package dep (module under store link)
import "pkg/textkit/words" as words;  // explicit path (still valid)
import scoring;                       // local module (S16)
```

Package root = directory containing that package's `jet.toml` (or its
`.jet/` source root). `pub` items only across package boundaries (S18).

Project root for module search (S16): directory containing the **root**
`jet.toml`; skips `build/`, `target/`, dot-directories.

**Ring imports (SL2, post-M10):** first-party short names (`http`, `regex`)
are reserved aliases for canonical `jet.*` packages. Version at import site
(`import http#0.8.1 as http`) resolves via manifest/lock in M12.2+.
`import jet.std as std` valid; `std#version` / `jet.std#version` not valid
in one package (toolchain-selected std).

---

## Sema / loader rules

1. **One version per package name** in the graph (E1201 with both chains).
2. **Lockfile authoritative** — mismatch → E1202 ("run `jet fetch`").
3. **Git via subprocess** — no HTTP in compiler; missing git → E1203.
4. **`[dependencies:rust]`** feeds M7 bridge; inline `@version` in
   `extern rust` when manifest exists → E1205.
5. **Toolchain** — `[package].jet` checked before sema; mismatch → E1208.
6. **Dependency diagnostics** — paths show package name
   (`textkit/words.jet:14:3`); lints suppressed outside root package.
7. **Manifest parse** — E1206 with line/column; subset = tables, strings,
   inline tables, arrays of strings for `[workspace].members`.
8. **Reserved sections** — non-empty reserved table → E1209.
9. **Unknown Jet keys** — error + did-you-mean; `[tool.*]` ignored.

---

## Phase 1 — M12.1 (implement first)

**Ships:** commands phase-1 column; store + hardlink linker; path + git deps;
exact pins + moving branch/`@latest`; lock graph v1; `.jet/` source-root;
E1201–E1206 + E1208–E1209.

**Implementation order:**

1. `src/manifest.rs` — parse/validate/serialize subset; reserved sections;
   comment-preserving edit helpers for `jet add`.
2. `src/lock.rs` — lock schema v1; fingerprint; graph; `--locked` verify.
3. `src/store.rs` — store paths, hardlink linker, hash verify, gc stub.
4. `src/fetch.rs` — git subprocess; path deps; integrate store.
5. Wire loader: project root, source-root (`.jet/`), package dep paths into S16.
6. CLI: `jet new` / `add` / `remove` / `fetch` / `update`.
7. M7 bridge reads `[dependencies:rust]` when manifest present.
8. Tests + user guide (`docs/guide/` or dedicated packages doc).

**Exit criteria:**

- `tests/pkg/` fixtures: path dep, git dep (local bare repo), lock verify,
  version conflict, cross-package private import (E0605), tamper (E1204),
  `--locked` CI mode, two projects sharing one store inode, `@latest`
  update rewrites lock, reserved section error, `[package].jet` mismatch.
- End-to-end tempdir: `jet new` → `jet add --path` → `jet run`.
- `jet new --annotated` snapshot test for generated file shape.

---

## Phase 2 — M12.2 (separate agent run)

**Ships:** append-only git registry (D-PM6); semver ranges + in-tree PubGrub
(D-PM3/4); `jet publish` with sema-enforced API diff; `jet vendor`;
`jet audit`/SBOM; `--as-of <date>`; local compile-once cache (D-PM8);
E1207 resolver conflicts; ring package version resolution (SL2).

**Exit criteria:** resolver conflict snapshot; publish refuses breaking
minor; `--as-of` reproduces old lock; air-gapped vendor build; registry
add without `--git`.

---

## Diagnostics

| Code | Meaning |
|---|---|
| E1201 | Two versions of one package required |
| E1202 | Lock out of date |
| E1203 | `git` not installed |
| E1204 | Tree-hash mismatch / tamper |
| E1205 | FFI pin belongs in `[dependencies:rust]` |
| E1206 | Manifest syntax/shape |
| E1207 | Resolver failure (phase 2) |
| E1208 | Toolchain `[package].jet` incompatible |
| E1209 | Reserved section used (not implemented) |

---

## Examples & tests

- `examples/32_packages/` — multi-file project with path dep.
- `tests/pkg/` — fixture workspaces (see Phase 1 exit).
- Ui: `tests/ui/manifest_*.jet` driven from bad toml fixtures.

---

## Out of scope (v1)

Layer 3 (recipes, sandbox, cross-machine cache, generations). Jet-code
`.jet` manifest for ordinary packages. Non-empty workspaces. Dev-dependencies
content. Features/conditional compilation. Binary dep artifacts in phase 1.
Yanking/mirrors implementation beyond spec.

**Post-v1 OS direction** (old nix-replacement / jetpack docs): declarative
OS on the same store lives in **docs/research/jetos.md** — D-NX1…6,
imperative `jetos add` editing config files, bootstrap via read-only Nix
cache tap. Not M12 work.

**Dev environment:** repo Nix flake for building Jet itself — **docs/nix.md**
(unchanged; not part of the user package manager).

---

## Ratification checklist

| Item | Status |
|---|---|
| Manifest layout (D-MF1…5, lock graph, `@latest`) | **Ratified** — S52 |
| Architecture (D-PM1…8) | **Ratified** — owner 2026-06-13 |
| Layer 3 / jetos / internal jetpack helper | **Deferred** — research/jetos.md |

**Agent checklist before M12.1:**

1. Read this file end-to-end; implement per ratified D-PM1…8.
2. Claim E1201–E1209 in docs/04 as diagnostics land.
3. Store paths: `~/.jet/store/<name>-<version>-<full-fingerprint>/`.
4. Do not implement layer-3 recipes, `package.jet` manifest, or a
   user-facing `jetpack` CLI (internal helper deferred to layer 3).
