# Epoch 5 — jetpack gates U11–U19: implementation plan

**Card:** c9jetpackgates. **Ratified:** 2026-07-01. **This doc is the HOW.**
The WHAT/WHY lives in [`vision.md`](vision.md) and [`README.md`](README.md);
the shipped-vs-pending baseline is [`IMPLEMENTATION-STATUS.md`](IMPLEMENTATION-STATUS.md).
Do not re-derive design here — read those first, then execute this.

Audience: a mid-tier implementation agent. Every gate below is executable
without re-opening the ballot. All paths absolute-from-repo-root. Run everything
through the Nix shell (`nix develop -c …`, see CLAUDE.md).

---

## Ratified outcomes (from `tools/Tower/tower.json`, `cardId == c9jetpackgates`)

| Gate | Decision | Outcome | One-line semantics |
|---|---|---|---|
| U11 | D-JPK-SCRIPTDEP1 | **A** | `use pkg#ver` inside a bare `.jet` script: `jet run` resolves, locks (cache keyed by file hash), runs. `jet lock <file>` materializes a sidecar; `jet init` lifts inline `use`s into a `pack.jet` `deps:` block. |
| U12 | D-JPK-SERVICE1 | **A** | `services: { postgres: Service.{ version: 16 } }` in an `env.*` module; jetpack **supervises** (up/down/health/logs), health-gates startup, project-scoped under `.jet/`. |
| U13 | D-JPK-SECRET1 | **A** | `secrets: { KEY: secret("name") }`; age-style encrypted file in-repo, decrypted at env entry **in memory only**, never in the hangar; reads are `Secret`-effect-gated. |
| U14 | D-JPK-IMAGE1 | **A** | `module image.server { kind: .Oci, from: packages.x }` → `jet image server` emits a distroless OCI image; `.Iso` emits a jetos installer. |
| U15 | D-JPK-FLEET1 | **A** | `module fleet.prod { hosts: { web1: system.web.{…} } }` → `jet push prod`: ssh deploy, health-gated staged rollout, per-host rollback. **Ratify surface now, realize post single-host jetos.** |
| U16 | D-JPK-BRIDGE1 | **A** | Three Nix flows verb-for-verb (`jet env -p`, `jet env` detects `flake.nix`, `jet run nixpkgs@tool`) + `jet bridge flake` export shim. Phase 1 shells out to `nix`. |
| U18 | D-JPK-TWONAMES1 | **A** (+amendment) | One reserved filename **`pack.jet`**; one command **`jet`**. Role modules (`env.*`/`system.*`/`fleet.*`/`build`) live in **any** `.jet` file, discovered by declaration via `find()`. Revises U10, D-WORKSPACE2, D-PM4. |
| U19 | D-JPK-DEVCOMPOSE1 | **D** | `jet env [name]` = shell only (base `env.*` + optional named overlay), **never executes project fns**, direnv-style trust gate on first entry. `jet dev`/`test`/`build` = explicit execution inside `env(base + env.<verb>)`. |
| U17 | D-JPK-OSNAME1 | **A** (2026-07-02) | OS product name is **jetos** — working title confirmed. Spell it `jetos` everywhere; trademark sweep stays a pre-release task. |

**U18 owner amendment (ratificationNote, verbatim intent — becomes an acceptance
test):** "only `pack.jet` is a reserved filename; every role module (`env.*`,
`system.*`, `fleet.*`, `build`) may live in any file the user chooses (e.g.
`build.jet`) and is discovered by declaration via `find()`, never by filename."

---

## Current reality (before you touch anything)

Much of the **jetos-tier** `system.*`/`image.*`/`Service` surface already
field-checks and captures (gap #5, E0972–E0978). The U11–U19 ballots layer the
**dev-env runtime** and the **rename** on top. Know the seam before coding:

- **Manifest = `pkg.jet` today**, constant `PAYLOAD_FILE` in
  `crates/jet-foundation/src/Syntax.rs:1332`. Parser:
  `crates/jet-driver/src/Jetpack/PackageManifest/` (`mod.rs`, `ParseBlocks.rs`
  parse `payload:`/`deps:`/`packages:`/`build:`). U18 renames this to `pack.jet`
  (see §U18 — this **reverses** the 2026-06-18 D-JPK-FILES filename choice; flag it).
- **Monorepo index = `workspace.jet`** (constant `WORKSPACE_FILE:1279`,
  `crates/jet-driver/src/Jetpack/WorkspaceFile.rs`, E0995–E0997) **plus a legacy
  `jetpack.toml`** (constant `JETPACK_TOML:1427`,
  `crates/jet-driver/src/Jetpack/ManifestTOML.rs`, `[repo]`/`[sources]`, E1214/E1215/E1225).
- **Module eval** = `crates/jet-driver/src/Jetpack/ModuleEval/` — `Source.rs`
  (`evaluate_env`), `System.rs` (`evaluate_system`/`evaluate_service`/`evaluate_image`),
  `Types.rs` (`EnvPlan`/`SystemPlan`/`ServicePlan`/`OptionPlan`/`ImagePlan`),
  `Diagnostics.rs` (E0966–E0978). Discovery walks all `.jet` files except
  `PAYLOAD_FILE`: `PackageManifest/Discovery.rs` `discover_module_in` +
  `WorkspaceFile.rs` `find_package_dirs`. **`find()`-by-declaration already exists** —
  U18's acceptance test rides it.
- **`ServicePlan` today = jetos flavor** (`enable: Bool` + display-string extras,
  captured for realize). U12's **supervised dev service** (version/port/init,
  health probe, `.jet/` data dir, `jet services` verbs) is **new runtime**, not
  the same code path. Same `Service` type, different lifecycle owner (ballot detail 1).
