# The Jet ecosystem — unified design-of-record

**Status:** owner-ratified vision, 2026-06-16. This is the **canonical
authoring-surface and filesystem model** for `jet` + `jetpack` + `jetos`. It
**supersedes** the pack-file surface in this folder's `README.md` (§3.3) and the
earlier `pack-abi.md`, **revises** D-JPK3/8/13 (file roles/names), and
**influences** `jetos-design.md` (module shape) and the Jet package manifest
(S52). Phase-1 directive scanning stays as the shippable bootstrap; this is the
target surface it evolves into.

All names below are **ratified** (see the Naming ledger). Open items are the
**U-series** at the end.

---

## 1. The three tools — roles and the one-way arrow

| Tool | What it is | Standalone? |
|---|---|---|
| **jet** | the language + compiler. Runs code. *A file is a complete program.* Knows nothing about jetpack. | ✅ fully usable alone |
| **jetpack** | the declarative **engine**: reads the manifests, resolves sources, realizes the **hangar** store, discovers + merges the module tree, builds environments. It *evaluates* Jet files via pure eval. | needs jet |
| **jetos** | a **consumer** of jetpack: adds whole-machine module namespaces (`system`/`image`) + activation/generations. | needs jetpack |

**Dependency arrow is strictly one-way: `jetos → jetpack → jet`.** jet never
depends on jetpack; jetpack never depends on jetos. This is what keeps "jet
usable on its own" true forever.

## 2. Jetpack file structure (Phase 1)

**Ratified structure (2026-06-18):**

| File | Location | Holds | Checked In? |
|---|---|---|---|
| **`jetpack.toml`** | repo root | Monorepo manifest: sources, package index | Yes |
| **`env.jet`** | repo root | Dev environment: sources, packages, shell config | Yes |
| **`pkg.jet`** | each package dir (user-chosen location) | Package identity: name, version, kind | Yes |
| **`.jet/lock`** | `.jet/` folder | Generated lockfile (resolved deps + fingerprints) | No |
| **`.jet/cache/`** | `.jet/` folder | Generated build cache | No |

A monorepo has one `jetpack.toml` (root) + one `env.jet` (root) + multiple `pkg.jet` 
files (one per package, in user-organized locations). Single-file packages omit 
`jetpack.toml` if they declare only themselves.

### 2.1 `jetpack.toml` — monorepo manifest (Cargo.toml analog)

TOML format. Lives at repo root. Declares sources and package index.

```toml
[repo]
name = "jetpack-core"
version = "1.0.0"

[sources]
stable = "github:NixOS/nixpkgs#nixos-24.05"
unstable = "github:NixOS/nixpkgs#nixpkgs-unstable"
mine = "path:."

[packages]
util = "util/pkg.jet"
http = "http/pkg.jet"
```

### 2.1a `pkg.jet` — package definition (one per package)

Jet syntax. Lives in each package's root directory (user-chosen location). 
One file per publishable package.

```jet
payload: {
    name:    "util",
    version: "1.0.0",
}
packages: {
    core: library,
}
```

### 2.2 `env.jet` — the project environment (devenv analog)

Jet code. Lives at repo root. Declares dev sources and packages for the environment.

```jet
use jetpack as pkg;

pub fn shell() -> [JSON] {
    return [
        pkg.source("stable", "github:NixOS/nixpkgs#nixos-24.05"),
        pkg.source("unstable", "github:NixOS/nixpkgs#nixpkgs-unstable"),
        pkg.source("mine", "path:.", "core"),
        pkg.packages(["mine:util", "mine:http"]),
        pkg.prompt("jetpack"),
    ];
}
```

`jetpack enter` reads `env.jet` and activates the shell. Edit `env.jet` to add/remove 
dev tools or change sources. The sources declared here reference `jetpack.toml` 
definitions by name (or can be inline).

### 2.3 Future: `config.jet` — jetos integration (deferred)

Deferred to Epoch 3. Not part of Phase 1 jetpack design.
}

