# Epoch 4 — implementation plan

**Card:** `c9jetpackgates`. **Updated:** 2026-07-02. This is the HOW. Read
[`README.md`](README.md) for canon/status and [`vision.md`](vision.md) for the
target UX.

Run repo commands through Nix (`nix develop -c ...`). Do not add external crates
to the compiler. Every new diagnostic needs `docs/spec/diagnostics.md` and a
snapshot or pinned inline runtime rendering.

---

## Phase A — foundation first

This phase reconciles shipped code with today's decisions before adding more
surface.

### A1. Process dispatch (`D-JPK-DISPATCH1=B`)

Goal: `jet` dispatches engine verbs to executable boundaries now, not to
`jet::Jetpack::run`.

Work:

1. Define the process contract for `jet -> jetpack` and `jet -> jetos`:
   exit codes, stderr/stdout ownership, `--json` diagnostics, version-skew
   error, and environment propagation.
2. Move existing `jet env` delegation off the in-process path.
3. Keep the D-DX5 `jet-<cmd>` PATH fallback for third-party commands, but engine
   verbs resolve to known engine binaries first.
4. Add tests for exit-code propagation, missing engine binary, `--json`
   preservation, and typo suggestions.

Diagnostics:

- `E1227` `engine-version-skew`: `jet` and `jetpack`/`jetos` protocol versions
  disagree. Fix: use matching tool versions.
- `E1228` `engine-missing`: engine verb needs an engine binary that is not on
  PATH. Fix: install the matching Jet toolchain.

Exit: the compiler binary is standalone-checkable; U11-U19 verbs cross the
process seam.

### A2. Filename canon (`D-JPK-FILENAME2=B`)

Goal: keep `pkg.jet`; remove stale `pack.jet` implementation instructions.

Work:

1. Ensure `PAYLOAD_FILE` / equivalent remains `"pkg.jet"`.
2. Remove or update docs/tests that teach `pack.jet` as current.
3. Treat `pack.jet`, `payload.jet`, `jet.toml`, and `jetpack.toml` as retired
   names with a teaching error pointing to `pkg.jet`.
4. Fold `workspace.jet` semantics into role-module discovery: `module workspace`
   can live in `pkg.jet` or any discovered `.jet` file.

Diagnostic:

- `E1226` `old-manifest-filename`: "`pack.jet` is not the package manifest name;
  Jet reads `pkg.jet`." Fix: rename to `pkg.jet`.

Acceptance test: a repo with `module workspace`, `module env.dev`, and
`module system.laptop` in arbitrary file names is discovered; only `pkg.jet` is
reserved.

### A3. Module declaration role form (`D-JPK-MODBODY1=A`)

Goal: canonicalize role modules as declaration names:
`module env.dev { fields }`.

Work:

1. Parser accepts dotted role names in module declarations:
   `env.<name>`, `system.<name>`, `image.<name>`, `fleet.<name>`, `workspace`,
   and future `build`.
2. Desugar canonical declarations into the existing contribution IR so the merge
   engine can be reused.
3. Emit a teaching diagnostic for the old form
   `module dev { env.dev: Env.{ ... } }`; do not keep both as legal stable
   spellings.
4. Formatter emits the canonical form and has a stability test.

Diagnostic:

- `E1229` `old-role-contribution-form`: role namespace belongs in the module
  declaration name. Fix: `module env.dev { ... }`.

Exit: every example in this folder parses as written.

---

## Phase B — independent surfaces

### U11. Inline script dependencies

Semantics:

- Bare script may use `use pkg#version`.
- `jet run script.jet` resolves, locks by file-content hash, and runs.
- `jet lock script.jet` writes `script.jet.lock`.
- `jet init` lifts inline deps into generated `pkg.jet`.
- No install-time code execution.

Touch points:

- Loader collects inline package refs for manifest-less entry files.
- Resolver uses `RefSpec` / providers / hangar.
- Cache lock under `.jet/cache/` keyed by file hash; sidecar lock reader/writer.
- Add `lock` and `init` to CLI registry and dispatch contract.

Diagnostics:

- `E1230` `inline-dep-unresolved`.
- `L02xx` `inline-dep-unpinned`.

Tests:

- `script_inline_dep_resolves` with offline provider fixture.
- `jet_lock_writes_sidecar`.
- `jet_init_lifts_uses_into_pkg_jet`.

### U14. Images

Semantics:

- `module image.name { kind: .Oci, from: packages.x }` builds deterministic OCI
  layout from hangar objects.
- `.Iso` rides the jetos installer tier.
- `--push` is gated on TLS; temporary `skopeo` bridge may exist but is not the
  core image contract.

Touch points:

- Extend `ImagePlan`: `kind`, package `from`, system `from` for ISO, `expose`,
  `env_vars`, `files`, `base`.
- New native OCI layout builder: layer tar, gzip, sha256, config JSON, manifest.
- `jet image <name>` via engine dispatch.

Diagnostics:

- `E0990` `image-unknown-kind`.
- `E0991` `oci-from-non-executable`.
- `E0992` `image-push-gated-on-tls`.

Tests:

- field-check capture;
- library source rejected for OCI;
- reproducibility vector: same input -> same digest;
- push path emits gated message until TLS lands.

### U15. Fleets

Semantics:

- `module fleet.prod { hosts: { web1: system.web.{ ... } } }`.
- Parse/capture/cross-check now.
- Real ssh deploy waits for single-host jetos and image/closure realization.

