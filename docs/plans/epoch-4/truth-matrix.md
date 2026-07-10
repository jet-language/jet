# Epoch 4 completion truth matrix

Audit date: 2026-07-10. This file classifies every Epoch 4 card currently in
Tower's `done` phase plus this stop-line card (#418). A `done` card proves only its narrow row below. It never
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
| #185 | live | compiler soundness tests | Language I2 sweep; not Jetpack parity. |
| #99 | compatibility-only | `Provider.rs`, `Recipe.rs`, build fixtures | Helpers only; #419 owns one action IR and #398 owns confinement. |
| #90 | model-only | workspace parser/model tests | #423 owns live resolution; #424 owns one semantic lock. |
| #3 | schema-only | envelope/index round trips | #395 owns substitution; #421/#434 own trust. |
| #13 | compatibility-only | author-sign/verify tests | Author TOFU exists; #421/#434 own cache/registry trust. |
| #179 | compatibility-only | toolchain lock/realize tests | #419/#426 own complete action and platform identity. |
| #85 | model-only | build-cache normalization tests | #419 owns complete executable action identity. |
| #5 | live | WASM plugin load/call integration | Narrow sandboxed plugin target only. |
| #229 | model-only | trust fact model tests | #421/#427/#431/#434 own production authority. |
| #231 | model-only | strict graph data-model tests | #423/#424 own resolver and lock wiring. |
| #232 | model-only | semantic-lock side-model tests | #424 owns unified atomic semantic lock. |
| #233 | compatibility-only | migration normalization tests | #428 owns live lossless import/build/run. |
| #234 | model-only | provider metadata tests | #423/#428 own executable providers. |
| #242 | model-only | replacement proof records | #428/#429 own executable conformance/certification. |
| #190 | compatibility-only | inline dependency tests | #427/#433 own hostile authority and live acceptance. |
| #191 | live | service process integration tests | Narrow project-local service lifecycle only. |
| #192 | live | encrypted secret-store integration tests | Narrow local secret lifecycle only. |
| #193 | live | OCI layout integration tests | Native local image layout; no registry/cache claim. |
| #194 | model-only | fleet parse/capture tests | Title already gates push; #322 owns remote deployment. |
| #195 | compatibility-only | installed-Nix bridge tests | #394/#396/#397/#399 own native no-installed-Nix path. |
| #196 | live | naming registry tests | Naming decision only. |
| #197 | live | engine dispatch tests | Dispatch seam only. |
| #198 | live | env/dev trust integration tests | Env/dev split and trust gate only. |
| #199 | compatibility-only | adapter realization tests | #398/#419/#427 own sandbox, action identity, authority. |
| #200 | compatibility-only | channel/lock tests | #423/#424 own live resolver and universal lock. |
| #201 | model-only | age-based cleanup tests | #393/#420 own canonical store and closure-safe GC. |
| #202 | compatibility-only | no-Nix graceful-degrade tests | #399 owns permanent no-installed-Nix product gate. |
| #203 | schema-only | cache envelope round trips | #395 owns live cache protocol. |
| #204 | model-only | platform key/CI intent tests | #398/#426/#433 own real tier-1 execution. |
| #205 | live | offline discovery CLI/LSP tests | Local index discovery only; no live registry claim. |
| #206 | live | failed-build log/scratch tests | Local debuggability path only. |
| #207 | model-only | policy/status tests | #398 owns enforced OS sandbox. |
| #208 | fixture-only | offline fixture sweep | #395/#433 own syscall denial and live closure proof. |
| #6 | compatibility-only | publish/index UX tests | #423 owns live registry consumption and delivery. |
| #139 | schema-only | package hash fields | #393 owns canonical Store v2. |
| #187 | live | memory-v5 compiler tests | Language memory model; not Jetpack parity. |
| #188 | live | syntax decision/grammar tests | Companion syntax law only. |
| #214 | live | CLI dispatch tests | `nixpkgs:` spelling only. |
| #215 | live | package-run process tests | Run visibility only. |
| #330 | compatibility-only | overlay draft/explain tests | #424/#428 own lock invalidation and provider conformance. |
| #418 | live | hostile cache/sandbox/root tests and this matrix gate | Truth stop-line only; downstream cards still own breadth. |

## Stop-line consequences

- Cache/index envelopes, trust facts, semantic-lock models, provider metadata,
  and offline fixtures cannot appear in release claims as live protocols.
- Sandbox capability detection cannot appear as confinement. Until #398 lands,
  every build is reported as fallback/unsandboxed.
- Installed-Nix compatibility cannot appear as native Nix interoperability.
- Active cards #393–#434 are completion owners. Legacy cards stay historical
  evidence; closing a successor requires its own hostile/live acceptance.
