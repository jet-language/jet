# Epoch 4 completion truth matrix

Audit date: 2026-07-16. This file freezes the exact 63-card Epoch 4 audit set:
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
| #179 | compatibility-only | `crates/jetpack/src/Toolchain.rs` | #419/#426 landed complete action and platform identity; #433 owns live tier-1 acceptance. |
| #85 | model-only | `tests/build_cache_normalization.rs` | #419 landed complete executable action identity; #395 owns live cache protocol and substitution. |
| #5 | live | `tests/ui/plugin_e1257_version_mismatch/main.jet` | Narrow sandboxed plugin target only. |
| #229 | model-only | `crates/jetpack/src/Trust.rs` | #421/#427/#431/#434 own production authority. |
| #231 | model-only | `crates/jetpack/src/PackageGraph.rs` | #423/#424 own resolver and lock wiring. |
| #232 | model-only | `crates/jetpack/src/SemanticLock.rs` | #424 landed the unified atomic semantic lock; #423 owns live resolver consumption. |
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
| #139 | schema-only | `Source/Publish/Schema.rs` | #393 landed canonical Store v2; #395 owns the live cache protocol over its envelope and store. |
| #187 | live | `tests/decisions.rs` | Language memory model; not Jetpack parity. |
| #188 | live | `tests/syntax_reconciliation.rs` | Companion syntax law only. |
| #214 | live | `crates/jetpack/src/CLI.rs` | Historical package-ref spelling only; D-JPK-REF1 later replaced it with `name@source`. |
| #215 | live | `tests/jetpack_dispatch.rs` | Run visibility only. |
| #330 | compatibility-only | `crates/jetpack/src/Overlay.rs` | #424/#428 own lock invalidation and provider conformance. |
| #418 | live | `tests/jetpack_truth.rs` | Truth stop-line only; downstream cards still own breadth. |
| #479 | live | `crates/jetpack/src/Doctor.rs` | Read-only local health diagnosis only; no repair or broad registry-availability claim. |
| #361 | live | `crates/jetpack/src/Output.rs` | Hybrid CLI output surface (D-FE-CLI1): color/plan symbols, NO_COLOR, -y apply, live-region erase; not package-manager parity. |
| #476 | live | `examples/features/devloop/task_runner.jet` | Jet-owned `#Task` entry dispatch (D-JPK-TASKRUN1) across AOT and interpreter tiers; Jetpack retains bridge coverage, not feature ownership; not scheduling or remote run. |
| #477 | live | `crates/jetpack/src/CLI/tool.rs` | On-demand `jetpack tool` run/install for built-in providers (D-JPK-TOOLRUN1); external-provider realization (E1298) not yet live. |
| #478 | live | `crates/jetpack/src/CLI/run_enter_dev.rs` | Monorepo `--filter`/`-p` package selection (D-JPK-SELECTOR1) for local dev/run; narrow workspace-local selector only. |
| #359 | live | `crates/jetpack/src/Shell.rs` | Hybrid bash/zsh/fish prompt only: live env, path, git, command lifecycle, Ctrl-G, strip, and NO_COLOR. Regression 33e2df42d pins the real PTY git branch; no general shell-runtime claim. |
| #419 | live | `tests/build_graph.rs`, `crates/jetpack/src/Recipe.rs` | One BuildPlan IR plus complete ActionKey: recipe lowering, action kinds, dependency outputs, env allowlist, helper versions, exact source, and FrontEndCompletion gate. Regression 0e3c158db; Store ingest remains #393. |
| #393 | live | `tests/jetpack_hangar_store_v2.rs` | Canonical Store v2 objects, metadata, atomic ingest, GC roots, archive hashing, and corrupt quarantine passed 3 focused tests and independent review at c8ab2a64a; closure-safe GC remains #420. |
| #394 | live | `crates/jetpack/src/NixDrv.rs` | ATerm parsing, derivation path calculus, fail-closed inputs, and the 64-derivation corpus passed 5 focused tests and independent review at 54cbab28d; no broader native-Nix claim. |
| #421 | live | `tests/jetpack_trust_root.rs` | Bootstrap pins, digest threshold, delegation, snapshot, rollback, bad-clock, and signature-stripping cases passed 8 library tests plus the integration test at 75678cab; broader authority remains #434. |
| #424 | live | `tests/jetpack_semantic_lock.rs` | One atomic `.jet/lock` path covers inputs, source maps, catalogs, selective update, overlay invalidation, and merge revalidation; 10 semantic-lock tests passed at 4380df6a8. |
| #426 | live | `tests/jetpack_variants.rs` | Typed PackageVariant selection, SysrootIdentity, action-key identity, lock records, and E1316 passed 7 focused tests and independent review at 5af6aeb5d. |
| #539 | model-only | `docs/spec/syntax-decisions.md` | D-SHAPE5a=A records typed package-role fields using the existing record form; independent review found no parser/runtime claim, and #560 owns executable enforcement. |
| #540 | model-only | `docs/spec/syntax-decisions.md` | D-SHAPE5b=A records the closed Output sum; independent review limits this card to the decision, while #587 owns the live Output model and #560 owns language enforcement. |
| #541 | live | `tests/cli.rs` | The grouped command registry, typed run parameters, metadata, completions, E2101/E2103 failures, and single-help path passed the recorded hostile CLI matrix at b1217649b and follow-up commits; no second CLI schema. |
| #578 | model-only | `docs/spec/syntax-decisions.md` | D-SHAPE-MERGEPROVENANCE1=A makes `.jet/lock` the primary successful merge-history authority; decisions and docs lint passed at 4d0288af2, while #560 owns executable views. |
| #582 | model-only | `docs/spec/syntax-decisions.md` | D-SHAPE-EXPOSE1=A preserves input, output, failure, effects, and function identity across lenses; independent review at 360dfaeca found no syntax/runtime claim, and #560 owns enforcement. |
| #586 | model-only | `docs/spec/syntax-decisions.md` | D-ECO-ENV1=A records one typed Environment output projected into dev, tasks, editors, and CI; independent review at 5ed100e5 leaves implementation to #587 and #560. |
| #605 | model-only | `docs/spec/syntax-decisions.md` | D-ECO-COMPOSE2=A records order-independent typed composition, provenance, and explicit conflict resolution; decisions/docs checks passed, while #560 owns runtime conformance. |
| #608 | model-only | `docs/spec/syntax-decisions.md` | D-ECO-RECEIPT2=A connects exact inputs, planned actions, output digests, activation proof, and parent generation without duplicating lock history; independent review passed at d7e8dee2, and #655 owns the receipt substrate. |
| #611 | model-only | `docs/spec/syntax-decisions.md` | D-ECO-EXTENSION1=A limits extensions to ordinary typed functions returning closed graph values under normal checks and provenance; independent review passed at 0b43a93f, and #560 owns enforcement. |
| #615 | model-only | `docs/spec/syntax-decisions.md` | D-ECO-DECL1=A records ordinary named typed project values without new spelling; decisions 2/2 and grammar 15/15 passed at 5877102e, while #560 owns executable source/tooling. |

## Stop-line consequences

- Cache/index envelopes, trust facts, semantic-lock models, provider metadata,
  and offline fixtures cannot appear in release claims as live protocols.
- Sandbox capability detection cannot appear as confinement. Until #398 lands,
  every build is reported as fallback/unsandboxed.
- Installed-Nix compatibility cannot appear as native Nix interoperability.
- Active cards #393–#434 are completion owners. Legacy cards stay historical
  evidence; closing a successor requires its own hostile/live acceptance.