// module _gaming { … }   // disabled: leading underscore → not discovered
```

## 3. Filesystem & store — tidy by default, one store, never relocated

- **One global store: `/etc/jet/hangar/`.** No per-user store. Content-addressed,
  deduplicated, and a clean backup target.
- **Your config is never force-moved.** Unlike NixOS's `/etc/nixos/`, the
  authored files live wherever you keep them — a project repo, or `~/.jet/` for
  the system tier (the default, also just a normal git repo you back up to
  GitHub). jetpack/jetos *point at* a config path; they never relocate it.
- **`.jet/` is the project-local managed folder** — lockfile, caches, GC roots.
  Never hand-edited. The realized packages are **not** here; they live in the
  shared hangar.

```
# A project (commits cleanly to GitHub):
wordstats/
  pkg.jet  env.jet
  modules/tools/lint.jet        # auto-discovered by find()
  src/ main.jet
  .jet/
    lock                        # the ONLY lockfile
    cache/  gcroots/

# The machine config (default ~/.jet/, also a git repo you back up):
~/.jet/
  config.jet
  modules/{apps,hosts,services}/
  .jet/lock

# The one shared store (backup target):
/etc/jet/hangar/
```

## 4. Modules — explicit, composable, one-character disable

- **A module is an explicit named declaration:** `module name { … }`. Multiple
  modules may live in one file, so you can group and toggle them independently.
- **Disable with a leading underscore:** `module _name { … }` is **not
  discovered or merged**. One character, reversible, obvious at a glance. (This
  supersedes jetos D-OS1's "the file is the module".)
- **Auto-discovery by convention:** `imports: find("./modules")` discovers every
  `.jet` file in the tree and merges each module's typed contributions — no
  manual `imports = [ … ]` list (flake-parts / import-tree, by default).
- **Liftability law (from jetos-design §5.5, generalized):** modules may **not**
  import each other. They only contribute to the merged whole. This is what
  makes "drop a file in" safe and keeps composition from exploding.

## 5. Namespaces & types

Reserved namespaces any module may contribute to, each with a matching type:

| Namespace | Type | Configures | Example |
|---|---|---|---|
| `env` | `Env` | a development environment / shell | `env.dev: Env { … }` |
| `system` | `System` | a whole machine (jetos) | `system.laptop: System { … }` |
| `image` | `Image` | an ISO / VM / disk image (jetos) | `image.installer: Image { … }` |

Packages are values of type **`Pkg`**. Source refs are `provider@target`:
`github@owner/repo/rev`, `path@../local`, `nixpkgs@…`. In `packages:` lists
(item type `Pkg`), the type-directed sugar applies: `default.ripgrep`,
`default.[ripgrep, fd]`, and `unstable.neovim`. Strings (`"mine@hello"`) remain
the escape hatch for refs the sugar doesn't cover.

**Provider kind is inferred, not declared (U9).** A source is *only* ever
`name: provider@target#version` — there is no `via:`/kind marker. How a source realizes
is inferred from its target: a target carrying a **`pkg.jet`** is a Jet
package repo and realizes through the first-party **core** provider (no nix);
any other target **falls back to a nix flake**. So core is the default *when the
target is one of ours*, and the entire nixpkgs ecosystem comes for free as the
fallback. The probe never clones a nixpkgs-sized repo: `path@…` stats the dir,
`nixpkgs@…` is unconditionally nix (never probed), and `github@…`/git URLs peek
at **only** `pkg.jet` (raw fetch / shallow `git archive`) before deciding
whether to do a full fetch.

**Interim policy — nixpkgs is acceptable now, gated on the user having Nix
(owner, 2026-06-24).** Until jetpack's own realization is complete, native /
system dependencies (C libraries, math/SIMD backing libs like BLAS, etc.) may
be provisioned through **nixpkgs** — but *only when the user is on NixOS / has
Nix available*. This is the `nix` compatibility provider doing real work today
(`Source/Jetpack/Provider.rs`: "every built-in source routes to `nix`"; C-deps
auto-provision `nixpkgs#<attr>` in `Source/CFFI.rs`). It is explicitly a
**stopgap**: long-term, jetpack's first-party **core** provider owns native
provisioning so a Jet user needs no Nix. A non-Nix user hitting a native dep
should get a clear "this currently needs Nix; jetpack-native provisioning is
planned" path, never a silent failure. Track the migration here, not as a new
ballot — there is no syntax choice, only the provider backend.

## 6. Merge rules (canonical — one table for all tiers)

Reconciles jetos-design §5.4 and the former pack-abi table into one referee:

| Field | Rule |
|---|---|
| `sources` | merge by key; duplicate names with different refs **conflict** unless explicitly overridden |
| `packages` | concatenate, de-duplicate, preserve source identity |
| `env.*` / `system.*` / `image.*` | merge by namespace key; package lists combine; scalar fields conflict unless priority-marked |
| `services` / `options` | merge by key; scalar conflicts are diagnostics unless `default`/`force`-marked |
| scalar settings | one value wins only by explicit priority (`default`/`force`) |

