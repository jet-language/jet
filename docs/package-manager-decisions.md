# Package manager — unified vision & decision file

> **STATUS (2026-06-12): DRAFT for owner ratification.** This file
> reconciles docs/plans/m12-packages.md (Cargo-style, ratified S52
> surface) with docs/jetpack.md (Nix-style exploration, unratified) into
> ONE plan, as required by docs/00-philosophy.md and docs/05-roadmap.md
> ("must be reconciled with M12 before any package-manager work starts").
> Once the D-PM decisions below are ratified, this file is the single
> source of truth; docs/jetpack.md becomes historical and
> docs/plans/m12-packages.md is rewritten to match.

---

## 1. Vision (one paragraph)

**Nix's engine under Cargo's steering wheel.** Jet's package manager
stores every package — every *version* of every package — immutably in
one content-addressed store, identified by a hash that covers its entire
dependency ancestry. Nothing is ever installed twice, upgraded in place,
or silently broken by an unrelated change; old and new versions coexist;
rollback is re-pointing a link. But no user ever learns that vocabulary
to ship a project: the daily surface is `jet add`, `jet run`, `jet.toml`,
`jet.lock` — the Cargo/uv workflow that won. Nix's power, none of Nix's
learning curve; Jet's diagnostics voice throughout (the exact thing Nix
lacks). Single-file `jet run file.jet` never needs any of it (R9, sacred).

## 2. What we take from each package manager (the survey)

| Source | What we take | What we refuse |
|---|---|---|
| **Nix** (primary inspiration) | Content-addressed immutable store; hash covers full dependency closure (Merkle); any number of versions coexist; atomic switch + rollback via generations; hermetic/pure evaluation (S60 `pure fn` is the enabling language feature); binary substitution keyed by hash; time-travel via pinned snapshots | The Nix language (Jet *is* the replacement — S60); the learning cliff; inscrutable errors; `/nix/store` requiring root daemon |
| **Cargo** | The workflow: one manifest, one lockfile, `add`/`build`/`run`/`test`/`publish` integrated with the compiler; lockfile-authoritative builds; `--locked` for CI | `build.rs` (arbitrary code at build time — our supply-chain stance forbids it); per-project `target/` duplication |
| **pnpm** | Hardlink packages from the global store into projects — ten projects share one copy, installs are milliseconds and ~0 bytes | node_modules layout complexity |
| **uv** | Speed as a feature: metadata-only resolution (never download packages to resolve), parallel index fetches, PubGrub error explanations | — |
| **Go modules** | Checksum transparency: every dependency's tree hash recorded and verified on every build (M12 already has this — E1204); trivial vendoring for air-gapped builds; proxy/mirror friendliness | MVS resolver (surprising to most users); import-path-versioning (`/v2`) |
| **Elm** | Enforced semver on publish: the registry API-diffs your public surface and *refuses* a minor bump that breaks it. Jet's sema knows every `pub` signature — this is nearly free and is the flagship registry feature (cli-tooling-research item 14) | — |
| **Deno** | Capability honesty: `jet build` can print what a dependency tree can touch (fs/net/process) from its std imports | URL imports |
| **npm** (anti-lessons) | — | Install scripts/postinstall hooks (ratified out already: "a dependency can't run code at install time"); silent duplicate versions; mutable registry (left-pad) — our registry entries are immutable, yank = flag, never delete |
| **apt/Homebrew** (anti-lessons) | — | One global mutable prefix where installing X breaks Y — the store model makes this class of failure impossible |

## 3. The architecture (three layers, one store)

```
LAYER 1 — daily workflow (Cargo-shaped; ships in M12 phase 1)
  jet.toml (S52) · jet.lock · jet add/remove/fetch/build/run/test
        │
LAYER 2 — resolution & registry (uv/Elm-shaped; M12 phase 2)
  static git index · PubGrub semver ranges · registry snapshots
  (`--as-of <date>`) · enforced-semver publish · jet audit / SBOM
        │
LAYER 3 — the engine (Nix-shaped; underneath from day one)
  content-addressed store  ~/.jet/store/<hash>-name-version/
  hash = sha256 of the package's plan, which embeds its deps' hashes
  immutable · hardlinked into projects · GC with lockfiles as roots
```

Layer 3 is not a later phase — it is the *storage model of phase 1*.
What changes by phase is only what goes in the store:

- **Phase 1:** verified source trees of dependencies (git/path). Deps
  compile from source per project (current M12 rule), but live in the
  store once, hardlinked everywhere — pnpm semantics immediately.
