# Epoch 4 completion truth matrix

Audit date: 2026-07-11. This file freezes the exact 42-card Epoch 4 audit set:
historical completion claims, reopened #6/#330, and this stop-line card (#418).
A `done` card proves only its narrow row below. It never
proves the broader package-manager capability owned by a named successor.

Classes:

- `live`: the narrow card behavior runs through a real product path.
- `compatibility-only`: behavior runs, but through a legacy provider or
  substrate that a named successor must replace.
- `model-only`: typed data or planning exists without the promised live path.
- `schema-only`: persistent shape exists without its consuming protocol.
- `fixture-only`: fixtures prove deterministic code paths, not live systems.

The `jetpack_truth` test requires one row for every live-Tower E4 `done` card,
rejects unknown classes, and requires non-live rows to name an active successor.

| Card | Class | Evidence | Completion boundary |
|---|---|---|---|
| #185 | live | `Source/main.rs` | Language I2 sweep; not Jetpack parity. |
| #99 | compatibility-only | `crates/jetpack/src/Recipe.rs` | Helpers only; #419 owns one action IR and #398 owns confinement. |
| #90 | model-only | `tests/workspace.rs` | #423 owns live resolution; #424 owns one semantic lock. |
| #3 | schema-only | `crates/jet-pkg-model/src/Envelope.rs` | #395 owns substitution; #421/#434 own trust. |
| #13 | compatibility-only | `Source/Publish/Sign.rs` | Author TOFU exists; #421/#434 own cache/registry trust. |
| #179 | compatibility-only | `crates/jetpack/src/Toolchain.rs` | #419/#426 own complete action and platform identity. |
| #85 | model-only | `tests/build_cache_normalization.rs` | #419 owns complete executable action identity. |
| #5 | live | `tests/ui/plugin_e1257_version_mismatch/main.jet` | Narrow sandboxed plugin target only. |
| #229 | model-only | `crates/jetpack/src/Trust.rs` | #421/#427/#431/#434 own production authority. |
| #231 | model-only | `crates/jetpack/src/PackageGraph.rs` | #423/#424 own resolver and lock wiring. |
| #232 | model-only | `crates/jetpack/src/SemanticLock.rs` | #424 owns unified atomic semantic lock. |
| #233 | compatibility-only | `crates/jetpack/src/MigrationImport.rs` | #428 owns live lossless import/build/run. |
| #234 | model-only | `crates/jetpack/src/Provider.rs` | #423/#428 own executable providers. |
| #242 | model-only | `crates/jetpack/src/Replacement.rs` | #428/#429 own executable conformance/certification. |
| #190 | compatibility-only | `tests/jetpack_studio.rs` | #427/#433 own hostile authority and live acceptance. |
| #191 | live | `tests/jetpack_services.rs` | Narrow project-local service lifecycle only. |
| #192 | live | `tests/secrets.rs` | Narrow local secret lifecycle only. |
| #193 | live | `tests/image.rs` | Native local image layout; no registry/cache claim. |
| #194 | model-only | `crates/jetpack/src/JetOS.rs` | Title already gates push; #322 owns remote deployment. |
| #195 | compatibility-only | `crates/jetpack/src/Bridge.rs` | #394/#396/#397/#399 own native no-installed-Nix path. |
| #196 | live | `crates/jet-foundation/src/Syntax.rs` | Naming decision only. |
| #197 | live | `crates/jetpack/src/Provider.rs` | Dispatch seam only. |
| #198 | live | `tests/env_dev_trust.rs` | Env/dev split and trust gate only. |
| #199 | compatibility-only | `tests/jetpack_engine.rs` | #398/#419/#427 own sandbox, action identity, authority. |
| #200 | compatibility-only | `crates/jet-pkg-model/src/Lock.rs` | #423/#424 own live resolver and universal lock. |
| #201 | model-only | `crates/jetpack/src/Store.rs` | #393/#420 own canonical store and closure-safe GC. |
| #202 | compatibility-only | `tests/jetpack_no_daemon.rs` | #399 owns permanent no-installed-Nix product gate. |
| #203 | schema-only | `crates/jet-pkg-model/src/Envelope.rs` | #395 owns live cache protocol. |
| #204 | model-only | `tests/jetpack_platform.rs` | #398/#426/#433 own real tier-1 execution. |
| #205 | live | `tests/jetpack_discovery.rs` | Local index discovery only; no live registry claim. |
| #206 | live | `tests/jetpack_build_debug.rs` | Local debuggability path only. |
| #207 | model-only | `crates/jetpack/src/RuntimePolicy.rs` | #398 owns enforced OS sandbox. |
| #208 | fixture-only | `tests/jetpack_offline.rs` | #395/#433 own syscall denial and live closure proof. |
| #6 | compatibility-only | `tests/pkg.rs` | #423 owns live registry consumption and delivery. |
| #139 | schema-only | `Source/Publish/Schema.rs` | #393 owns canonical Store v2. |
| #187 | live | `tests/decisions.rs` | Language memory model; not Jetpack parity. |
| #188 | live | `tests/syntax_reconciliation.rs` | Companion syntax law only. |
| #214 | live | `crates/jetpack/src/CLI.rs` | `nixpkgs:` spelling only. |
| #215 | live | `tests/jetpack_dispatch.rs` | Run visibility only. |
| #330 | compatibility-only | `crates/jetpack/src/Overlay.rs` | #424/#428 own lock invalidation and provider conformance. |
| #418 | live | `tests/jetpack_truth.rs` | Truth stop-line only; downstream cards still own breadth. |
| #479 | live | `crates/jetpack/src/Doctor.rs` | Read-only local health diagnosis only; no repair or broad registry-availability claim. |
| #361 | live | `crates/jetpack/src/Output.rs` | Hybrid CLI output surface (D-FE-CLI1): color/plan symbols, NO_COLOR, -y apply, live-region erase; not package-manager parity. |
| #476 | live | `examples/features/jetpack/task_runner.jet` | `#Task` entry dispatch (D-JPK-TASKRUN1) across AOT and interpreter tiers; not scheduling or remote run. |
| #477 | live | `crates/jetpack/src/CLI/tool.rs` | On-demand `jetpack tool` run/install for built-in providers (D-JPK-TOOLRUN1); external-provider realization (E1298) not yet live. |
| #478 | live | `crates/jetpack/src/CLI/run_enter_dev.rs` | Monorepo `--filter`/`-p` package selection (D-JPK-SELECTOR1) for local dev/run; narrow workspace-local selector only. |

## Stop-line consequences

- Cache/index envelopes, trust facts, semantic-lock models, provider metadata,
  and offline fixtures cannot appear in release claims as live protocols.
- Sandbox capability detection cannot appear as confinement. Until #398 lands,
  every build is reported as fallback/unsandboxed.
- Installed-Nix compatibility cannot appear as native Nix interoperability.
- Active cards #393–#434 are completion owners. Legacy cards stay historical
  evidence; closing a successor requires its own hostile/live acceptance.