Touch points:

- `FleetPlan`, `HostPlan`, `RolloutPlan`.
- Cross-check host system refs.
- `jet push <fleet>` command gives honest gated message until Phase D.

Diagnostics:

- `E0993` `fleet-unknown-system`.
- `E0994` `push-gated`.

Tests:

- field-check fleet;
- unknown system diagnostic;
- copy-with-update override capture.

### U19. Env/dev split and trust

Semantics:

- `jet env [name]` opens a shell and never runs project functions.
- `jet dev` realizes `env(base + env.dev)`, waits for services, then runs
  `fn dev()` or fallback `fn run()`.
- Trust prompt gates env entry when hooks/services/sources/secrets are present.
- Grant is keyed by env-definition hash; `--trust` is one-shot; `jet config
  trust add/list/remove` manages patterns.

Touch points:

- Split env shell from dev execution.
- Add trust store: `~/.jet/trust` patterns plus per-repo env hash grants.
- Wire engine dispatch for `config trust`.

Diagnostics:

- `E1231` `dev-no-entry`.
- `E1232` `env-untrusted` for non-interactive / JSON path.

Tests:

- env entry runs no project function;
- dev runs `fn dev()` after service readiness;
- first entry requires trust;
- `--trust` bypasses;
- pattern trust pre-authorizes.

---

## Phase C — env runtime

### U12. Supervised services

Semantics:

- `services:` in `env.*` is a dev runtime surface.
- jetpack supervises under `.jet/services/<name>/`.
- `jet services up|down|health|logs`.
- `jet dev` health-gates startup.

Touch points:

- Add dev `Service` evaluation distinct from jetos system service capture.
- `DevServicePlan` with readiness contract, ports, init, shutdown, data dir.
- `Services` runtime using `std::process`.
- Built-in service catalog maps known services to provider packages and probes.

Diagnostics:

- `E0984` `service-health-timeout`.
- `E0985` `unknown-service-field`.

Tests:

- field-check service fields;
- fixture daemon up/down;
- health gate;
- logs capture.

### U13. Secrets

Semantics:

- `secret("name")` resolves from encrypted repo file.
- Decrypt at env entry / service start, in memory only.
- Reads require `Secret` effect.
- Build tier denies `Secret` by default.
- Crypto backend is a vetted bridge (`D-JPK-SECRETCRYPTO1=A`), not a compiler
  dependency.

Touch points:

- `Secrets` engine module for set/get/recipients/decrypt.
- Capture `secrets:` refs in env/system plans.
- Add `Secret` effect to sema.
- Bridge template for age-style crypto.
- `jet secrets set|get|recipients`.

Diagnostics:

- `E0986` `secret-missing-entry`.
- `E0987` `secret-read-ungranted`.
- `E0988` `secret-in-build`.

Tests:

- missing entry;
- effect required;
- build denial;
- no plaintext in `.jet/`, lock, hangar, or temp files;
- crypto round-trip through bridge fixture.

### U16. Nix bridge

Semantics:

- `jet env -p nodejs ripgrep`.
- `jet env` detects foreign `flake.nix`/`devenv.nix` only when no `env.*`
  modules exist; `--flake` forces it.
- `jet run nixpkgs@fastfetch`.
- `jet bridge flake` emits generated shim.

Touch points:

- Env command gains `-p`, `--flake`, `--pure`.
- Foreign devshell detection shells out to `nix` as ratified stopgap.
- Generated `flake.nix` shim module.
- Lock narHashes in `.jet/lock`.

Diagnostics:

- `E0995` `bridge-no-nix`.
- `L02xx` `devenv-unmappable-field`.

Tests:

- ad-hoc package shell command plan;
- flake detection ordering;
- missing nix;
- generated shim drift check.

---

## Phase D — jetos realization

Do not start until prerequisites land: M12 layer 3 / pure eval foundations,
Phase A dispatch, canonical role modules, and enough hangar realization.

Milestones:

| MS | Goal | Exit |
|---|---|---|
| OS0 | option registry + merge engine | three modules merge to canonical JSON; scalar conflict and cycle snapshots; shuffle-order deterministic |
| OS1 | import tree + host selection + check/init | example repo with two hosts and five modules gives identical JSON across discovery orders |
| OS2 | build generator + activation | VM switch -> rollback; power-cut sim boots prior generation |
| OS3 | lift | external module lift succeeds; private-option read rejected |
| OS4 | std option tree v0 | real machine boots from `system.laptop` |
| OS-ISO | installable graphical image | `jet image installer` / spelling per CLI emits x86_64 Plasma Calamares ISO; QEMU boots to installer |
| OS-VM | scripted VM harness | build ISO -> boot -> switch -> rollback round-trip |

---

## Proposed: ad-hoc adapters (`D-JPK-ADAPTER1`)

Not ratified. Do not implement until owner decides.

Recommended model:

- Provider fetches bytes.
- Adapter recipe turns bytes into a `Pkg`.
- Adapter lock covers source hash, adapter text hash, helper version, platform,
  declared tool deps, effects, and output hash.
- Autodetect drafts recipes but never executes upstream code.

First implementation after ratification should ship `Recipe.prebuilt` and
`Recipe.copy`, then curated `cargo` / `go` / `node` / `cmake` / `make`, then
expert `Recipe.build(fn(BuildContext))` once build authority is settled.
