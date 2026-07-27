---
title: Jet build-system compliance reconciliation
---
# Jet build-system compliance reconciliation — 2026-07-22

Scope: #672, ratified build laws in docs/spec/syntax-decisions.md and docs/plans/epoch-5/metaprogramming.md sections 12 and 15. Baseline is clean origin/master 57401f2d in /tmp/jet-card672-root. Audit changed no compiler files.

## Public execution seam

Real Jet path: crates/jet-comptime/src/Comptime/Build/runtime_bridge.rs begin_program_build/eval_program_build_method/finish_program_build -> crates/jet-driver/src/Driver/mod.rs compile_bundle_path_build -> execute_build_plan/materialize/reload -> Source/CmdCompile.rs build/graph/query/explain commands. Rust-only BuildContext methods without a runtime_bridge route are substrate, not public shipment.

Targeted results:
- scripts/agent/jet-env cargo test --test build_entry: 28/28 pass.
- scripts/agent/jet-env cargo test --test build_graph: 26/26 pass.
- scripts/agent/jet-env cargo test --test diagnostics_coverage: 11/11 pass.
- scripts/agent/jet-env cargo test --test build_cache_normalization: 10/10 pass.
- scripts/agent/jet-env cargo test --test compiler_api: 7/7 pass.
- scripts/agent/jet-env cargo test --test cli profile_: 4/4 pass.
No full suite ran.

## Decision matrix

| Law | Status | Current trace / reconciliation |
|---|---|---|
| D-BUILDENTRY1 | shipped | runtime_bridge session -> Driver selected-root staging -> CmdCompile; build_entry proves root-only selection, exact source closure, execution, frontend reload, and jet build. Credit retired #95. |
| D-BUILDPOLICY1 | partial -> #690 | Root action/probe gate, declaration, CLI/package/workspace merge and pre-spawn denial ship in Driver/build_entry. Dependency-local denial, named dependency grants, full provenance/audit do not. |
| D-BUILDSCOPE1 | partial -> #690 | Policy files are read for a root source, but pkg.jet/workspace.jet unit-local build entry selection, dependency-ordered member builds, conflicts, and static three-layer audit are absent. |
| D-BUILDGEN1 | partial -> #691 | b.generate materializes under .jet/generated, re-enters frontend, participates in transaction/lock, and locked drift is reachable. Additive shadow rejection, deterministic rounds/cycles, emit-generated, and specified example are absent. |
| D-METADEPTH2 | shipped | Driver creates post-sema ProgramInfo; runtime_bridge b.error requires span/code/what/why/fix; build_entry structured-program tests and build_enforce_reserved_code UI prove root-only read/enforce and E3530. Credit #95/#129 only for actual reflection work. |
| D-BUILDPROFILE1 | shipped | CLI -> Source/main.rs ProfileConfig -> CmdCompile profile/cache settings; filtered CLI profile tests 4/4. |
| D-BUILDNORM1 | shipped | CanonicalAST -> CmdCompile content key; build_cache_normalization 10/10. |
| D-BUILD-DEFAULT1 | existing residual #666 | Current BuildProfile::Default remains one Basic/dev default; ratified command split is exact open criterion 3 on #666. No duplicate. |
| D-BUILDTARGET1 | partial -> #692 | All nine add_* methods and default selection reach runtime_bridge/Driver; richer target deps/toolchain/probes remain Rust-only. Credit #219 substrate. |
| D-BUILDACTION1 | partial -> #692 | Basic declared inputs/outputs/argv/caps execute end to end. Uncached phony, env/allowlist, kinds/helpers/signing/pools remain Rust-only. Credit #220 substrate. |
| D-BUILDTOOLCHAIN1 | partial -> #692 | Public fn build has simplified named target triple and actions consume the handle. SDK/linker/signing identities and complete provenance remain Rust-only. Credit #221 substrate. |
| D-BUILDPROBE1 | partial -> #692 | Public find_program/pkg_config/header probes execute; compile checks and explicit reproducibility/toolchain controls remain Rust-only. Credit #221 substrate. |
| D-BUILDCACHE1 | partial -> #692 | Public basic actions use canonical keys/local CAS/restore/rebuild provenance and build_entry proves a second-run hit. Ratified env/allowlist/helpers/signing/full-toolchain identity inputs remain Rust-only until #692 exposes them. Credit #222 substrate and shipped subset. |
| D-BUILDREMOTE1 | gap -> #693 | Only policy/provenance model facts and a remote-denial test exist; no remote cache transport or remote executor. |
| D-BUILDSCHED1 | partial -> #692 | execute_build_plan batches canonical selected actions and build_graph proves order/cancellation/metrics; Jet cannot declare resource pools. Credit #223 substrate. |
| D-BUILDQUERY1 | shipped | CmdCompile graph/query/explain -> Driver query_build_plan/build_plan_json; build_entry proves static non-execution, overlay parity, JSON, explain/rebuild provenance, and LSP canonical plan. Credit #224. |
| D-BUILDLEGACY1 | gap -> #694 | Typed wrapper constructors/policy tests exist only in ActionSpec/build_graph; no Jet fn-build route or CI-ban vertical. Credit #225 substrate only. |
| D-BUILDPLUGIN1 | gap -> #695 | BuildContext validates in-memory WasmComponentPluginSpec contribution, but no package load/component execution reaches real fn build. Distinct #549/application plugin. Credit #226 substrate only. |
| D-FRONTENDAPI1 | partial -> #696 | Source/Compiler.rs Rust facade is real and compiler_api 7/7 passes. Ratified Jet core.compiler and CLI JSON mirror are absent. Distinct #549/#129. Credit #227 substrate. |
| D-DSLBLOCK1 | gap -> #698 | Syntax.rs owns the fixed SQL/HTML whitelist and has no user registration path, but no first-party block parser/sema/value/test vertical exists. Archived #128 records the law only; #698 owns the narrow shipment gap. |
| D-METAMUTATE1 | shipped negative law | Program/build inspection values are read-only; runtime_bridge only registers canonical graph/generated values; build_entry enforcement proves no mutation path. |