- **`ImagePlan` today** = `from`/`format∈{iso,qcow,raw}`/`target`, realized by the
  jetos tier. U14's `kind: .Oci` distroless container path + `jet image` verb +
  registry push is **new**.
- **Top-level `jet` verbs**: `crates/jet-foundation/src/... ` no — the registry is
  `Source/CLI.rs` `COMMANDS`/`FLAGS` (drives completions + man + typo-suggest).
  Present: run/check/test/build/dev/serve/env/add/remove/fetch/update/store/gc/…
  **Absent (this card adds): `services`, `secrets`, `image`, `switch`, `push`,
  `generations`, `rollback`, `bridge`, `lock`, `init`, `config`.** Dispatch is in
  `Source/main.rs`: `jet env` → in-process `jet::Jetpack::run(["enter", …])`
  (`main.rs:866`); git-style external `jet-<cmd>` fallback exists (`find_external`,
  `main.rs:513`, D-DX5).
- **jetpack engine CLI**: `crates/jet-driver/src/Jetpack/CLI.rs` (`cmd_run`/`enter`/
  `build`/`list`/`clean`/`add`/`remove`/`os`, lines 405–748). Store/realize:
  `Provider.rs`, `Store.rs`; shell compose: `Shell.rs`; lockfile: `.jet/lock`.
- **E-code families & free ranges** (confirm against `docs/spec/diagnostics.md`
  before assigning — I4): modeval `E09xx` (E0966–E0997 used; free: E0982–E0994
  except E0982/E0983 lib-use, E0998–E0999); manifest `E12xx` (used through E1225;
  free: E1207, E1216, E1218, E1220–1224, E1226+); jetos `E0979–E0981`; C-FFI `E32xx`.
  Proposed codes below sit in these families; **final numbers are assigned in
  `docs/spec/diagnostics.md` at implement time** and must not collide.

---

## Sequencing DAG

```
                     ┌─────────────────────────────────────────┐
  Phase A  U18  ─────┤ pack.jet rename + workspace/jetpack.toml │  (foundation:
  (do first)         │ fold + verb routing substrate            │   touches every
                     └───────────────┬─────────────────────────┘   manifest path)
                                     │
        ┌────────────────────────────┼───────────────────────────────┐
        ▼                            ▼                                ▼
  Phase B (parallel)
   U11 scriptdep            U19 env/dev split + trust gate      U14 image.oci
   (loader + lock,          (revises jet env / jet dev)         (+ jet image verb)
    independent)                   │                                  │
                                   ▼                                  ▼
                       Phase C (rides U19 activation)         U15 fleet SURFACE
                        U12 services (jet dev starts;         (parse+capture now;
                         jet services verbs)                   REALIZE gated on
                        U13 secrets (jet env decrypts)         jetos Phase 2 + U14)
                        U16 bridge (jet env -p / flake)
```

**Dependency edges (named):**
- **U18 → all.** The rename + module-by-any-file substrate is under every gate;
  land it first so later gates aren't rebased across a filename churn.
- **U19 → U12, U13, U16.** DEVCOMPOSE=D fixes *where* code runs: `jet dev` starts
  supervised services (U12) and runs `fn dev()`; `jet env` decrypts secrets (U13)
  into the shell and hosts the trust gate; `jet env -p`/foreign-flake (U16) are
  `jet env` variants. Build these against the settled env/dev boundary.
- **U12 ∥ U13**, but **U13 decrypt feeds U12 startup** (services read secrets from
  the activated env). Order U13 env-entry decryption before/with U12 supervisor.
- **U14 → U15.** Fleet ships **system/image closures**; per ballot it rides the
  jetos realize tier. Parse+capture `fleet.*` in Phase B alongside U14, but gate
  `jet push` realization on single-host jetos + U14's image realize.
- **U11 independent.** Rides the ratified `pkg#version` pin + `.jet/lock` + cache;
  no dependency on the module surface.

---

## U18 — the two-names rename (Phase A, foundation)

**Ratified semantics.** `pack.jet` is the only filename the tool looks for; `jet`
is the only command. Every role is a **module namespace** (`env.*`, `system.*`,
`fleet.*`, `build`, `workspace`) discovered by declaration in any `.jet` file via
`find()`, never by filename. `jetpack`/`jetos` stay as engines `jet` dispatches to.

**Rename / fold (clean break, no alias, teaching error — house pattern):**

1. `crates/jet-foundation/src/Syntax.rs:1332` — `PAYLOAD_FILE: "pkg.jet"` → `"pack.jet"`.
   Keep the `payload:` **block keyword** (`MANIFEST_BLOCK_PAYLOAD`) unchanged; only
   the **filename** moves. Update the doc comments at 1105/1140/1329.
2. Fold **`workspace.jet`** into discovery: `module workspace { members: … }` is
   found in `pack.jet` or any `.jet` file, not a dedicated `WORKSPACE_FILE`. Rework
   `WorkspaceFile.rs::load` from "read `dir/WORKSPACE_FILE`" to "scan discovered
   files for `module workspace`" (reuse `Discovery::file_declares_module`). Retire
   the `WORKSPACE_FILE` constant (:1279).
