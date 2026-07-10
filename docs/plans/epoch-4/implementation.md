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

Goal: keep the ratified reserved filenames: `pkg.jet` for package identity,
`env.jet` for dev-shell roles, and `workspace.jet` for the monorepo index.

Work:

1. Ensure `PAYLOAD_FILE` / equivalent remains `"pkg.jet"`.
2. Ensure role-file probes recognize `env.jet` and `workspace.jet` for their
   reserved jobs without making other role modules filename-bound.
3. Treat `pack.jet`, `payload.jet`, `jet.toml`, and `jetpack.toml` as retired
   names with a teaching error pointing to `pkg.jet`.
4. Keep `workspace.jet` semantics tied to `module workspace { ... }`: the file
   gives the index role; the declaration gives the module body shape.

Diagnostic:

- `E1226` `old-manifest-filename`: "`pack.jet` is not the package manifest name;
  Jet reads `pkg.jet`." Fix: rename to `pkg.jet`.

Acceptance test: a repo with `workspace.jet` containing `module workspace` and
`env.jet` containing `module env.dev` is discovered. Other role modules can be
found by declaration in ordinary `.jet` files. `system.*` examples are frozen
jetos research until Epoch 7.

### A3. Module declaration role form (`D-JPK-MODBODY1=A`)

Goal: canonicalize role modules as declaration names:
`module env.dev { fields }`.

Work:

1. Parser accepts dotted role names in module declarations:
   active `env.<name>`, OCI `image.<name>`, `workspace`, and future `build`.
   `system.<name>`/`fleet.<name>` stay research/capture-only until Epoch 7
   reopens jetos.
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

### A4. Hangar object envelope + lock schema (`D-JPK-CACHE1=A`, `D-JPK-TOOLCHAIN1=A`)

Goal: freeze the cache-substitution envelope into the hangar/lock schema **now**,
before adapters, toolchains, and lockfiles proliferate — retrofitting it later
means migrating every hangar and lock in existence (the reason CACHE1 was
decided early).

Work:

1. Add envelope fields to every realized hangar object and its `.jet/lock`
   record: `output_hash` (content hash of the realized output tree),
   `platform` (target triple key), `signature` slot (empty until the TLS/signing
   card), `provenance` (resolved source ref + build recipe id). Writer/reader in
   `Store.rs` + `WorkspaceLock.rs`; keep the human-readable `<name>-<version>-<fp>`
   id (D-PM1).
2. Add a `[toolchain]` lock record shape (used by `toolchain-as-dependency.md`)
   reusing the same envelope fields — a toolchain is an ordinary hangar object.
3. Make build scratch hangar-scoped and crash-cleaned (D-JPK-GC1); `jet hangar du`
   reports honestly and never counts `/tmp`.

Tests: `hangar_object_carries_envelope`; `lock_roundtrips_envelope`;
`build_scratch_is_hangar_scoped`; the D-JPK-OFFLINE1 golden sweep asserts a
lock-satisfied realize touches no network.

Exit: envelope fields present in schema and round-tripped; no later migration
needed for CACHE1/TOOLCHAIN1/ADAPTER1.

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
- `.Iso` rides the frozen jetos installer research tier; do not implement in
  Epoch 4.
- `--push` is gated on TLS; temporary `skopeo` bridge may exist but is not the
  core image contract.

Touch points:

- Extend `ImagePlan`: `kind`, package `from`, `expose`, `env_vars`, `files`,
  `base`. `system from` for ISO remains frozen jetos research.
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

- Frozen by D-JETOS-FREEZE1. Fleet host maps and ssh rollout wait for Epoch 7
  jetos ballots.
- Existing capture experiments may stay inert; do not add user-facing build or
  deploy behavior in Epoch 4.

Touch points:

- Preserve inert research notes only.

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
- `jet run nixpkgs:fastfetch`.
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

## Frozen Research — jetos realization

