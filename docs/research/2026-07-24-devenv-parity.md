# Devenv parity for Jet / jetpack

*Mined 2026-07-24 from [Adumh00man — Nix Devenv is the Flake Killer (Pt.2)](https://www.youtube.com/watch?v=jldArhyi6jM) plus [devenv.sh](https://devenv.sh) docs/options. Owner directive: Jet must support a **flake-class** equivalent (inputs/lock/outputs/reproducibility) **including** the devenv feature surface via jetpack — not by adopting Nix devenv as the product.*

**Report-only inventory + Tower follow-through.** Cards/ballots listed at the end.

## Sources

| Source | Role |
|---|---|
| Video `jldArhyi6jM` | Demo of daily devenv UX (hook, languages, scripts, files, tests, services, containers) |
| [devenv.sh](https://devenv.sh) + [options reference](https://devenv.sh/reference/options/) | Canonical feature catalog (45 top-level option roots; 58 languages; 42 services) |
| Epoch 4 vision + U11–U29 | Binding Jet law for env/services/images/bridge |
| Owner 2026-07-24 | Full support required; flake-equivalent explicitly wanted |
| Owner 2026-07-24 (amend) | Beginner default = easy Jet `env.*`; **experts also get flake-parts** support (dual-facet — not a reject) |
| Owner 2026-07-24 (ratify A + comment) | Align flake-parts expert path with Jet package/module paradigms: single-file vs modular composition — do not invent a parallel system |

Captions on the video were auto-only (medium confidence). Product claims below rest on devenv docs where the demo was incomplete (container run failed on host).

---

## Feature matrix

Legend: **shipped** · **partial** (ratified / in progress) · **gap** · **ballot** · **reject** (conflicts Jet law)

### A. Flake-class project graph (owner: required)

| Devenv / Nix feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Locked inputs (`devenv.yaml` / flake inputs) | `sources:` + hangar lock / channel refs (U21) | **partial** | Card: flake-class graph completeness |
| Input URI forms (github/git/path/tarball/follows) | package refs `name@provider`; path sources | **partial** | Same card — URI parity + follows |
| Lockfile pin + `update` | lock + `jet update` / `jet outdated` | **partial** | Same card — prove flake.lock / devenv.lock interop story |
| Structured outputs (packages, apps, checks, shells) | `Output` kinds + hangar; `D-ECO-OUTPUT*` | **partial** | Same card — env-linked outputs |
| Multiple named shells | `env.<name>` overlays (U19) | **partial** | Same card — named env ≡ multiple shells |
| Consume foreign `flake.nix` / `devenv.nix` | U16 bridge; L0204 unmapped fields | **partial** | Same card — close L0204; native eval (#396) path |
| Emit flake shim for Nix consumers | `jet bridge flake` | **partial** | Same card — round-trip acceptance |
| Flake-parts composition | — | **ballot** | D-ENV-FLAKEPARTS1 — beginner Jet default; expert flake-parts path |

### B. Shell activation & lifecycle (video-heavy)

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| `devenv shell` / enter env | `jet env` (U19) | **shipped/partial** | Prove tooling completeness |
| Auto-activate on `cd` | D-ENVHOOK1 `jet env hook` | **shipped** | — |
| Explicit allow / trust | vision trust-by-env-hash | **partial** | Card: env lifecycle |
| Hot-reload on env edit | hook re-eval; async rebuild unproven | **gap** | Card: env lifecycle |
| `enterShell` / lifecycle tasks | D-ECO7-class hook; `#Task` | **partial** | Card: env lifecycle |
| `enterTest` / clean-shell smoke | `jet test` ≠ env smoke | **gap** | Card: env lifecycle |
| `dotenv` | — | **gap** | Card: env lifecycle |
| `unsetEnvVars` / env hygiene | — | **gap** | Card: env lifecycle |
| Prompt / starship | `prompt:` on env | **partial** | Card: env lifecycle (prompt polish) |
| `devenv info` summary | `jet info` (U26) | **partial** | Card: discoverability |
| Eval cache / <100ms activate | hangar + native eval work | **partial** | Depends #396/#397; note on flake-class card |
| `devenv gc` | U22 hangar GC | **partial** | Covered by hangar cards |
| Ad-hoc CLI env (`-O` / `-p`) | `jet env -p` (U16) | **shipped** | — |

### C. Languages & tooling packs (video-heavy)

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| `languages.<lang>.enable` one-toggle | Explicit `packages: [...]` only | **ratified A** | #791 — typed packs |
| Version / channel / toolchain extras | package refs / overlays (#330) | **partial** | #791 after packs |
| Language-bundled LSP/fmt/linter | — | **ratified A** | #791 |
| Python venv + requirements | — | **gap** | #791 pack extras |
| 58-language catalog | — | **gap** | #791 |

### D. Packages, overlays, search

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| `packages = [ … ]` | `packages:` on env | **shipped** | — |
| Overlays / overrides | D-JPK-OVERLAY1; #330 | **partial** | Existing #330 |
| `devenv search` | U26 `jet search` | **partial** | Card: discoverability |
| Cachix pull/push | U24 / D-JPK-CACHE* | **partial** | Existing cache cards |
| Binary cache UX polish | — | **partial** | Existing program |

### E. Scripts, tasks, files, git-hooks (video)

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Named scripts / aliases | `#Task fn` (D-JPK-TASKRUN1) | **shipped** (better shape) | Prefer typed tasks |
| Per-script packages | task-local deps unproven | **gap** | Card: ergonomics |
| Task DAG / before/after / modes | `#Every` / schedule; limited DAG | **gap** | Card: ergonomics |
| Task status skip / execIfModified | — | **gap** | Card: ergonomics |
| Task I/O JSON / exports | — | **gap** | Card: ergonomics |
| Generated `files.` (json/toml/sh) | no env.files | **gap** | Card: ergonomics |
| `git-hooks` / pre-commit catalog | D-ECO6 `git_hooks_path` mention | **gap** | Card: ergonomics |
| `treefmt` / formatter integrations | D-ECO12 formatter passthrough | **partial** | Card: ergonomics |

### F. Processes & services (video-heavy)

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Declarative processes + `up`/`down` | D-JPK-SERVICE1 / `jet services` | **partial** | Card: process depth |
| Ready probes (exec/http/notify/TCP) | ready/ports (E1261) | **partial** | Card: process depth |
| Restart policies | — | **gap** | Card: process depth |
| File watch → restart | #439 watch; not process-tied | **gap** | Card: process depth |
| Socket activation | — | **gap** | Card: process depth |
| Automatic port allocation | — | **gap** | Card: process depth |
| Process↔task dependency graph | — | **gap** | Card: process depth |
| Alternate managers (process-compose…) | — | **reject** | I8 one supervisor |
| Service presets (postgres, redis, …) | typed Service; thin catalog | **gap** | Card: service catalog (42 devenv services) |
| Service state dirs / init scripts | partial | **partial** | Service catalog card |

### G. Containers (video)

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Build OCI from env (`shell` / `processes`) | D-JPK-IMAGE1 from packages | **partial** | Card: env containers |
| `container run/copy` + registry | push TLS-gated E1268 | **partial** | Same + TLS gate |
| Custom containers + copyToRoot | Image fields partial | **gap** | Same |
| `devcontainer` export | — | **gap** | Same (lower priority slice) |
| macOS remote Linux builder | D-JPK-REMOTE1 | **partial** | Existing remote cards |

### H. Composition & profiles

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Local/remote imports | `imports: find(...)` (U4) | **partial** | Card: compose/profiles |
| Monorepo folder envs | workspace.jet | **partial** | Same |
| Named profiles + `extends` | env overlays only | **ratified C** | #790 — includes hostname/user auto |
| Hostname / user auto profiles | — | **ratified C** | #790 — must show in `jet env info` |
| Package profiles / generations | D-JPK-PROFILE* | **partial** | Existing (different concept) |

### I. Secrets & trust

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Declared secrets | U13 / vault (stronger than secretspec) | **shipped** | Prefer Jet vault |
| secretspec.dev providers | — | **reject**/defer | Do not fork; map providers into vault if needed later |
| Trust allow before activate | vision | **partial** | Env lifecycle card |

### J. Tooling / AI / niche modules

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| `devenv lsp` for options | U26 LSP for env fields | **partial** | Discoverability card |
| `devenv mcp` | #768 MCP campaign | **partial** | Link #768; env tools as MCP resources |
| `claude.code` / `opencode` modules | — | **gap** | Card: optional integrations (P2) |
| android / apple SDK modules | — | **gap** | Same P2 catalog |
| certificates / hosts | — | **gap** | Ergonomics or integrations |
| `outputs` language.import packaging | package recipes / adapters | **partial** | Flake-class + package program |

### Video-called-out checklist (must not slip)

1. Flake boilerplate → developer-oriented env — **Jet `env.*`**
2. YAML/inputs + rolling nixpkgs — **sources + channels**
3. Shell hook auto-activate + allow — **D-ENVHOOK1 + trust**
4. Hot reload on edit — **gap**
5. LSP completions for options — **U26 partial**
6. `languages.rust.enable` (+ channel/version) — **ballot**
7. Python venv + requirements — **ballot / after**
8. Scripts + per-script packages — **#Task + gap**
9. Generated files from config — **gap**
10. `devenv test` enter tests — **gap**
11. Services one-liner (postgres) — **catalog gap**
12. Containers from same env — **partial/gap**

---

## Tower follow-through

| Ref | Lane | Deliverable |
|---|---|---|
| **D-ENV-LANGPACK1** on #791 | **ratified A** | Typed `languages:` packs |
| **D-ENV-PROFILE1** on #790 | **ratified C** | Named profiles + hostname/user auto (info must disclose) |
| **D-ENV-FLAKEPARTS1** on #793 | **ratified A** | Dual-facet + align with Jet single-file vs modular modules (owner comment) |
| **#783** | plan | Flake-class inputs/lock/outputs + full flake/devenv bridge |
| **#793** | ready | Expert flake-parts interop **aligned with Jet module composition** |
| **#784** | plan | Trust, hot-reload, enter hooks, dotenv, env smoke |
| **#785** | plan | Process supervisor depth (probes/restart/watch/ports/DAG) |
| **#786** | blocked←#785 | Service catalog (flagship 7 → path to 42) |
| **#787** | blocked←#784 | Task-local packages, generated files, git-hooks, treefmt |
| **#788** | blocked←#783,#786 | Env→shell/processes OCI containers |
| **#789** | blocked←#783,#791 | search/info/LSP/doctor |
| **#792** | blocked←#783,#791 | Optional android/apple/certs/hosts/editor modules (P2) |

### Already covered — do not duplicate

- D-ENVHOOK1, U12/U13/U14/U16/U19/U21–U29
- #330 overlays, #396/#397 nix eval, hangar/cache/GC cards
- #439 watch, #768 MCP, secrets vault cards
- Reject only as **beginner-required** path: flake-parts (experts get it via D-ENV-FLAKEPARTS1)
- Still reject: alternate process managers as the default supervisor menu; secretspec fork