3. Retire **`jetpack.toml`** entirely: `[repo]`/`[sources]` fold into `pack.jet`
   blocks; delete `ManifestTOML.rs` wiring from `CLI.rs::load_project_plan`. Keep
   E1225's teaching direction (`[packages]` → `module workspace`).
4. **`env.jet`/`config.jet` demote to blessed conventions**: keep `ENV_FILE`/
   `CONFIG_FILE` only as `jet new` scaffold hints; discovery must find `env.*`/
   `system.*` modules regardless of filename (already true — `discover_module_in`
   and the modeval walk scan all `.jet` except `PAYLOAD_FILE`).
5. **Verb routing**: route the new engine verbs (`services`/`secrets`/`image`/
   `switch`/`push`/`generations`/`rollback`/`bridge`) through the existing
   `jet env`→`jet::Jetpack::run` in-process dispatch (`main.rs:866`) + `Source/CLI.rs`
   `COMMANDS`. A verb needing a role module absent from scope is a teaching error
   (`jet switch` with no `system.*`). NB: full "compiler links nothing, execs
   `jetpack`/`jetos` binaries" separation is a **larger architectural item** — today
   `Jetpack` is a linked crate; do NOT block U11–U19 on binary separation, use the
   in-process path + the D-DX5 `jet-<cmd>` fallback that already exists.

**New diagnostics (I4):**
- **E1226** `old-manifest-filename`. what: "``pkg.jet`` is no longer the manifest
  name — Jet reads ``pack.jet``". why: "one reserved filename (U18); prior names
  were retired in a clean break". fix: "rename ``pkg.jet`` to ``pack.jet``". Same
  code covers `payload.jet`/`jet.toml`/`jetpack.toml` (name in the message).
  Fixture: `tests/ui/manifest_old_filename.stderr` (runtime, no source span →
  inline-snapshot in the loader test, mirroring the `ManifestTOML` pattern noted
  in IMPLEMENTATION-STATUS).
- **E1227** `verb-needs-role-module`. what: "nothing here declares a machine —
  ``jet switch`` needs a ``system.<name>`` module". why: "verbs resolve against
  declared module namespaces (U18)". fix: "add ``module system.<name> { … }`` in
  any ``.jet`` file — see ``jet help switch``". Fixture:
  `tests/ui/switch_no_system.stderr`.

**Acceptance test (owner amendment — REQUIRED):** a repo where `module system.laptop
{ … }` lives in an arbitrarily-named file (`machine-config.jet`) and `module
env.dev { … }` lives in `toolchain.jet` — assert both are discovered and their
verbs resolve, and that only `pack.jet` is treated as reserved. Add to
`tests/modules.rs` (or `tests/workspace.rs`): `role_module_in_any_file_is_discovered`.

**Examples:** `manifest/pack_workspace.jet` (root `pack.jet` carrying `payload:` +
`deps:` + inline `module workspace { members: find("./packages") }`).

**Targeted tests:** `tests/pkg.rs` (rename all `pkg.jet` fixtures + the manifest
parse tests), `tests/workspace.rs` (workspace-in-`pack.jet` fold), `tests/jetpack.rs`
(verb routing), plus re-bless `tests/cli/completions_{bash,zsh,fish}.txt` +
`tests/cli/man.txt` and all `pkg.jet`→`pack.jet` stderr snapshots.

**Exit:** `pack.jet` is the sole reserved name; `pkg.jet`/`jetpack.toml`/`payload.jet`
fire E1226; `module workspace`/`env.*`/`system.*` are discovered from any file;
completions/man re-blessed; full suite green.

---

## U11 — inline script dependencies (Phase B, independent)

**Ratified semantics.** A bare single-file script may carry `use pkg#version`
(the already-ratified ref pin). `jet run script.jet` resolves + locks (lock lives
in the **cache, keyed by file-content hash** — user-invisible) + runs. Bare `use
pkg` (no pin) resolves latest-compatible and records the choice in the cache lock
with a hint to add the pin. `jet lock script.jet` writes a `script.jet.lock`
sidecar for committed reproducibility (`jet run` prefers it when present). `jet
init` lifts inline `use`s into a generated `pack.jet` `deps:` block. No install-time
code execution (PM-I1) — identical to package resolution.

**Surface.** No manifest; the pin rides `use`:
```jet
// analyze.jet — the only file on disk
use textkit#1.4
use core.fs as fs
fn run() { loop line in fs.read("app.log")?.lines() { print(textkit.truncate(line, 100)) } }
```

**Driver touch-points.**
- `Source/Loader.rs` — when loading a manifest-less entry file, collect `use <pkg>#<ver>`
  refs (parser already yields them; wire the pin through). Resolve via
  `crates/jet-driver/src/Jetpack/RefSpec.rs` + `Provider.rs` into the hangar.
- Cache lock: new per-file-hash lock under `.jet/cache/` (reuse `Store.rs` roots);
  sidecar `<file>.lock` reader/writer.
- `Source/CLI.rs` `COMMANDS`: add `lock` ("write a lock sidecar next to a script")
  and `init` ("lift a script's inline deps into a new pack.jet"). Dispatch in
  `Source/main.rs`.

**New diagnostics (I4):**
- **E1228** `inline-dep-unresolved`. what: "no package ``textkit`` matches
  ``#1.4``". why: "an inline ``use pkg#version`` resolves against the registry
  (U11)". fix: "check the name/version, or remove the pin to take the latest".
  Fixture: `tests/ui/script_dep_unresolved.jet` + `.stderr`.