## Diagnostic matrix

| Code | diagnostics.md | Reachable Jet-owned What/Why/Fix | Pinned coverage | Reconciliation |
|---|---|---|---|---|
| E3501 | yes | yes, Driver bad_build_signature | tests/ui/build_entry_bad_sig | shipped |
| E3502 | yes | yes, runtime/plan/materialization failures | tests/ui/build_plan_invalid | shipped |
| E3503 | yes | yes, Driver declaration/effective policy | tests/ui/build_action_ungranted | shipped |
| E3504 | yes | no through current public path; diagnostics_coverage acknowledges Driver pre-validation makes executor MissingGrant unreachable | none | #690 owns dependency/workspace reachability and snapshot |
| E3505 | yes | yes, action/probe/sandbox failure | tests/ui/build_action_failed plus build_entry | shipped |
| E3512 | yes | yes, Lock drift reached by two build_entry tests | no tests/ui snapshot | #697 |
| E3530 | yes | yes, runtime_bridge reserved-code guard | tests/ui/build_enforce_reserved_code | shipped |

diagnostics_coverage 11/11 is green, but its E3504 acknowledged-gap comment is direct non-reachability evidence; green registry coverage does not waive I4 snapshot requirements.

## Residual cards and dedup

New exact residuals:
- #690 scope/dependency authority/E3504.
- #691 generated staging completion/E3510/E3511.
- #692 public typed target/action/toolchain/probe/scheduler controls.
- #693 remote cache/execution.
- #694 public declared legacy wrappers.
- #695 packaged WASM build plugins.
- #696 Jet core.compiler plus CLI JSON.
- #697 E3512 UI snapshot.
- #698 fixed stdlib DSL block vertical.

Retired claims credited, not reopened: #95 and #219-#227. Archived #128 is law history, while live #698 owns its unshipped first-party vertical. #129 owns reflection/derive breadth, not frontend/build gaps. #549 owns compiler-extension WASM plugins, not build plugins. #666 already owns D-BUILD-DEFAULT1 implementation. No broad fn-build reimplementation card was created.


## Independent review corrections

Fresh Sol-high review rejected two initial classifications. D-BUILDCACHE1 is now partial through #692 because full cache identity controls are Rust-only. D-DSLBLOCK1 is now an explicit gap through #698 because archived #128 had no live implementation criteria. No other concrete finding remained.