- **Phase 2:** registry packages; cached *generated .rs / compiled
  artifacts* keyed by plan hash, so a dependency at a given version+deps
  compiles once per machine, ever (Nix's cache key idea applied to
  Cargo's pain point).
- **Phase 3 (post-v1):** sandboxed builds of pure-Jet recipes
  (`jet eval --pure`, S60), signed binary-cache substitution, profiles/
  generations for installed tools. This is docs/jetpack.md's JP0–JP6,
  re-homed onto the same store and lockfile instead of a parallel
  system. jetos (docs/jetos.md) waits on this layer, unchanged.

The hash discipline is Nix's, from day one: a dependency's identity is
`sha256(canonical plan)` where the plan = `{name, version, source
url+tree-hash, deps: [dep plan hashes…]}`. Patch a leaf dependency and
every dependent's identity changes — "upgrade broke an unrelated
project" is structurally impossible, and the hash is a future worldwide
cache key for free.

## 4. Decisions for the owner (D-PM1…D-PM8)

Primary (these steer the vision):

| ID | Question | A | B | C | Rec |
|----|----------|---|---|---|-----|
| D-PM1 | Core architecture | **Nix core, Cargo surface**: global content-addressed store + hardlinks from phase 1; Cargo workflow on top (§3) | Full-Nix now: pure recipes, sandbox builds, general software builder (jetpack.md as spec) | Cargo-first: per-project cache as currently planned in M12; bolt store on later | **A** — B front-loads years of work before `jet add http` works; C bakes in the wrong storage model and a migration |
| D-PM2 | Manifest language | **Keep `jet.toml` (S52, frozen)** for projects; pure-Jet recipe files appear only in layer 3 for packaging non-Jet software | Un-ratify S52; pure-Jet `package.jet` for everything (jetpack D-JP2) | TOML now, migrate to .jet manifests at layer 3 | **A** — S52 is ratified+enforced (tests/decisions.rs); data-shaped TOML is machine-editable by `jet add`; recipes-as-code only where code is needed |
| D-PM3 | Version resolution | **Exact pins in phase 1; PubGrub semver ranges in phase 2**; one version per package name in the graph (E1201 on conflict) until evidence demands duplication | Ranges + PubGrub from day one | Cargo-style: allow multiple semver-major versions to coexist (store makes it free) | **A** — beginner-first: one version per name keeps mental model simple; the store already supports C if evidence arrives |
| D-PM4 | Tool identity | **Everything in the `jet` binary**; I6 holds (shell out to `git`/`curl`, vendored sha256) | Separate `jetpack` binary exempt from I6 (tokio/reqwest/pubgrub crates) | `jet` subcommands now; split an engine binary out at layer 3 if needed | **A** — one tool to teach, no I6 carve-out (carve-outs have been declined before); PubGrub is implementable in std Rust; revisit only if layer 3 demands it |

Secondary (defaults proposed; object where wrong):

| ID | Question | A | B | Rec |
|----|----------|---|---|-----|
| D-PM5 | Store location | **`~/.jet/store`** (rootless, per-user; no daemon) | `/jet/store` system-wide | **A** — store paths don't appear in binaries in phase 1–2 (source-compiled deps), so short stable system paths buy nothing yet; revisit at layer 3 |
| D-PM6 | Registry shape | **Static git index** (JSON-lines per package: name, versions, source URL, tree hash) with append-only history — which *is* the snapshot mechanism: `--as-of <date>` = the index at that commit | Hosted API service | **A** — trivially hostable/mirrorable/auditable; Go-proxy-style mirrors and private indexes are just more git remotes (enterprise.md req); promote to API only on demonstrated need |
| D-PM7 | Generations & rollback | **Layer 3 (post-v1)**, for globally installed tools (`jet install <tool>` profiles); projects don't need it — `jet.lock` in git *is* the project's generation history | In M12 | **A** — matches Nix's actual value split: generations matter for environments, lockfiles for projects |
| D-PM8 | Binary cache / substitution | **Layer 3 (post-v1)**: HTTP cache, zstd, ed25519-signed, keyed by plan hash; phase 2's compiled-artifact cache is local-only first | In M12 phase 2 | **A** — needs hash discipline proven in the wild first; refusing unsigned substitutions needs a key-distribution story |

## 5. Phasing (replaces both M12's two phases and jetpack's JP0–JP6)

| Phase | Milestone | Ships | Exit criteria (tests) |
|-------|-----------|-------|----------------------|
| 1 | **M12.1** | `jet.toml` + path/git deps, exact pins; `jet add/fetch`; `jet.lock` with plan hashes (§3); **content-addressed store + hardlink linker**; E1201–E1206; `--locked` | current m12-packages.md battery **plus**: two fixture projects sharing one dep have identical store inodes; tampered store path detected (E1204); lock replay reproduces identical plan hashes |
| 2 | **M12.2** | static git-index registry; PubGrub + semver ranges; `jet publish` with **enforced semver API diff**; `--as-of`; compiled-artifact cache (local); `jet audit` + SBOM emission (CycloneDX — enterprise.md); vendoring (`jet vendor`) | conflict-explanation snapshot (PubGrub voice, docs/04 format); publish-refusal snapshot on breaking minor bump; `--as-of` reproduces an old lock byte-for-byte; air-gapped build from vendor dir |
| 3 | **post-v1** (needs its own plan + ballots; subsumes jetpack JP0–JP6 and unblocks jetos) | `jet eval --pure` (S60) over recipe files; sandboxed builds; signed binary caches; profiles/generations/GC for installed tools; packaging non-Jet software | jetpack.md §6 JP-row criteria, re-homed |

## 6. Invariants (extend I1–I8)

- **PM-I1** No code execution at dependency install/fetch time, ever.
  No install hooks, no build scripts in phase 1–2. (Already ratified
  in M12; npm's hardest-learned lesson.)
- **PM-I2** The store is append-only. Nothing in it is edited; paths
  are only added or garbage-collected. Every store path's content is
  verified against its hash on creation and re-verifiable on demand.
- **PM-I3** The lockfile is generated, authoritative, and sufficient:
  a `jet.lock` + the store (or network) reproduces the exact
  dependency tree, byte-for-byte, on any machine.
- **PM-I4** Resolution never downloads packages — metadata only
  (uv's rule). Downloads happen once, after versions are chosen.
- **PM-I5** Registry entries are immutable. Yanking flags a version;
  it never deletes bytes that a lockfile may point to.
- **PM-I6** One mechanism per job: one manifest, one lockfile, one
  store layout, one registry protocol. No alternates, no escape
  hatches.
- **PM-I7** Every package-manager diagnostic carries an E12xx code,
  what/why/fix, and a ui snapshot (compiler I4 applies unchanged).
- **PM-I8** R9 stands forever: `jet run file.jet` with no manifest
  works exactly as today, touching none of this machinery.

## 7. Conflicts this file resolves (the reconciliation ledger)

| Conflict | jetpack.md said | m12-packages.md said | Resolution here |
|----------|-----------------|----------------------|-----------------|
| Manifest | pure-Jet `package { }` | `jet.toml` (S52 ratified) | jet.toml (D-PM2); .jet recipes only at layer 3 |
| Pin policy | semver ranges + PubGrub now | exact pins only | pins phase 1 → ranges phase 2 (D-PM3) |
| Resolver | pubgrub-rs crate | n/a (no ranges) | PubGrub algorithm, std-only implementation (D-PM4) |
| Tool | separate `jetpack` binary, I6-exempt | `jet` subcommands | `jet` subcommands, I6 holds (D-PM4) |
| Store | `/jetpack/store`, phase 1 of its own track | none (cache dir `~/.cache/jet/pkg`) | `~/.jet/store`, content-addressed, in M12.1 (D-PM1/5) |
| Lockfile name/format | `jetpack.lock`, canonical JSON | `jet.lock` | `jet.lock`; format carries plan hashes (canonical, generated) |
| Registry | full registry + binary cache service | static git index | static git index, append-only = snapshots (D-PM6) |
| Generations/GC | core feature | out of scope | layer 3, tools/profiles only (D-PM7) |

Open jetpack/jetos decision IDs (D-JP1–5, D-OS1–7) are superseded or
deferred to layer 3's future plan; none remain open against M12.

## 8. On ratification (agent checklist)

1. Owner answers D-PM1…D-PM8 → record in §4 with date.
2. Rewrite docs/plans/m12-packages.md to match §5 phases 1–2 (it keeps
   its E-codes, S52 surface, supply-chain rules; gains the store).
3. Update docs/05-roadmap.md M12 entry + remove the "Unreconciled"
   footnote; update the status banners in docs/jetpack.md, docs/jetos.md,
   and docs/00-philosophy.md to point here.
4. Any *syntax* implied by layer 3 (recipe files, `pure fn` surfaces
   beyond S60) still requires its own ballot before code — this file
   ratifies architecture, not syntax.