- **L02xx (warning, not error)** `inline-dep-unpinned` — bare `use pkg` in a script
  resolved to a concrete version; suggest adding `#<ver>`. Warning shape per
  diagnostics.md. Fixture: `tests/ui/script_dep_unpinned.jet` + `.stderr`.

**Examples:** `scripting/inline_deps.jet` (+ expected stdout, golden-enforced I5).

**Targeted tests:** new cases in `tests/jetpack.rs` — `script_inline_dep_resolves`,
`jet_lock_writes_sidecar`, `jet_init_lifts_uses_into_pack_jet`. Use captured
provider fixtures (offline) per the existing determinism pattern.

**Exit:** a one-file script with `use pkg#ver` runs offline against a fixture;
`jet lock` sidecar round-trips; `jet init` emits a valid `pack.jet`; PM-I1 holds.

---

## U19 — env/dev split + trust gate (Phase B, substrate for C)

**Ratified semantics (outcome D).** `jet env [name]` realizes the environment
(base `env.*` plus optional named overlay, U5 merge) and opens a shell — **no
project function ever runs from env entry**. `jet dev` is the explicit execution
verb: realize `env(base + env.dev)`, wait for services ready, then run `fn dev()`
(fallback `fn run` under watch/reload). `jet test`/`jet build` use `env.test`/
`env.build` the same way. Because `on_enter` hooks and service defs are
project-authored code, the **first `jet env` in a fresh repo shows a trust summary
(hooks, services, sources) and requires a direnv-style allow**; the grant is
recorded and re-prompted when the env definition **hash** changes. `jet env --trust`
bypasses in one shot (CI/scripts/known sources); `jet config trust add <pattern>`
pre-trusts sources by pattern. `jet dev` in an untrusted repo is the user's stated
intent to run code — **no gate**.

**Driver touch-points.**
- `Source/main.rs:866` — split the current `jet env`→`jetpack enter` delegation:
  `jet env` must realize + open shell WITHOUT invoking any `fn`/`on_enter` until the
  trust grant clears. `jet dev` (currently `Source/CmdDevTools.rs` watch/re-run) gains
  the env-overlay compose + service-wait + `fn dev()` path.
