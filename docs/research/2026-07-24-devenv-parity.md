# Devenv parity for Jet / jetpack

*Mined 2026-07-24 from [Adumh00man — Nix Devenv is the Flake Killer (Pt.2)](https://www.youtube.com/watch?v=jldArhyi6jM) plus [devenv.sh](https://devenv.sh) docs/options. Owner directive: Jet must support a **flake-class** equivalent (inputs/lock/outputs/reproducibility) **including** the devenv feature surface via jetpack — not by adopting Nix devenv as the product.*

**Report-only inventory + Tower follow-through.** Cards listed at the end (small-scope slices; mega-cards are trackers).

## Sources

| Source | Role |
|---|---|
| Video `jldArhyi6jM` | Demo of daily devenv UX (hook, languages, scripts, files, tests, services, containers) |
| [devenv.sh](https://devenv.sh) + [options reference](https://devenv.sh/reference/options/) | Canonical feature catalog (45 top-level option roots; 58 languages; 42 services) |
| Epoch 4 vision + U11–U29 | Binding Jet law for env/services/images/bridge |
| Owner 2026-07-24 | Full support required; flake-equivalent explicitly wanted |
| Owner 2026-07-24 (amend) | Beginner default = easy Jet `env.*`; **experts also get flake-parts** support (dual-facet — not a reject) |
| Owner 2026-07-24 (ratify A + comment) | Align flake-parts expert path with Jet package/module paradigms: single-file vs modular composition — do not invent a parallel system |
| Owner 2026-07-25 | Locked product answers below; acceptance spine = Jet dogfoods Jet |

Captions on the video were auto-only (medium confidence). Product claims below rest on devenv docs where the demo was incomplete (container run failed on host).

---

## Owner locks (2026-07-25)

| Topic | Lock |
|---|---|
| Success bar | **Both:** day-1 newbie *and* devenv-refugee parity+delight |
| Service catalog | Grow toward full devenv service set — **no hard flagship-7 cap** |
| Hot-reload | Match **devenv 2.1:** background re-eval on file change → apply at **next prompt**; `--reload` default, `--no-reload` escape |
| Env smoke | **`jet env test`** (distinct from `jet test`) |
| Generated `files.` | Devenv-parity modes on enter (**symlink** / **seed** / **copy**) + optional **`jet env sync`** for CI/no-shell |
| Trust | **Both:** first-run prompt **and** hash allowlist file |
| Acceptance spine | **Jet’s own devenv for Jet** — dogfood until this repo’s daily shell no longer requires nix-direnv/`use flake` |

### Dual cut lines

| Cut | Meaning |
|---|---|
| **Newbie** | `docs/first-hour.md` can teach `jet env` / hook, not only `nix build` |
| **Refugee / dogfood** | Jet repo core shell works via Jet env; `.envrc` drops `use flake` / `JET_ENV_DISABLE=1` (or becomes a thin Jet-hook wrapper). `jet bridge flake` remains for Nix consumers |

### Dogfood acceptance (stress test)

Current gap: root `env.jet` is a thin package list; `.envrc` still owns the shell via nix-direnv and sets `JET_ENV_DISABLE=1`.

Done when a contributor can:

1. `cd jet` → `jet env hook` activates (no nix-direnv required for daily path)
2. Trust prompt + allowlist once
3. Hot-reload on `env.jet` edit → next prompt
4. `env.dev` (core) + `env.full` (FFI/browser/graphics) via pack/profile — not `JET_DEV_SHELL` + flake attr
5. Language/toolchain packs cover what Jet needs (rust, node, wasm tooling)
6. `#Job`s cover common repo workflows
7. `jet env test` proves tools on PATH + TZDIR/JET_ROOT hygiene
8. Generated files (if needed) via `files.`
9. Optional later: in-dev services; container from same env
10. Daily DX is Jet; flake shim stays for external Nix consumers

---

## Feature matrix

Legend: **shipped** · **partial** (ratified / in progress) · **gap** · **ballot** · **reject** (conflicts Jet law)

### A. Flake-class project graph (owner: required)

| Devenv / Nix feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Locked inputs (`devenv.yaml` / flake inputs) | `sources:` + hangar lock / channel refs (U21) | **partial** | Small cards under #783 |
| Input URI forms (github/git/path/tarball/follows) | package refs `name@provider`; path sources | **partial** | Same |
| Lockfile pin + `update` | lock + `jet update` / `jet outdated` | **partial** | Same |
| Structured outputs (packages, apps, checks, shells) | `Output` kinds + hangar; `D-ECO-OUTPUT*` | **partial** | Same |
| Multiple named shells | `env.<name>` overlays (U19) | **partial** | Same |
| Consume foreign `flake.nix` / `devenv.nix` | U16 bridge; L0204 unmapped fields | **partial** | Same |
| Emit flake shim for Nix consumers | `jet bridge flake` | **partial** | Same |
| Flake-parts composition | — | **ratified A** | #793 tracker + slices |

### B. Shell activation & lifecycle (video-heavy)

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| `devenv shell` / enter env | `jet env` (U19) | **shipped/partial** | Prove tooling completeness |
| Auto-activate on `cd` | D-ENVHOOK1 `jet env hook` | **shipped** | — |
| Explicit allow / trust | vision trust-by-env-hash | **partial** | #784 slices (prompt + allowlist) |
| Hot-reload on env edit | hook re-eval; async rebuild unproven | **gap** | #784 — devenv 2.1 semantics |
| `enterShell` / lifecycle tasks | D-ECO7-class hook; `#Job` | **partial** | #784 |
| `enterTest` / clean-shell smoke | `jet test` ≠ env smoke | **gap** | **`jet env test`** |
| `dotenv` | — | **gap** | #784 |
| `unsetEnvVars` / env hygiene | — | **gap** | #784 |
| Prompt / starship | `prompt:` on env | **partial** | #784 |
| `devenv info` summary | `jet info` (U26) | **partial** | #789 |
| Eval cache / <100ms activate | hangar + native eval work | **partial** | Depends #396/#397 |
| `devenv gc` | U22 hangar GC | **partial** | Covered by hangar cards |
| Ad-hoc CLI env (`-O` / `-p`) | `jet env -p` (U16) | **shipped** | — |

### C. Languages & tooling packs (video-heavy)

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| `languages.<lang>.enable` one-toggle | Explicit `packages: [...]` only | **ratified A** | #791 slices |
| Version / channel / toolchain extras | package refs / overlays (#330) | **partial** | After pack surface |
| Language-bundled LSP/fmt/linter | — | **ratified A** | Pack extras |
| Python venv + requirements | — | **gap** | Dedicated pack slice |
| 58-language catalog | — | **gap** | Catalog expansion slices |

### D. Packages, overlays, search

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| `packages = [ … ]` | `packages:` on env | **shipped** | — |
| Overlays / overrides | D-JPK-OVERLAY1; #330 | **partial** | Existing #330 |
| `devenv search` | U26 `jet search` | **partial** | #789 |
| Cachix pull/push | U24 / D-JPK-CACHE* | **partial** | Existing cache cards |
| Binary cache UX polish | — | **partial** | Existing program |

### E. Scripts, tasks, files, git-hooks (video)

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Named scripts / aliases | `#Job fn` (D-JPK-TASKRUN1) | **shipped** (better shape) | Prefer typed tasks |
| Per-script packages | task-local deps unproven | **gap** | #787 |
| Task DAG / before/after / modes | `#Every` / schedule; limited DAG | **gap** | #787 |
| Task status skip / execIfModified | — | **gap** | #787 |
| Task I/O JSON / exports | — | **gap** | #787 |
| Generated `files.` (json/toml/sh) | no env.files | **gap** | #787 — symlink/seed/copy + `jet env sync` |
| `git-hooks` / pre-commit catalog | D-ECO6 `git_hooks_path` mention | **gap** | #787 |
| `treefmt` / formatter integrations | D-ECO12 formatter passthrough | **partial** | #787 |

### F. Processes & services (video-heavy)

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Declarative processes + `up`/`down` | D-JPK-SERVICE1 / `jet services` | **partial** | #785 slices |
| Ready probes (exec/http/notify/TCP) | ready/ports (E1261) | **partial** | #785 |
| Restart policies | — | **gap** | #785 |
| File watch → restart | #439 watch; not process-tied | **gap** | #785 |
| Socket activation | — | **gap** | #785 |
| Automatic port allocation | — | **gap** | #785 |
| Process↔task dependency graph | — | **gap** | #785 |
| Alternate managers (process-compose…) | — | **reject** | I8 one supervisor |
| Service presets (postgres, redis, …) | typed Service; shared typed catalog constructors | **shipped** | — |
| Service state dirs / init scripts | partial | **partial** | #786 |

### G. Containers (video)

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Build OCI from env (`shell` / `processes`) | D-JPK-IMAGE1 from packages | **partial** | #788 |
| `container run/copy` + registry | push TLS-gated E1268 | **partial** | #788 |
| Custom containers + copyToRoot | Image fields partial | **gap** | #788 |
| `devcontainer` export | — | **gap** | #788 P2 |
| macOS remote Linux builder | D-JPK-REMOTE1 | **partial** | Existing remote cards |

### H. Composition & profiles

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Local/remote imports | `imports: find(...)` (U4) | **partial** | Profile/compose slices |
| Monorepo folder envs | workspace.jet | **partial** | Same |
| Named profiles + `extends` | env overlays only | **ratified C** | #790 |
| Hostname / user auto profiles | — | **ratified C** | #790 — must show in `jet env info` |
| Package profiles / generations | D-JPK-PROFILE* | **partial** | Existing (different concept) |

### I. Secrets & trust

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| Declared secrets | U13 / vault (stronger than secretspec) | **shipped** | Prefer Jet vault |
| secretspec.dev providers | — | **reject**/defer | Do not fork; map providers into vault if needed later |
| Trust allow before activate | vision | **partial** | Prompt + allowlist |

### J. Tooling / AI / niche modules

| Feature | Jet today | Status | Follow-up |
|---|---|---|---|
| `devenv lsp` for options | U26 LSP for env fields | **partial** | #789 |
| `devenv mcp` | #768 MCP campaign | **partial** | Link #768; env tools as MCP resources |
| editor / agent tool modules | typed `env.editor.vscode` / `env.agent.codex` projections | **shipped** | #792 / #1101 |
| android / apple SDK modules | typed `env.platform.android()` / `env.platform.apple()` presets | **shipped** | #792 / #1099 |
| certificates / hosts | typed `env.security.certificates` / `env.network.hosts` projections | **shipped** | #792 / #1100 |
| `outputs` language.import packaging | package recipes / adapters | **partial** | Flake-class + package program |

### Better-than-devenv (ergonomics / UX / DX)

Keep as product advantages; do not dilute:

- Typed `#Job fn` (not shell-string scripts)
- Vault secrets (stronger than secretspec)
- Beginner `env.*` without Nix ceremony; expert flake-parts without a parallel system
- One process supervisor (I8)
- Sub-100ms activate + honest progress; `jet doctor` / cockpit `jet info`
- Dogfood proves the story under real monorepo load

### Video-called-out checklist (must not slip)

1. Flake boilerplate → developer-oriented env — **Jet `env.*`**
2. YAML/inputs + rolling nixpkgs — **sources + channels**
3. Shell hook auto-activate + allow — **D-ENVHOOK1 + trust**
4. Hot reload on edit — **devenv 2.1 semantics**
5. LSP completions for options — **U26 partial**
6. `languages.rust.enable` (+ channel/version) — **#791**
7. Python venv + requirements — **pack slice**
8. Scripts + per-script packages — **#Job + #787**
9. Generated files from config — **symlink/seed/copy + sync**
10. Env smoke — **`jet env test`**
11. Services one-liner (postgres) — **#786**
12. Containers from same env — **#788**

---

## Tower follow-through

Small-scope slices measure progress; **TRACKER** cards close only when their slices are done.

### Trackers → slices

| Tracker | Role | Slices |
|---|---|---|
| **#783** | Flake-class graph + bridge | **#794** URI/follows · **#795** lock/update · **#796** outputs · **#797** named shells · **#798** L0204 packages+services · **#799** bridge round-trip |
| **#784** | Env lifecycle | **#800** trust prompt+allowlist · **#801** hot-reload (devenv 2.1) · **#802** enterShell Tasks · **#803** dotenv · **#804** unsetEnvVars · **#805** `jet env test` · **#806** prompt polish |
| **#785** | Process supervisor depth | **#807** ready probes · **#808** restart · **#809** watch→restart · **#810** port alloc · **#811** socket activation · **#812** process↔task DAG |
| **#786** | Service catalog (no 7-cap) | **#813** preset framework · **#814** postgres · **#815** redis · **#816** mysql/mariadb · **#817** nginx · **#818** minio · **#819** expansion toward full set |
| **#787** | Env ergonomics | **#820** task-local packages · **#821** task DAG/skip · **#822** files. symlink/seed/copy + `jet env sync` · **#823** git-hooks · **#824** treefmt |
| **#788** | Env containers | **#825** shell OCI · **#826** processes OCI · **#827** copyToRoot · **#828** run/copy/registry · **#829** devcontainer P2 |
| **#789** | Discoverability | **#830** info cockpit · **#831** search · **#832** env LSP · **#833** doctor |
| **#790** | **D-ENV-PROFILE1=C** | **#834** named+extends · **#835** hostname/user auto + info disclosure |
| **#791** | **D-ENV-LANGPACK1=A** | **#836** pack surface · **#837** rust · **#838** node · **#839** python+venv · **#840** catalog→58 |
| **#792** | Optional P2 | **#841** certs/hosts · **#842** android/apple · **#843** editor agents |
| **#793** | **D-ENV-FLAKEPARTS1=A** | **#844** map flake-parts↔Jet modules · **#845** expert docs+golden |
| **#853** | Dogfood acceptance spine | **#846** map flake→env.dev/full · **#847** shellHook/wrappers · **#848** replace nix-direnv `.envrc` · **#849** repo `#Job`s · **#850** repo `jet env test` · **#851** first-hour path · **#852** closeout (`needsAcceptance`) |

### Already covered — do not duplicate

- D-ENVHOOK1, U12/U13/U14/U16/U19/U21–U29
- #330 overlays, #396/#397 nix eval, hangar/cache/GC cards
- #439 watch, #768 MCP, secrets vault cards
- Reject only as **beginner-required** path: flake-parts (experts get it via D-ENV-FLAKEPARTS1)
- Still reject: alternate process managers as the default supervisor menu; secretspec fork