**Frozen by D-JETOS-FREEZE1.** Full plan (OS0–OS4, OS-ISO, OS-VM: stages, exit
criteria, tests, dependency edges, ballot rows) lives in
[`jetos-implementation.md`](jetos-implementation.md) as research appendix only.
Do not start until Epoch 7 reopens exact syntax and command ballots.

---

## Phase B+ — realization ecosystem (U20–U29, all ratified 2026-07-02)

These gates were ratified after this file's first draft. They ride Phase B/C;
the constraints (U22/U28/U29) are asserted from Phase A onward. The build
substrate for U20 is specified in `package-build-from-source.md` (card #99),
which lands first in the jetpack lane — this phase consumes its `BuildRecipe`.

### U20. Ad-hoc adapters (`D-JPK-ADAPTER1=A`)

Ratified. Adapters are `Pkg` values, not a provider kind: a recipe over fetched
bytes, inline in `env.*` or named in `pkg.jet`, same IR and lock path as any
package. Safety contract: read-only probe (no upstream script runs); build
network denied except locked `fetch(url, sha256:)`; ambient commands only via
`BuildContext` with effect provenance in `.jet/lock`; outputs under the package
output root; build tools are `Pkg` deps; first build passes the U19 trust gate.

Ship order: `Recipe.prebuilt` / `Recipe.copy` (no `BuildContext`) first, then
curated `cargo` / `go` / `node` / `cmake` / `make`, then expert
`Recipe.build(fn(BuildContext))` once the D-BUILDPOLICY1 build-authority slice
lands (e5 build-as-Jet card). `jet add <ref> --adapt` drafts a recipe from
read-only probes and never executes upstream code.

**Constructor/​recipe spellings are a follow-up ballot — `D-JPK-ADAPTNAME1`
(this card). Use internal `BuildRecipe` types until it ratifies; do not bake a
user-facing spelling into fixtures or golden output before then.**

Touch points: `Jetpack/Recipe.rs` (shared with #99), adapter lock identity
(source hash, recipe text hash, helper version, platform, tool deps, effects,
output hash), `jet add --adapt`.

Diagnostics: `E12xx` `adapter-network-ungranted`, `adapter-output-escape`,
`adapter-probe-only` (reuse #99's `E1236`–`E1238` build-sandbox family).

Tests: probe stays read-only; prebuilt/copy realize; sandbox violations
diagnose; adapter lock offline-reproducible.

### U21. Channel refs (`D-JPK-CHANNEL1=A`)

`#latest` / `#v0.x` / `#main` resolve **only** in `jet update` and first
`jet add`; the lock always records the exact resolved identity. `jet outdated`
is read-only. An unlocked channel ref in CI is an error.

Touch points: channel parse in `RefSpec.rs`; resolve-on-update gate; `jet update`
/ `jet outdated` verbs.

Diagnostic: `E12xx` `unlocked-channel-in-ci`.

Tests: channel resolves only on update; lock stays exact; CI-unlocked errors;
`jet outdated` mutates nothing.

### U22. Hangar disk contract (`D-JPK-GC1=B`, owner-amended 2026-07-03)

Auto-GC ages out unreferenced objects (**30d** default, opportunistic, no
daemon); manual verb is **`jet clean`** (not `jet gc`), which both
garbage-collects and optimizes the hangar (hardlink/dedup, `nix store
optimise` equivalent) in one pass; honest `jet hangar du`.
Lockfile/generation-reachable objects are never collected. Zero-`/tmp`
guarantee is golden-tested. Build scratch is hangar-scoped and crash-cleaned.
(Envelope/reachability land in A4.)

Tests: unreferenced object aged out at 30d; reachable object kept; optimize
dedups identical objects and reports bytes saved; `/tmp` stays empty across a
build+crash; `jet hangar du` matches on-disk bytes.

Cleanup: E2604 fix text in docs/spec/diagnostics.md says `jet gc --force` —
rewrite to the `jet clean` spelling and re-bless its snapshot as part of this
item. Also `jet gc` stub mentions in docs/spec/roadmap.md (E2-M8/M12.2).

### U23. No-Nix machines (`D-JPK-NONIX1=A`)

Everything Nix-free realizes normally. A package that genuinely needs the Nix
bridge fails with one `E12xx` naming it plus both fixes (install Nix, or
`--adapt`). Never holds already-realized packages hostage.

Touch points: provider selection reports Nix-need explicitly; diagnostic path.

Diagnostic: `E12xx` `nix-bridge-required`.

Tests: Nix-free env realizes with `nix` absent; bridge-needing package gives the
two-fix error; realized packages still run.

### U24. Binary cache direction (`D-JPK-CACHE1=A`)

Envelope fields land in A4. This gate's *protocol/push* (output-hash-addressed
HTTP, signed objects, hash-verified on arrival) is a later card behind the TLS
gate — implement only the schema now; leave the signature slot empty.

Tests (schema only): object/lock carry `output_hash`, `platform`, `signature`
slot, `provenance` (covered by A4).

### U25. Platform tiers (`D-JPK-PLATFORM1=A`)

Linux + macOS + Windows all tier-1 native for jetpack core (hangar, core
provider, adapters, services, secrets, trust). Stand up per-platform CI lanes in
Phase A; a platform break is P1. The Nix bridge stays Linux/macOS (U23's
diagnostic covers the gap); jetos stays Linux.

Work: Windows/macOS CI lanes; platform-key in the envelope; path/exec
abstractions audited for all three.

Tests: hangar/core/adapter/services/secrets/trust suites run on each lane.

### U26. Discovery (`D-JPK-DISCOVER1=A`)

`jet search` + `jet info` over a fast **local, offline** index built from the
same metadata the resolver uses; LSP completions/hover for package names and
typed option fields in `env.*` modules. Follows once provider metadata is
indexable.

Touch points: local index builder; `jet search` / `jet info` verbs; LSP
provider hooks.

Tests: offline search hits the index; `jet info` shows resolved identity; LSP
completes package names and env option fields.

### U27. Failed-build debuggability (`D-JPK-BUILDDBG1=A`)

`--shell-on-fail` opens a shell inside the **preserved** scratch at the failing
step (the sole exception to the U22 cleanup rule; still GC-swept later).
`jet explain <ref>` prints the resolution path + locked identity. `jet logs
<pkg>` is persisted per-step with `--json`.

Touch points: build runner keeps scratch on failure + registers it for GC;
per-step log capture; `explain` / `logs` verbs.

Tests: failing build preserves scratch and drops a shell; scratch later GC'd;
`jet explain` prints the path; `jet logs --json` shape.

### U28. No daemon / no root (`D-JPK-NODAEMON1=A`)

Standing constraint CI asserts from Phase A: no resident daemon; no root
(transient `sudo` only for jetos activation). Unprivileged sandboxing with an
honest fallback warning + `sandbox require`; file-lock coordination for
concurrent invocations. A violation requires a new ballot.

Tests: no long-lived process after any verb; concurrent realizes coordinate via
file lock; sandbox-unavailable warns (or `sandbox require` errors).

### U29. Offline guarantee (`D-JPK-OFFLINE1=A`)

Realize-class verbs never touch the network when the lock is satisfied.
`--offline` turns any would-be fetch into a loud error; network-class verbs
refuse under it. Golden test severs the network and sweeps every verb.

Tests: lock-satisfied realize with network severed; `--offline` fetch errors
loudly; `jet update` (network-class) refuses under `--offline`.

---

## Follow-up ballot surface (this card)

- **`D-JPK-ADAPTNAME1`** — exact adapter constructor + recipe spellings
  (`Pkg.adapt` / `Recipe.*` and friends). Open; ballot-ready. Until ratified,
  U20 and #99's build recipe use internal `BuildRecipe` types and no user-facing
  spelling appears in fixtures or goldens.