- `Source/CLI.rs` `COMMANDS`: revise `env` summary ("enter the project dev shell —
  tools only, runs nothing"); add `config` (`trust add|list|remove`). Add
  `--trust` to `FLAGS`.
- Trust store: `~/.jet/trust` (patterns) + per-repo grant record keyed by env-def
  hash. New module e.g. `crates/jet-driver/src/Jetpack/Trust.rs`.
- Env compose + overlay merge already in `ModuleEval` (U5) + `Shell.rs`.

**New diagnostics / prompts (I4).** The trust prompt is interactive UX, not a
diagnostic; snapshot the non-interactive `--trust`/denied paths:
- **E1230** `dev-no-entry`. what: "``jet dev`` found no ``fn dev`` or ``fn run``".
  why: "``jet dev`` runs the project's dev entry inside the dev env (U19)". fix:
  "add ``fn dev() { … }`` (or ``fn run``)". Fixture: `tests/ui/dev_no_entry.stderr`.
- **E1231** `env-untrusted` (non-interactive/`--json` path only). what: "this
  environment defines hooks/services and hasn't been trusted". why: "entering an
  env realizes project-authored code; first entry needs an explicit allow (U19)".
  fix: "run ``jet env --trust``, or ``jet config trust add <pattern>``". Fixture:
  `tests/ui/env_untrusted.stderr`.

**Security note.** `jet env` may realize the env and set PATH/env-vars/**decrypt
secrets** (see U13) into the shell, but MUST NOT execute `on_enter`, service
commands, or any project `fn` before the trust grant. The gate re-arms on env-def
hash change.

**Examples:** `env/dev_split.jet` (`module env.dev {}` + `module env.test {}` +
`fn dev()`), showing `jet env` (shell) vs `jet dev` (executes).

**Targeted tests:** `tests/jetpack.rs` — `env_entry_runs_no_project_fn`,
`dev_runs_fn_dev_after_services`, `env_first_entry_requires_trust`,
`env_trust_flag_bypasses`, `config_trust_add_pretrusts_pattern`.

**Exit:** `jet env` opens a shell and provably runs no project fn (assert via a
tell-tale side-effect fn that must NOT fire); `jet dev` runs `fn dev()` after
services are ready; trust gate blocks untrusted first entry and records the grant.

---

## U12 — typed supervised services (Phase C, after U19)

**Ratified semantics (A).** A service is a typed struct under an `env.*` module's
`services:` map; fields are its options (editor completion + docs). Core ships a
common set (postgres, redis, mysql, nats, minio…); any package may define a
`Service` type. jetpack **supervises**: `up`/`down`/`health`/`logs`, health-gated
startup (`jet dev` waits for ready before `fn dev()`), project-scoped processes
with sockets/data under `.jet/` (never system daemons — jetos system services stay
in `system.*`, same type, different lifecycle owner).

**Surface.**
```jet
module env.dev {
    services: {
        postgres: Service.{ version: 16, port: 5432, init: "schema.sql" }
        redis: Service.{}
    }
}
```
```
$ jet dev                     # services up, health-checked, then fn dev()
$ jet services logs postgres
$ jet services down
```

**Driver touch-points.**
- Extend the **dev-env** service surface (distinct from the jetos `ServicePlan` in
  `ModuleEval/System.rs`): a dev `Service` under `env.*` with a readiness contract
  (port/probe), version, ports, init script, shutdown hook. Add a
  `DevServicePlan` to `ModuleEval/Types.rs` + field-check in a new
  `ModuleEval/Service.rs` (mirror `System.rs::evaluate_service`, but the dev flavor:
  no required `enable`; typed known fields per built-in service + open extras for
  user types).
- Supervisor runtime: new `crates/jet-driver/src/Jetpack/Services.rs` — spawn under
  `.jet/services/<name>/` (data + socket + pidfile), health poll, log capture to
  `.jet/services/<name>/log`. Std-only (`std::process`, no external crate).
- Built-in service catalog: realize the service binary via the existing provider
  (nixpkgs today) — `postgres_16` etc. are just `Pkg`s; the catalog maps a
  `Service` name to its package + default probe.
- `Source/CLI.rs` `COMMANDS`: add `services` (`up|down|logs|health <name>`).
  `jet dev` (U19) calls the supervisor before `fn dev()`.

**New diagnostics (I4):**
- **E0984** `service-health-timeout` (runtime; inline snapshot). what: "the service
  ``postgres`` did not become ready within 60s". why: "``jet dev`` health-gates
  startup so the app never races the database (U12)". fix: "check ``jet services
  logs postgres``; raise the readiness timeout if the service is slow".
- **E0985** `unknown-service-field`. what: "``prot`` isn't a field of the
  ``postgres`` service". why: "a built-in ``Service`` has typed fields (U12)". fix:
  "did you mean ``port``?". Fixture: `tests/ui/service_unknown_field.jet` + `.stderr`.

**I6 watch:** none — supervision is std `std::process`. Built-in DB binaries come
from the provider (nixpkgs, already sanctioned as the interim native-dep source).

**Examples:** `env/services_postgres.jet` (+ a `jet services` transcript; the
runtime is non-golden, so assert via a scripted test not example stdout).

**Targeted tests:** new `tests/services.rs` — `dev_service_field_check`,
`supervisor_up_down_roundtrip` (fixture "service" = a trivial sleep/echo daemon so
CI needs no real postgres), `health_gate_blocks_until_ready`,
`services_logs_captures_output`.

**Exit:** an `env.dev` with a fixture service field-checks, comes up under `.jet/`,
health-gates `jet dev`, and `jet services down` cleans up; user-defined `Service`
types work; auditors can grep `services:`.

---

## U13 — first-class encrypted secrets (Phase C, after U19)

**Ratified semantics (A).** An age-style secrets file encrypted to a **recipients**
list lives in the repo. `secret("name")` references an entry. Decryption happens
**at env entry / service start, in memory, never written to the hangar**. Reading a
secret from code requires the **`Secret` effect** (audit = grep `#(Secret)`). `jet
secrets set` prompts + encrypts; `jet secrets recipients add <key.pub>` re-encrypts.
Base tier needs no vault; vault/op/kms are provider **adapters** for enterprises.

**Surface.**
```jet
module env.dev { secrets: { STRIPE_KEY: secret("stripe-dev"), DB_PASS: secret("db-dev") } }

fn charge() -> Receipt ? Error #(Net, Secret) {
    key :: secrets.get("stripe-dev")?    // effect-gated read
}
```
```
$ jet secrets set stripe-dev            # prompts, encrypts to recipients
$ jet secrets recipients add karl.pub
$ jet dev                                # decrypted into the env, memory only
```

**Driver touch-points.**
- Secrets store: encrypted file in-repo (e.g. `secrets.jet.age` or `.jet/secrets`)
  + a `recipients` list. New `crates/jet-driver/src/Jetpack/Secrets.rs`
  (set/get/recipients, encrypt/decrypt).
- `secrets:` field on `env.*`/`system.*` modules → capture `secret("name")` refs in
  `ModuleEval` (new field in the env/`SystemPlan` surface); resolve to decrypted
  values at env activation only.
- **`Secret` effect**: add to the effect vocabulary — ballot says "effect #11 or
  ride `Env`; separate is better for D-EFFBUDGET1 budgets" → treat as a **new
  effect `Secret`** in `crates/jet-sema` effect tracking; a `secrets.get` call
  carries it; build tier denies it by default (D-BUILDPOLICY1).
- `Source/CLI.rs` `COMMANDS`: add `secrets` (`set|get|recipients <sub>`).
- `jet env` (U19) decrypts into the shell env; `jet dev`/service start read from
  the activated env.

**Security notes (load-bearing).**
- **Never plaintext at rest**: only the encrypted file + public recipients are
  committed. Private keys live outside the repo (user/CI key path). Decrypt output
  exists only in process memory / the entered shell's env — **never a hangar object,
  never `.jet/lock`, never a temp file**. Hangar artifacts must be secret-free by
  construction; a build that reads a secret fails (Secret effect denied in build tier).
- **Per DEVCOMPOSE=D trust gate**: `jet env` MAY decrypt secrets into the shell env
  (that is env realization, not code execution) but MUST NOT run project code before
  the trust grant. A service's use of a secret happens at `jet dev` (explicit
  execution). CI uses a machine key + `--trust`.
- **Rotation** = recipients-list edit + re-encrypt; removing a leaver's key rotates
  them out (ballot story). Effect-gated reads make "what can exfiltrate keys" a grep.

**I6 watch — OWNER BALLOT REQUIRED.** age-style crypto needs X25519 +
ChaCha20-Poly1305 (+ scrypt/bech32). Zero external crates in `Source/` is I6.
Candidates to ballot as **D-JPK-SECRETCRYPTO1**:
  (a) a vetted crypto crate behind the stdlib FFI-bridge pattern (see
      `ffi-bridge-stdlib-pattern` memory / `Archive.rs`/`Db.rs`/`CFFI.rs`) — emit a
      bridge template, never add to jet's `Cargo.toml`;
  (b) shell out to the `age`/`rage` binary (nix-bridge-style stopgap, requires it
      installed, clear message otherwise);
  (c) native std-only X25519 + ChaCha20-Poly1305 (Epoch-3 "replace external deps"
      mandate; hardest, most I6-pure).
**Do not pick one — raise the ballot with these options + worked examples; build
the non-crypto scaffolding (surface, effect, store format, CLI, trust integration)
meanwhile so only the cipher is gated.**

**New diagnostics (I4):**
- **E0986** `secret-missing-entry`. what: "no secret named ``stripe-dev``". why:
  "``secret("name")`` references an entry in the encrypted secrets file (U13)". fix:
  "run ``jet secrets set stripe-dev``". Fixture: `tests/ui/secret_missing.jet` + `.stderr`.
- **E0987** `secret-read-ungranted`. what: "reading ``stripe-dev`` needs the
  ``Secret`` effect". why: "secret reads are effect-gated so exfiltration is
  auditable (U13)". fix: "add ``#(Secret)`` (or ``#(…, Secret)``) to the function's
  effect row". Fixture: `tests/ui/secret_ungranted.jet` + `.stderr`.
- **E0988** `secret-in-build`. what: "a build step read the secret ``db-dev``".
  why: "the build tier denies ``Secret`` so no key reaches the hangar (U13/D-BUILDPOLICY1)".
  fix: "resolve secrets at env/service start, not at build". Fixture:
  `tests/ui/secret_in_build.jet` + `.stderr`.

**Examples:** `env/secrets.jet` (declares `secrets:` + an effect-gated reader).

**Targeted tests:** new `tests/secrets.rs` — `secret_missing_is_e0986`,
`secret_read_requires_effect_e0987`, `build_tier_denies_secret_e0988`,
`decrypt_never_writes_plaintext` (assert no plaintext hits `.jet/`), plus a
round-trip gated behind whatever SECRETCRYPTO1 provides (stub with a fixture cipher
until ratified).

**Exit:** `secrets:` + `secret()` field-check; the `Secret` effect gates reads and
is denied in build; decrypt is memory-only and provably never persisted; CLI
set/recipients round-trip (once the crypto ballot lands). Until then: everything
except the cipher is complete and tested, cipher named as the gate.

---

## U14 — `image.*` OCI containers + ISOs (Phase B)

**Ratified semantics (A).** An `image` module names inputs (a package target, env
slices, files) and outputs (`kind: .Oci` | `.Iso`). Content-addressed hangar makes
images layered + reproducible. `.Oci` = distroless by default (closure of the named
target only; ad-hoc files are explicit `files: […]`), built directly from hangar
objects (no Docker dependency); `--push` speaks the registry protocol. `base:
oci("debian:12")` is the escape hatch. `.Iso` = a jetos installer (rides the jetos
realize tier already stubbed via gap #4). `jet audit image` lists every path + origin.

**Surface.**
```jet
module image.server { kind: .Oci, from: packages.pulseops, expose: [8080], env_vars: { RUST_LOG: "info" } }
```
```
$ jet image server                              # -> hangar:…/pulseops-oci-…  (distroless, reproducible)
$ jet image server --push ghcr.io/acme/pulseops:0.4.0
```

**Driver touch-points.**
- Extend `ImagePlan` (`ModuleEval/Types.rs`) + `evaluate_image` (`ModuleEval/System.rs`):
  add `kind` (`.Oci`/`.Iso`), `from: packages.<name>` (not just `system.<name>`),
  `expose`, `env_vars`, `files`, `base`. Today `from` is `system.<name>` only and
  formats are iso/qcow/raw — reconcile: `.Oci` builds from a **package target**,
  `.Iso` from a **system** (keep the existing system path for ISO).
- OCI builder: new `crates/jet-driver/src/Jetpack/Image.rs` — assemble an OCI layout
  (tar layers + config JSON + manifest) from the target's hangar closure; `--push`
  = registry v2 protocol.
- `Source/CLI.rs` `COMMANDS`: add `image` (`<name> [--push <ref>]`). `jet audit
  image` rides the existing `audit` verb.

**I6 watch — OWNER BALLOT.** OCI build needs sha256 (digest) + gzip (layers);
`--push` needs HTTPS/TLS. None exist std-only in `Source/`. Ballot
**D-JPK-OCITOOL1**: (a) shell out to `skopeo`/`nix dockerTools` (stopgap, like the
nix bridge); (b) FFI-bridge to an OCI/registry lib; (c) native (sha256 is
std-implementable; TLS is the hard part). Flag; build the plan/capture surface
meanwhile. **ISO realize stays gated on jetos Phase 2** (already so).

**New diagnostics (I4):**
- **E0989** `image-unknown-kind`. what: "``.Vm`` isn't an image kind". why: "v1
  builds ``.Oci`` (containers) or ``.Iso`` (installers) (U14)". fix: "use ``kind:
  .Oci`` or ``kind: .Iso``". Fixture: `tests/ui/image_unknown_kind.jet` + `.stderr`.
- **E0990** `oci-from-non-executable`. what: "an OCI image needs an executable
  ``from:``, but ``pulseops`` is a library". why: "a container ships a binary + its
  closure (U14)". fix: "point ``from:`` at an ``executable`` package target".
  Fixture: `tests/ui/oci_from_library.jet` + `.stderr`.

**Examples:** `image/oci_server.jet`, `image/iso_installer.jet`.

**Targeted tests:** `tests/jetpack.rs` (or new `tests/image.rs`) —
`image_oci_field_check`, `oci_from_library_is_e0990`, `oci_layout_is_reproducible`
(same inputs → identical digest; gate the real build behind OCITOOL1, field-check
now).

**Exit:** `image.*` with `kind`/`from`/`expose`/`env_vars` field-checks and
captures; the OCI build (once OCITOOL1 lands) yields a reproducible distroless
layout from the hangar closure; `--push` targets a registry; ISO stays jetos-gated.

---

## U15 — `fleet.*` multi-host deploy (Phase B surface / Phase D realize)

**Ratified semantics (A).** `fleet.<name>` is a map of hostname → `system` value;
per-host differences are **copy-with-update** on a shared `system` module (no
overlay spaghetti). `jet push <fleet>` builds each host closure, ships to each
host's hangar over ssh, activates atomically, health-probes, and rolls back any
host that fails; unchanged hosts (same closure hash) are skipped. **Ratify the
surface now so `system.*` design accounts for it; implement realize after
single-host jetos works.**

**Surface.**
```jet
module fleet.prod {
    hosts: {
        web1: system.web.{ region: "us-east" }
        web2: system.web.{ region: "eu-west" }
        db:   system.database.{ replicas: 2 }
    }
    rollout: Rollout.{ stage: 1, health_timeout: 60s }
}
```
```
$ jet push prod    # web1: build✓ ship✓ activate✓ health✓ ; db: unchanged, skipped
```

**Driver touch-points.**
- **Phase B (now):** parse + field-check + capture `fleet.<name>` — new
  `FleetPlan`/`HostPlan`/`RolloutPlan` in `ModuleEval/Types.rs`, `evaluate_fleet`
  in `ModuleEval/System.rs` (each host value is a copy-with-update on a captured
  `SystemPlan`, so reuse the `image_from_unknown_system` cross-check shape for
  `system.<name>` references). Add `push` to `Source/CLI.rs` `COMMANDS` (errors with
  a "gated on jetos Phase 2" teaching message until realize lands).
- **Phase D (gated on single-host jetos + U14 realize):** the ssh deploy /
  staged-rollout / per-host rollback engine, rides jetos generations (c27).

**New diagnostics (I4):**
- **E0991** `fleet-unknown-system`. what: "host ``web1`` is built from an unknown
  system ``web``". why: "a fleet host names a ``system.<name>`` some module defines
  (U15)". fix: "define ``module system.web { … }``, or point the host at an existing
  system". Fixture: `tests/ui/fleet_unknown_system.jet` + `.stderr`.
- **E0992** `push-gated` (until Phase D; runtime inline snapshot). what: "``jet
  push`` needs single-host jetos, which isn't built yet". why: "fleet deploy rides
  the jetos realize tier (U15)". fix: "the surface is ratified; realize lands after
  single-host ``jet switch`` works".

**Examples:** `fleet/prod.jet`.

**Targeted tests:** `tests/modules.rs`/`tests/jetpack.rs` — `fleet_field_check`,
`fleet_unknown_system_is_e0991`, `host_copy_with_update_overrides` (assert per-host
`region:`/`replicas:` override the base system).

**Exit (this card):** `fleet.*` parses, field-checks, captures, and cross-checks
host→system references; `jet push` gives the honest gated message. Realize is a
follow-on jetos card.

---

## U16 — the Nix bridge (Phase C, rides U19)

**Ratified semantics (A).** Three daily Nix flows, verb-for-verb: (1) ad-hoc tools
`jet env -p nodejs ripgrep` (subshell with those packages from the default
source); (2) repo shells `jet env` in a repo with `flake.nix`/`devenv.nix` and no
`env.*` modules → realize the foreign devShell, prompt + lock jet-side; (3)
ephemeral `jet run nixpkgs@fastfetch`. Export: `jet bridge flake` writes a
`flake.nix` shim exposing this repo's packages/env to Nix consumers. **Phase 1
shells out to the `nix` binary** (ratified stopgap; requires Nix installed, clear
message otherwise). Detection order: `env.*` modules win over `flake.nix`;
`--flake` forces foreign. `devenv.nix` consumption limited to mappable
packages/env/services fields — unmappable fields produce a **named warning**, not
silence. Foreign inputs recorded in `.jet/lock` with narHashes.

**Surface.**
```
$ jet env -p nodejs_22 ripgrep     # nix-shell -p parity, typed refs, jet prompt
$ jet env                          # flake.nix detected -> its devShell
$ jet run nixpkgs@fastfetch        # nix run parity, nothing installed
$ jet bridge flake                 # writes flake.nix shim for Nix users
$ jet env --pure                   # only declared packages
```

**Driver touch-points.**
- `jet env` (U19) gains: `-p <pkgs…>` ad-hoc mode; foreign-devshell detection
  (`flake.nix`/`devenv.nix` present + no `env.*` module) → shell out to `nix develop`;
  `--flake`/`--pure` flags. `jet run nixpkgs@<tool>` ephemeral path already partly
  exists via the nix provider (`Provider.rs`) — wire the run verb to it.
- `jet bridge flake` export: new `crates/jet-driver/src/Jetpack/Bridge.rs` — emit a
  generated (never hand-edited, CI-drift-checkable) `flake.nix` shim from the
  `pack.jet` package/env surface.
- `Source/CLI.rs` `COMMANDS`: add `bridge` (`flake`); add `-p`, `--flake`, `--pure`
  to `FLAGS` (or as `env`-scoped flags). Missing-`nix` message reuses the existing
  JPK-1 missing-nix diagnostic.
- Lock: fold foreign narHashes into `.jet/lock` (`Store.rs`).

**New diagnostics (I4):**
- **E0993** `bridge-no-nix`. Reuse/extend the existing missing-`nix` diagnostic:
  "``jet env`` needs the ``nix`` binary to realize a ``flake.nix`` devShell". fix:
  "install Nix, or declare an ``env.*`` module". Fixture: inline snapshot in
  `tests/jetpack.rs` (runtime).
- **L02xx (warning)** `devenv-unmappable-field` — a `devenv.nix` field jet can't map
  (named, never silent). Fixture: `tests/ui/` warning snapshot or inline.

**I6 watch:** none new — shells out to the already-sanctioned `nix` binary (the
ratified Phase-1 stopgap). No crate added.

**Examples:** `bridge/flake_export.jet` (a `pack.jet` + the emitted `flake.nix`
checked as a golden export).

**Targeted tests:** `tests/jetpack.rs` — `env_dash_p_composes_adhoc_shell`
(fixture provider), `env_detects_flake_when_no_env_module`,
`bridge_flake_export_is_stable` (idempotent, drift-checkable), `env_modules_win_over_flake`.

**Exit:** `jet env -p`, foreign-flake detection (env.* precedence), ephemeral
`jet run nixpkgs@`, and `jet bridge flake` export all work through the `nix`
stopgap; unmappable devenv fields warn; foreign inputs land in `.jet/lock`.

---

## Cross-cutting checklist

- **`tests/decisions.rs` (ratification enforcement):** register all 8 ratified
  decisions (D-JPK-SCRIPTDEP1/SERVICE1/SECRET1/IMAGE1/FLEET1/BRIDGE1/TWONAMES1=A,
  DEVCOMPOSE1=D). This gate must stay green (Task-zero contract).
- **Golden CLI surface:** every new verb (`services`/`secrets`/`image`/`switch`/
  `push`/`generations`/`rollback`/`bridge`/`lock`/`init`/`config`) + new flags
  re-bless `tests/cli/completions_{bash,zsh,fish}.txt` and `tests/cli/man.txt`
  (`env UPDATE_EXPECT=1`).
- **Formatter round-trip (house rule):** any new manifest/module syntax
  (`kind: .Oci`, `secrets:`, dev `services:`, `fleet.*`, `Rollout.{}`) needs
  formatter emission + a `tests/fmt.rs` STABILITY test — idempotence alone misses
  dropped tokens.
- **Syntax.rs (I7):** every new user-typeable keyword/sigil (`services`, `secrets`,
  `secret`, `fleet`, `hosts`, `rollout`, `image` kind idents, `Rollout`, trust
  patterns) gets a `crates/jet-foundation/src/Syntax.rs` constant with its decision ID.
- **spec.md:** extend the jetpack sections (`docs/spec/spec.md:984` System/Service/
  Image; `:1033` jetos tier) with the dev-env service/secret/image-OCI/fleet/bridge
  surface + the `pack.jet` rename. Migrate the superseded filename guidance in
  `payload-env-separation.md` (it argues separate `pkg.jet`/`env.jet` files; U18
  moves that separation to namespaces) — confirm scope with the owner before deleting.
- **diagnostics.md (I4):** every E-code above gets a what/why/fix entry + a
  `tests/ui` snapshot (or an inline runtime snapshot where there's no source span) —
  no snapshot, no diagnostic. Confirm final numbers don't collide with the used
  ranges listed in "Current reality".
- **I6 ballots to raise NOW (do not assume):** **D-JPK-SECRETCRYPTO1** (age-style
  cipher provider) and **D-JPK-OCITOOL1** (OCI build/push backend). Both block only
  the cipher/build step, not the surrounding surface. Raise them ballot-ready in
  `tools/Tower/tower.json` (per the `owner-gates-must-be-ballots` rule) before the
  crypto/OCI code, with the option menus in §U13/§U14.
- **OSNAME (U17) ratified 2026-07-02:** the name is `jetos`, final.

---

## Suggested landing order

1. **U18** (rename + fold + verb substrate + acceptance test) — unblocks clean paths.
2. **U19** (env/dev split + trust) — the activation substrate.
3. **U11** (scriptdep) in parallel with U19 (independent).
4. **U14** (image OCI surface + builder-gated) + **U15** (fleet surface) — parallel.
5. **U12** (services) + **U13** (secrets) on the U19 substrate — U13 decrypt before
   U12 startup.
6. **U16** (bridge) on the U19 `jet env` variants.
7. Raise SECRETCRYPTO1 + OCITOOL1 ballots at the start of steps 4/5, not the end.