## 7. Why this is genuinely better than Nix

1. **One real language for code *and* config** — same syntax, LSP, formatter,
   and diagnostics. Nix config is a separate untyped language.
2. **Typed by default** — a bad package ref or unknown option is a *local
   diagnostic at edit time* with "did you mean", not an evaluator stack trace.
3. **Import-tree by default (`find`)** — no `inputs`/`outputs`/`system`/`mkShell`
   boilerplate; convention discovers modules.
4. **Tidy by default** — all machinery in `.jet/`; one shared hangar store; no
   `flake.lock`/`result` symlinks scattered in your repo.
5. **One coherent scale** — the same module/merge model carries you from dev
   shell → whole machine → ISO. Nix needs flakes + home-manager + NixOS modules
   + flake-parts glued together.
6. **No footguns** — no `with`, no `rec`, no lazy infinite recursion, no
   stringly-typed paths; explicit typed refs and a one-char module disable.
7. **Honest sandboxed eval** (pure `fn`, S60) — deterministic, with call-trace
   diagnostics when something sneaks in I/O.
8. **One store, one lockfile, reproducible, air-gappable, backup-friendly.**

## 8. Scales — progressive disclosure (jet stays sacred)

| Scale | You write | Tool |
|---|---|---|
| 0 — script | nothing — just `app.jet` | `jet run app.jet` |
| 1 — package | `pkg.jet` (in package dir) | `jet build` / `jetpack` |
| 2 — project env | `jetpack.toml` + `env.jet` (at root) | `jet dev` / `jetpack enter` |
| 3 — system | `config.jet` (jetos, deferred) | `jetos switch` / `build` |

Scale 0 never needs a manifest, `.jet/`, or anything — the hard line that keeps
jet a beginner's first compiled language.

## 9. Future development (out of scope now, designed-for)

- **Imperative operations.** Ad-hoc `jetpack install/remove` against the shared
  hangar (à la `nix profile`), reconciled with the declarative config — either
  recorded back into a module or tracked in a separate imperative profile.
  Designed against the same store; not v1.
- **Multi-machine / fleet** config from one `config.jet`.
- **Signed hangar + generations/rollback** (ties to E2-M16 layer 3, S60).

## 10. Package manager architecture (D-PM1…8 — ratified 2026-06-13, superseded naming only by U-series)

The v1 source-library package manager design was ratified as D-PM1…8. File names (`jet.toml`, `jet.lock`, `~/.jet/store`) were later renamed by U1/U2/U10 to the current surface (`pkg.jet`, `.jet/lock`, hangar store); the architecture decisions below remain authoritative.

**Package manager invariants (PM-I1…PM-I8):**

- **PM-I1** No code execution at install/fetch time.
- **PM-I2** Store append-only; verify fingerprint on create and on demand.
- **PM-I3** Lockfile + store (or network) reproduces exact tree byte-for-byte.
- **PM-I4** Resolution downloads metadata only; packages fetched after version selection.
- **PM-I5** Registry entries immutable; yank flags, never deletes bytes.
- **PM-I6** One mechanism per job: one manifest, one lockfile, one store layout.
- **PM-I7** Every diagnostic: E12xx code, what/why/fix, ui snapshot.
- **PM-I8** R9 forever: `jet run file.jet` with no manifest works as today.

**D-PM1 — Store path layout:** Content-addressed store with human-readable names first: `<store-root>/<name>-<version>-<full-fingerprint>/`. The full plan fingerprint disambiguates when the same name+version could differ. Identity for correctness is always the lockfile entry, not parsing the directory name. Phase 1 stores source trees only; Phase 2 adds compiled artifact cache.

**D-PM2 — Manifest language:** `pkg.jet` (Jet syntax, U10) is the only manifest for dependencies and package identity. A future `build.jet` (layer 3, separate ballot) may describe build steps; it never merges with manifest duties.

**D-PM3 — Resolver phasing:** Phase 1 (M12.1): exact pins only; E1201 on version conflicts. Phase 2 (M12.2): registry, semver ranges, PubGrub resolver — one version per package name in the graph.

**D-PM4 — Tooling binary:** M12.1 implements fetch/store/lock in `jet`. For source-library workflow, users use `jet add`/`jet fetch`/`jet update`. Layer 3 (`jetpack`) owns binary packages/environments and is invoked directly; later `jet add/remove` may plumb to it.

