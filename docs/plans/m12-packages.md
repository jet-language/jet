# M12 — Package manager

**Decisions:** S52 (amended 2026-06-13), manifest-design-research.md
(ratified), package-manager-decisions.md (architecture recommendations).
Depends on M6 (multi-file), M7 (FFI deps in manifest), M10 (std imports).
**Error codes:** E1201–E1207 (claim in docs/04 as implemented).

## Goal

Opt-in packages with zero ceremony for the single-file case (R9). Two
phases on one store layout — see docs/package-manager-decisions.md §6.
No install-time code execution (PM-I1).

## Blocked on decisions

All manifest layout decisions are **ratified** (manifest-design-research.md,
S52 amendment 2026-06-13). Architecture decisions D-PM1…D-PM8 follow the
recommendations in package-manager-decisions.md unless the owner overrides
before implementation starts.

---

## Manifest (`jet.toml`)

Hand-parsed TOML subset in the compiler (I6). Ratified shape:

```toml
[package]
name = "wordstats"
version = "0.1.0"
jet = ">=0.1.0"
description = ""
license = ""
repository = ""

[dependencies]
textkit = { git = "https://github.com/someone/textkit", tag = "v1.2.0" }
helpers = { path = "../helpers" }
parsekit = { git = "https://github.com/acme/parsekit", branch = "main" }

[dependencies:rust]
base64 = "0.22"

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

### `[package]` fields

| Field | Required | Notes |
|---|---|---|
| `name` | yes | Package name; import root |
| `version` | yes | Semver string |
| `jet` | generated | Toolchain constraint; checked before sema |
| `description` | publish | Empty string OK in dev |
| `license` | publish | SPDX expression or use `license-file` later |
| `repository` | no | URL string |

`jet new` writes the useful template (D-MF3). `jet new --annotated` adds
commented example dependency lines (D-MF5).

### Dependency spelling (D-MF2)

Name-as-key only — no `[[dependencies]]` array tables in the manifest.

**Registry (M12.2):** `textkit = "1.2.0"` or `textkit = "^1.2"`.

**Git (M12.1):** inline table with exactly one of:
`tag = "v1.2.0"`, `branch = "main"`, `rev = "abc…"`, or moving
`tag = "@latest"` / `branch = "@latest"`.

**Path (M12.1):** `helpers = { path = "../helpers" }` — relative to
manifest directory.

### Project layout — `.jet/` folder

When `<project-root>/.jet/` exists:

- **Project root** = directory containing `jet.toml`.
- **Source root** = `.jet/` (module search, default entry `main.jet`).
- `jet new --with-dot-jet` (or default once convention stabilizes):
  creates `jet.toml` + `.jet/main.jet` + `.gitignore`.

Without `.jet/`, behavior is unchanged from M6 (sources beside `jet.toml`).

### Workspaces & sub-packages (reserved)

Parser recognizes `[workspace]` and `members = […]` but returns a clear
"not implemented yet" diagnostic if non-empty. No code generation. Future
implementation walks members, resolves nested `jet.toml` files, builds in
graph order. See manifest-design-research.md §2.

---

## Lockfile (`jet.lock`)

Tool-owned TOML; never hand-edited. Schema version field mandatory.

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

1. **Graph-shaped** — shared deps appear once; `[root]` or equivalent lists
   direct deps; transitive edges recorded per node.
2. **Original + locked** — `source` echoes the manifest selector;
   `locked` holds exact rev + tree-hash.
3. **Plan fingerprint** — each node's fingerprint includes dependency
   plan fingerprints (store key — D-PM1).
4. **Sorted** — stable key order for diff-friendly CI review.
5. **`--locked`** — refuse network; fail if lock ≠ manifest resolution.
6. **`jet update`** — re-resolve `@latest` and branch selectors; rewrite
   lock; optionally bump pins interactively in phase 2.

Tree-hash: canonical hash of the dependency source tree (document algorithm
in docs/07-packages.md when implemented). Verified on every fetch/build
(E1204).

---

## Store (M12.1 — D-PM1/5)

`~/.jet/store/<fingerprint>-<name>-<version>/` — append-only, per-user, no
daemon. Hardlink into project build dir; fallback to copy cross-device.
`jet gc` removes unreferenced store entries. `jet store verify` re-checks
hashes.

Phase 1 stores **source trees** only; phase 2 adds **compiled artifact**
cache keyed by the same fingerprint.

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

---

## Rules

1. **One version per package name** in the graph (E1201 with both chains).
2. **Lockfile authoritative** — mismatch → E1202 ("run `jet fetch`").
3. **Git via subprocess** — no HTTP in compiler; missing git → E1203.
4. **`[dependencies:rust]`** feeds M7 bridge; inline `@version` in
   `extern rust` when manifest exists → E1205.
5. **Toolchain** — `[package].jet` checked before sema; mismatch → new
   E1208 (register in docs/04).
6. **Dependency diagnostics** — paths show package name
   (`textkit/words.jet:14:3`); lints suppressed outside root package.
7. **Manifest parse** — E1206 with line/column; subset = tables, strings,
   inline tables, arrays of strings for `[workspace].members`.
8. **Reserved sections** — non-empty reserved table → E1209 with fix
   ("not implemented in v1").
9. **Unknown Jet keys** — error + did-you-mean; `[tool.*]` ignored.

---

## Phase 1 — M12.1 (implement first)

**Ships:** everything in Commands phase-1 column; store + hardlink linker;
path + git deps; exact pins + moving branch/@latest; lock graph v1;
`.jet/` source-root convention; E1201–E1206 + E1208–E1209.

**Implementation order (suggested):**

1. `src/manifest.rs` — parse/validate/serialize `jet.toml` subset; reserved
   section diagnostics; comment-preserving edit helpers for `jet add`.
2. `src/lock.rs` — lock schema v1; read/write; fingerprint computation;
   graph serialization; `--locked` verification.
3. `src/store.rs` — store paths, hardlink linker, hash verify, gc stub.
4. `src/fetch.rs` — git subprocess fetch; path deps; integrate store.
5. Wire loader: project root detection, source-root (`.jet/`), package
   dep paths into S16 module search.
6. `jet new` / `jet add` / `jet remove` / `jet fetch` / `jet update` CLI.
7. M7 bridge reads `[dependencies:rust]` when manifest present.
8. Tests + docs/07-packages.md user guide.

**Exit criteria:**

- `tests/pkg/` fixtures: path dep, git dep (local bare repo), lock verify,
  version conflict, cross-package private import (E0605), tamper (E1204),
  `--locked` CI mode, two projects sharing one store inode, `@latest`
  update rewrites lock, reserved section error, `[package].jet` mismatch.
- End-to-end tempdir: `jet new` → `jet add --path` → `jet run`.
- `jet new --annotated` snapshot test for generated file shape.

---

## Phase 2 — M12.2 (separate agent run)

**Ships:** append-only git registry (D-PM6); semver ranges + in-tree
PubGrub resolver (D-PM3/4); `jet publish` with sema-enforced API diff;
`jet vendor`; `jet audit`/SBOM; `--as-of <date>`; local compile-once cache
(D-PM8); E1207 resolver conflicts.

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

- `examples/32_packages/` — multi-file project with path dep (slot per
  README numbering).
- `tests/pkg/` — fixture workspaces (see Phase 1 exit).
- Ui: `tests/ui/manifest_*.jet` for diagnostics driven from bad toml
  fixtures in `tests/ui/manifest_*` directories.

---

## Out of scope (v1)

Layer 3 (recipes, sandbox, cross-machine cache, generations) — post-v1 per
package-manager-decisions.md. Jet-code `.jet` manifest file — post-v1
ballot. Non-empty workspaces. Dev-dependencies content. Features/
conditional compilation. Binary dep artifacts in phase 1. Yanking/mirrors.
