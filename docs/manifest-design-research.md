# Jet manifest design — ratified

**Status:** ratified 2026-06-13 (owner). Supersedes the draft decision aid.
**Authority:** manifest format and layout rules live here and in **S52**
(docs/02-syntax-decisions.md). Package-manager architecture (store,
registry, phasing) lives in docs/package-manager-decisions.md.
**Implement from:** docs/plans/m12-packages.md.

Single-file `jet run file.jet` stays manifest-free forever (R9).

---

## 1. Manifest shape

Human-owned `jet.toml`; every resolved detail in generated `jet.lock`.

```toml
[package]
name = "wordstats"
version = "0.1.0"
jet = ">=0.1.0"
description = "Count words in plain text."
license = "MIT OR Apache-2.0"
repository = "https://github.com/acme/wordstats"

[dependencies]                 # Jet packages
textkit = "1.2.0"
helpers = { path = "../helpers" }
parsekit = { git = "https://github.com/acme/parsekit", tag = "v0.4.1" }

[dependencies:rust]            # M7 FFI pins (when a manifest exists)
base64 = "0.22"

[dependencies:c]               # reserved for v2 C FFI (S59); parse but ignore in v1
# (empty)
```

**Ratified:** accept this skeleton. Dependency kinds use a **colon suffix**
on the table name — `[dependencies]`, `[dependencies:rust]`,
`[dependencies:c]` — not separate top-level tables like
`[rust-dependencies]`. Extensible for future FFI backends without reserving
more top-level names.

Field order is stable: `[package]`, `[dependencies]`, then
`[dependencies:*]` in alphabetical suffix order, then reserved future
sections.

What we copy: TOML readability; metadata-first layout; lockfile for the
exact graph; Nix-style original-vs-locked split.

What we reject: install hooks; XML/code manifests in v1; one file that
mixes manifest and lock; Nix language in the user manifest.

---

## 2. Format evolution & project layout

**Ratified:** declarative TOML only for the v1 manifest (`jet.toml`).

**Staged (post-v1, needs ballots before syntax):** a Jet-code manifest
(`.jet` project file) for enterprise/complex projects — same role as
layer-3 recipes in docs/package-manager-decisions.md, not a replacement
for ordinary packages.

**Ratified — `.jet/` source folder:** when a project root contains a
`.jet/` directory, Jet treats **`.jet/` as the source root** for module
search (S16) and the default location for `main.jet`. `jet.toml` always
lives at the **project root** (parent of `.jet/`), never inside `.jet/`.
`jet new` creates `<name>/jet.toml` + `<name>/.jet/main.jet` when this
convention is enabled (see m12 plan).

**Ratified — sub-packages (reserved, not generated in v1):** reserve
`[workspace]` and per-member tables so monorepos can be declared without
local ad-hoc conventions:

```toml
[workspace]
members = ["helpers", "tools/lint"]

[package]
name = "wordstats"
version = "0.1.0"
```

Discovery rule (when implemented): `jet build` / `jet fetch` walks
`[workspace].members`, resolves each member's `jet.toml`, and builds in
dependency order. Phase 1 does not generate or resolve workspaces; the
parser rejects unknown Jet-owned keys but **recognizes and reserves**
`[workspace]` and member paths (D-MF4).

---

## 3. Lockfile (Nix flake lessons)

**Ratified:** graph lock with **original + locked** pairs for every node;
schema version from day one; canonical source-tree hash verified on every
build; provenance timestamp for display only.

Human-owned moving selector (`jet.toml`):

```toml
[dependencies]
parsekit = { git = "https://github.com/acme/parsekit", branch = "main" }
textkit = { git = "https://github.com/acme/textkit", tag = "@latest" }
```

Generated lock-owned (`jet.lock`):

```toml
version = 1

[[package]]
name = "parsekit"
source = { git = "https://github.com/acme/parsekit", branch = "main" }
locked = { rev = "a8b3c5d82…", tree-hash = "sha256-…", last-modified = 1710000000 }

[[package]]
name = "textkit"
source = { git = "https://github.com/acme/textkit", tag = "@latest" }
locked = { rev = "…", tree-hash = "sha256-…", last-modified = 1710000000 }
```

**Ratified — `@latest` / moving selectors:** manifest may use `@latest`
(or `#latest` on git refs — both spellings accepted, `@latest` is
canonical) as a **moving selector**. `jet update` re-resolves moving
selectors and rewrites the lock; `jet fetch --locked` refuses to change
them. Exact pins, path deps, and locked revs are unchanged by `--locked`.

Do not put Nix `outputs = …` code, system matrices, or NAR format in
`jet.toml`. Jet uses a documented canonical tree hash (not NAR).

---

## 4. Feature policy (ratified)

| Feature | Jet call |
|---|---|
| Comments | Keep; generator preserves user comments and ordering |
| Lockfiles | Generated `jet.lock`; sorted, graph-shaped, diff-stable |
| Version ranges | v1 exact pins/path/git; ranges in M12.2 with clear resolver errors |
| Dev/test deps | Defer until package tests + publish |
| Optional deps/features | Defer; explicit named variants later |
| Overrides/patches | Root-only `[patch]` later; never from dependencies |
| Workspaces | Defer; reserve `[workspace]` |
| Tool config | Reserve `[tool.*]`; Jet ignores except warn on `[tool.jet]` |
| Build/install hooks | Never in package manifests; future recipes are pure/sandboxed |
| Unknown fields | Error on unknown Jet fields; allow only `[tool.*]` |

---

## 5. Layout decisions (D-MF1…D-MF5)

| ID | Decision | Ratified |
|---|---|---|
| D-MF1 | `[package].jet` toolchain constraint | **A** — add `jet = ">=0.1.0"` |
| D-MF2 | Dependency spelling | **A** — name-as-key in tables |
| D-MF2a | Moving git refs | **allow branch/tag/@latest with lock**; `--locked` freezes |
| D-MF3 | `jet new` template | **useful template** — name, version, jet, description, license, repository |
| D-MF4 | Future sections | **reserve** `[dev-dependencies]`, `[patch]`, `[workspace]`, `[tool.*]` |
| D-MF5 | Generated comments | **minimal default**; `jet new --annotated` for teaching |

---

## 6. Rules (ratified)

- `jet.toml` is human-owned; `jet.lock` is tool-owned.
- `jet.lock` records both the manifest selector and the exact locked
  identity for every dependency in the graph.
- `jet.lock` is graph-shaped, schema-versioned, sorted, and
  content-hash verified.
- The generator preserves comments and ordering when editing dependencies.
- Unknown Jet-owned sections → error with "did you mean …" (reserved names
  excepted).
- `[tool.*]` is allowed but ignored; `[tool.jet]` is reserved.
- No install hooks, postinstall hooks, build scripts, shell commands, or
  code execution in `jet.toml`.
- Published packages must include `name`, `version`, `description`, and
  `license` or `license-file` (enforced at `jet publish`, M12.2).

---

## 7. Sources

See original survey links in git history. Local Nix reference:
`flake.nix`, `flake.lock`.