**D-PM5 — Store location:** Phase 1: `~/.jet/store/`; Phase 2 (jetos): `/etc/jet/hangar/`. No root daemon. `jet gc` removes unreferenced entries; `jet store verify` re-checks hashes.

**D-PM6 — Registry (Phase 2):** Append-only git repo; `--as-of <date>` = older commit; sema-enforced API diff at `jet publish`.

**D-PM7 — Generations:** Layer 3 for global tools; projects use git on `.jet/lock`.

**D-PM8 — Cross-machine cache:** Local reuse Phase 2; signed remote cache at layer 3.

**Lockfile schema:** Graph-shaped TOML (`version = 1`), schema versioned, original selector + locked identity per node (source echoes manifest selector; locked holds exact rev + tree-hash), plan fingerprint per node, stable key order.

**Fingerprint:** Covers the whole dependency ancestry — change a deep dep, new identity for everything above it; old and new coexist; nothing mutated in place.

## 11. Ratified naming ledger (ratified 2026-06-16, updated 2026-06-18)

> **Section 10** above holds the still-authoritative D-PM1…8 package architecture decisions (ratified 2026-06-13). File names in those decisions were superseded by U1/U2/U10; the architecture stands.

| Thing | Name |
|---|---|
| monorepo manifest | `jetpack.toml` (TOML; at repo root; U1/U10 revised) |
| package definition | `pkg.jet` (Jet syntax; one per package; in user-chosen location; U10 revised) |
| project environment | `env.jet` (Jet syntax; at repo root; defines sources + packages + shell) |
| system config | `config.jet` (jetos, deferred) |
| lockfile | `.jet/lock` (generated; in `.jet/` folder; replaces `jet.lock`) |
| build cache | `.jet/cache/` (generated; in `.jet/` folder) |
| project-managed folder | `.jet/` (holds lock + cache only; source manifests at root or package level) |
| hangar store | `~/.jet/store/` (Phase 1) or `/etc/jet/hangar/` (Phase 2 jetos) |
| source refs | `provider@target#version` (U1/U9 revised; e.g. `github@owner/repo#tag`) |
| provider@target separator | `@` for provider classifier; `#` for version/revision pin |

## 12. Decisions (U-series — RATIFIED 2026-06-16)

U1–U10 are **ratified** and recorded in `docs/spec/syntax-decisions.md` (Ratified
section + decision log) and `Source/Syntax.rs`; `tests/decisions.rs` enforces them.

| ID | Decision | Status |
|---|---|---|
| U1 | Retire `jet.toml` → a Jet-syntax package manifest; amends ratified **S52** (filename finalized by U10) | ratified |
| U2 | Unify `jet.lock` + `pack.lock` → single `.jet/lock`; `.jet/` managed folder (amends S52) | ratified |
| U3 | Explicit `module name {}` + `_` disable everywhere; **supersedes jetos D-OS1** (file-is-module) | ratified |
| U4 | `find(...)` auto-discovery as the default import surface (generalizes jetos D-OS7) | ratified |
| U5 | The §6 canonical merge table across all tiers (replaces jetos §5.4 + the former pack-abi table) | ratified |
| U6 | `provider@target` source refs (was D-JPK18) + `Pkg` sugar (was D-JPK19) | ratified |
| U7 | `jet run file.jet` stays zero-ceremony forever | ratified |
| U8 | `sources:`/`imports:` nest inside `module {}` (siblings of contributions), not file top-level; amends U4 | ratified |
| U9 | A source's provider kind is **inferred, never declared** — target with a `pkg.jet` → core provider, else nix flake fallback; `nixpkgs@…` always nix; manifest-only remote probe (no `via:` marker). Revised 2026-06-18: `pkg.jet` instead of `payload.jet` | ratified |
| U10 | **File structure (ratified 2026-06-18):** Monorepo manifest **`jetpack.toml`** (TOML, at root); package definitions **`pkg.jet`** (Jet, one per package, user-chosen locations); dev environment **`env.jet`** (Jet, at root). Package identity: `payload: { name, version }` block + `packages: { name: library\|executable }` listing. Amended from earlier U1 | ratified |

U1–U10 now fully reflected in current docs. File names: `jetpack.toml` (root), 
`env.jet` (root), `pkg.jet` (packages), `.jet/lock` (generated).
