# Codex Security deep-scan discovery report

Date: 2026-08-03

Repository revision: `47fa26f4b77fbaca22268f5fd25d534a800b442a`

Scan status: canceled during discovery consolidation.

## Status and limits

The scan produced 1,046 raw candidate reports and 134 deduplicated candidates.

The scan did not run centralized validation, attack-path analysis, severity assignment, or final report generation.

Treat every entry as an unvalidated hypothesis. A card can close an entry only with validation evidence.

The scan covered the full repository. It includes compiler code, Jetpack, Tower, editor extensions, scripts, and agent configuration.

## Campaign index

| Campaign | Candidates | Tower card | Tower milestone |
|---|---:|---:|---|
| [Tower authorization, CSRF, and document containment](#tower-control-plane) | 11 | #1377 | `e12-m01-control-planes` |
| [JIT, WebAssembly, FFI, and ABI memory safety](#memory-abi-safety) | 9 | #1378 | `e12-m03-memory-safe-execution` |
| [Identity, tokens, secrets, and cryptography](#identity-secrets-crypto) | 12 | #1379 | `e12-m02-identity-secrets` |
| [Resource bounds, parser depth, and service availability](#resource-bounds) | 35 | #1380 | `e12-m04-bounded-services` |
| [Network egress, SSRF, HTTP framing, and local disclosure](#network-boundaries) | 6 | #1381 | `e12-m05-network-filesystem-containment` |
| [Filesystem roots, symlinks, temporary files, and path containment](#filesystem-containment) | 22 | #1382 | `e12-m05-network-filesystem-containment` |
| [Devserver, Canvas, and Studio control planes](#devtools-control-plane) | 10 | #1383 | `e12-m01-control-planes` |
| [Command, shell, editor, and generated-code injection](#command-code-injection) | 20 | #1384 | `e12-m06-command-supply-chain` |
| [Package, Git, provider, store, and dependency integrity](#package-supply-chain) | 7 | #1385 | `e12-m06-command-supply-chain` |
| [Trust policy, sandbox claims, concurrency, and remaining integrity gaps](#policy-integrity) | 2 | #1385 (absorbed #1386) | `e12-m06-command-supply-chain` |

Card #1387 is the final security gate. It depends on all ten campaign scopes
(#1377-#1386); #1386's two candidates are absorbed into #1385.

## Required disposition

For each candidate, record one outcome: confirmed, rejected, duplicate, out of scope, or already fixed.

Confirmed candidates need a root-cause fix, hostile regression proof, and an independent security review.

Rejected and out-of-scope candidates need source-backed reasons.

## Current tree follow-up (2026-08-22)

The package-trust key-generation candidates are addressed in the current tree:
Hangar, native cache, and shared-store trust material now use the shared bounded
OS-CSPRNG helper in `crates/jetpack/src/TrustRoot.rs` and fail closed when it is
unavailable. The historical candidate counts and unvalidated status above remain
unchanged; this note records the current source disposition for the package-trust
slice and does not replace independent security validation.

The `process-pipeline-limits-ignored` candidate is addressed in the current
tree. The shared `core.process` Prelude pipeline now enforces each stage's
captured output budget, polls live stage deadlines, terminates the pipeline on
output overflow, timeout, forwarder read failure, or broken pipe, and joins
every forwarder and drain worker before returning. Non-detached Windows stages
reuse the existing Job Object tree boundary. The production-path checks are
`core_process_pipeline_honors_stage_timeout`,
`core_process_live_stream_does_not_block_on_sibling_output`,
`core_process_limits_kill_descendants_and_stop_output_early`, and
`process_pipeline_output_limit_matches_aot_resident_jit_and_interpreter`.
The historical candidate record below remains unchanged; this note records its
current source disposition and does not replace independent security
validation.

The `processspec-output-limit-late` candidate is also addressed in the current
tree. The shared `core.process` Prelude bounds captured and streamed output
before wait or receipt assembly, stops the full child tree on overflow, and
returns a typed `IOError`. The production-path checks are
`core_process_limits_kill_descendants_and_stop_output_early` and
`process_session_resource_limits_match_all_execution_tiers`, plus the pipeline
parity check `process_pipeline_output_limit_matches_aot_resident_jit_and_interpreter`.
The historical candidate record below remains unchanged; this note records its
current source disposition and does not replace independent security
validation.

The owned archive, fenced-expansion, and YAML candidates are also addressed in
the current tree. The archive kernel now preflights bounded JSON name output,
checks ZIP/TAR offsets and wire-size arithmetic, and applies one cumulative
materialization budget. `FencedNames` applies checked count and byte budgets to
ranges, explicit lists, and generated statements. Runtime and comptime YAML
use the same depth, node, byte, line, and alias-clone limits. The hostile test
sources are `tests/archive.rs:281-432`,
`crates/jet-parser/src/FencedNames.rs:1006-1051`,
`tests/corelib_parts/derives.rs:1200-1226` (wired through
`tests/corelib.rs:3-7`), and
`crates/jet-comptime/src/Comptime/EncodingLite.rs:3673-3679`.
The process parity source is `tests/jit_run.rs:775-851,1070-1100`.
This lane ran no build or test commands; the source changes and hostile test
cases are evidence only until the integrated verification pass.

## tower-control-plane

### Tower authorization, CSRF, and document containment

11 candidates. Priority P0. Milestone `e12-m01-control-planes`.

The detailed [Tower source artifact](security-deep-scan-2026-08-03-full-tower-control-plane.md)
is authoritative for #1377 source traces and blocker evidence. This table is
the candidate inventory. The similarly named `security-deep-scan-2026-08-03-full.md`
belongs to #1378 and is not a Tower source pointer.

Host does not authenticate a request: the no-token server trusts only a loopback socket whose Host is literal loopback and whose X-Forwarded-For is empty or literal loopback; when a token is configured, remote access is allowed only after the configured token matches. Targeted Tower proof passed with `node --test plugins/tower/test/server.test.mjs`, `node --test plugins/tower/test/wave.test.mjs`, `node --test plugins/tower/test/docs.test.mjs`, `node --test plugins/tower/test/store.test.mjs`, `node --test plugins/tower/test/acceptance-queue.test.mjs`, `node --test plugins/tower/test/repair.test.mjs`, and `node --test plugins/tower/test/security-hardening.test.mjs`.

| Candidate ID | Discovery title | Primary locations | Source reports | Disposition | Current source evidence |
|---|---|---|---:|---|---|
| `tower-default-network-auth-bypass` | Tower's default network authentication bypass exposes read and mutation APIs | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 116 | already-fixed | plugins/tower/app/server.mjs:166-207,254-268,321-323,577-578; token-mode loopback regression plugins/tower/test/server.test.mjs:82-90; no-token hostile-Host regression plugins/tower/test/server.test.mjs:82-90 |
| `tower-docs-symlink-read` | Tower document reads follow symlinks outside the docs root | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 31 | already-fixed | plugins/tower/app/docs.mjs:312-359,555-578,599-605; plugins/tower/app/server.mjs:397-399; hostile regressions plugins/tower/test/docs.test.mjs:99-116 and plugins/tower/test/security-hardening.test.mjs:33-123 |
| `tower-owner-authorization-bypass` | Tower grants owner-acceptance authority without establishing owner identity | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 29 | already-fixed | plugins/tower/app/server.mjs:202-209,321-325,445-530; plugins/tower/app/store.mjs:1778-1782,2006-2011,2061-2143,2169-2172; plugins/tower/test/acceptance-queue.test.mjs:285-409 |
| `tower-loopback-csrf` | Tower loopback mutation APIs lack browser-origin and CSRF controls | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 20 | already-fixed | plugins/tower/app/server.mjs:230-237,321-323,386-410; hostile/headerless regression plugins/tower/test/server.test.mjs:82-129 |
| `tower-owner-payload-forgery` | Tower trusts caller-supplied owner attribution for privileged mutations | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 18 | already-fixed | plugins/tower/app/server.mjs:134,517-550; plugins/tower/app/store.mjs:2096-2103; hostile audit-attribution regression plugins/tower/test/acceptance-queue.test.mjs:244-260; plugins/tower/test/server.test.mjs:127-137,234-248; plugins/tower/test/store.test.mjs:87-95 |
| `tower-docs-symlink-write` | Tower docs API writes through symlinked directories outside the repository | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 17 | already-fixed | plugins/tower/app/docs.mjs:312-359,555-578,599-605; plugins/tower/app/server.mjs:404-419; hostile regressions plugins/tower/test/docs.test.mjs:99-116 and plugins/tower/test/security-hardening.test.mjs:33-195,288-328 |
| `tower-docs-symlink-delete` | Tower docs API deletes or moves files through symlinked directories outside the repository | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 14 | already-fixed | plugins/tower/app/docs.mjs:312-359,555-578,599-605; plugins/tower/app/server.mjs:413-419; hostile regressions plugins/tower/test/docs.test.mjs:99-116 and plugins/tower/test/security-hardening.test.mjs:33-69 |
| `tower-docs-symlink-walk` | Tower docs inventory recursively traverses symlinked directories | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 4 | already-fixed | plugins/tower/app/docs.mjs:547-571,592-618; plugins/tower/test/docs.test.mjs:94-152; plugins/tower/test/security-hardening.test.mjs:33-123 |
| `cd005-tower-token-dns-rebind` | Tower token authentication is bypassed for DNS-rebound loopback requests | plugins/tower/app/server.mjs | 2 | already-fixed | plugins/tower/app/server.mjs:166-207,254-268,321-323,577-578; no-token hostile-Host regressions plugins/tower/test/server.test.mjs:82-90; token-mode local and remote regressions plugins/tower/test/server.test.mjs:82-90 |
| `tower-tracked-state-priority-xss` | Tracked Tower card priority reaches innerHTML without validation or escaping | plugins/tower/app/store.mjs<br>plugins/tower/app/ui/tower.js | 2 | already-fixed | plugins/tower/app/store.mjs:885-890; plugins/tower/app/ui/tower.js:572-578,1033-1038; plugins/tower/test/store.test.mjs:97-102 |
| `tower-ratified-decision-integrity-bypass` | Generic API callers can reopen or delete ratified owner decisions without an owner check | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 1 | already-fixed | plugins/tower/app/server.mjs:129-134,517-552; plugins/tower/app/store.mjs:2096-2103,2107-2112,2169-2172; generic-route hostile regression plugins/tower/test/store.test.mjs:69-85 |

## memory-abi-safety

### JIT, WebAssembly, FFI, and ABI memory safety

9 candidates. Priority P0. Milestone `e12-m03-memory-safe-execution`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `jit-jetarena-vec-layout-casts` | JIT list mutations cast `JetArena` to `Vec` without a guaranteed layout | crates/jet-rt/src/lib.rs<br>crates/jet-jit/src/Collections.rs | 23 |
| `d0017-s1-aot-termios-layout` | AOT terminal control uses a non-portable termios FFI layout | crates/jet-codegen/src/Prelude/Core/RuntimeControl.rs<br>crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs | 4 |
| `d0017-s1-jit-termios-layout` | JIT secret-input path uses a non-portable termios FFI layout | crates/jet-jit/src/IO.rs | 3 |
| `wasm-list-i64-untrusted-ownership` | Generated WebAssembly integer-list ABI reconstructs and frees unchecked host pointers | crates/jet-codegen/src/Codegen/Web.rs | 1 |
| `wasm-list-string-untrusted-ownership` | Generated WebAssembly string-list ABI trusts host pointer ownership and embedded counts | crates/jet-codegen/src/Codegen/Web.rs | 1 |
| `wasm-map-untrusted-ownership` | Generated WebAssembly string-map ABI reconstructs and frees unchecked host pointers | crates/jet-codegen/src/Codegen/Web.rs | 1 |
| `wasm-string-untrusted-ownership` | Generated WebAssembly string ABI reconstructs and frees unchecked host pointers | crates/jet-codegen/src/Codegen/Web.rs | 1 |
| `jit-http-worker-runtime-uaf` | Resident JIT teardown can leave HTTP workers with stale runtime and code pointers | crates/jet-jit/src/Concurrency.rs<br>crates/jet-jit/src/net_http_hosts.rs | 1 |
| `jit-event-four-capture-abi` | Four-capture JIT event callbacks are invoked with the wrong ABI | crates/jet-jit/src/jit/lower_ctx.rs<br>crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs | 1 |

### Current dispositions

The dispositions below are the retained source-backed traces against the
current tree.

| Candidate ID | Disposition | Current-source evidence |
|---|---|---|
| `jit-jetarena-vec-layout-casts` | `already-fixed` | The live list mutation hosts use `JetArena::list_values_mut` at `crates/jet-jit/src/Collections.rs:1694-1703,3613-3664`, which converts carriers explicitly before shared `collection_semantics`; `crates/jet-rt/src/lib.rs:466-480` contains no `JetArena`-to-`Vec` layout cast. Hostile dense-list insert/remove/pop tier-parity proof: `jit_list_mutations_preserve_dense_arena_values` at `tests/jit_run.rs:1953-2014`. |
| `wasm-list-i64-untrusted-ownership` | `already-fixed` | Generated WASM records exact `(kind, ptr, byte_len)` ownership at `crates/jet-codegen/src/Codegen/Web.rs:4407-4453`; fixed-width list argument/free paths compute the byte length and require the registered record before `Box` reconstruction at `crates/jet-codegen/src/Codegen/Web.rs:4522-4534,4546-4553`. Hostile forged-pointer, count, and valid-roundtrip proof: `web_wasm_fixed_int_list_rejects_forged_ownership` at `tests/web_build.rs:4509-4579`. |
| `wasm-list-string-untrusted-ownership` | `already-fixed` | Generated WASM requires an exact registered allocation before parsing the bounded list-string blob at `crates/jet-codegen/src/Codegen/Web.rs:4647-4674` and before freeing it at `4685-4690`. Hostile forged-pointer, length, and valid-roundtrip proof: `web_wasm_list_string_export_hostile_roundtrip` at `tests/web_build.rs:4582-4696`. |
| `wasm-map-untrusted-ownership` | `already-fixed` | Generated WASM requires an exact registered allocation before parsing bounded map bytes at `crates/jet-codegen/src/Codegen/Web.rs:4722-4758` and before freeing it at `4769-4774`. Hostile forged-pointer, length, integer-range, and valid-roundtrip proof: `web_wasm_map_string_int_export_hostile_roundtrip` at `tests/web_build.rs:4699-4941`. |
| `wasm-string-untrusted-ownership` | `already-fixed` | JavaScript creates string tokens at `crates/jet-codegen/src/Prelude/DomRuntime.js:717-735`; generated WASM records exact ownership at `crates/jet-codegen/src/Codegen/Web.rs:4407-4453`, then requires it before string argument reconstruction at `crates/jet-codegen/src/Codegen/Web.rs:4469-4480` and freeing at `4491-4496`. Hostile forged-token, length-mismatch, and valid-roundtrip proof: `web_wasm_string_param_export_hostile_roundtrip` at `tests/web_build.rs:4333-4425`. |
| `d0017-s1-aot-termios-layout` | `already-fixed` | Current `ProcessPty.rs` has no local `Termios` or terminal FFI; PTY setup calls `super::super::jet_term_configure_fd` at `crates/jet-codegen/src/Prelude/CoreLib/ProcessPty.rs:261-262`. The target-specific `Termios`, FFI, and configurator are the single shared implementation at `crates/jet-codegen/src/Prelude/Term.rs:515-616,714-746`, re-exported at `crates/jet-codegen/src/lib.rs:281-282`. Source regression check: `process_pty_reuses_the_canonical_termios_abi` at `tests/os_native.rs:100-120`. |
| `d0017-s1-jit-termios-layout` | `duplicate-of-d0017-s1-aot-termios-layout` | The JIT and interpreter process module reuses the same `ProcessPty.rs` through `crates/jet-codegen/src/lib.rs:283-286` and `crates/jet-jit/src/ambient_interp.rs:839-841`; it has no independent `Termios` layout. Both paths reach the canonical target-specific implementation at `crates/jet-codegen/src/Prelude/Term.rs:515-616,714-746`. Source regression check: `tests/os_native.rs:91-120`. |
| `jit-http-worker-runtime-uaf` | `already-fixed` | HTTP callbacks now use the epoch-validating `Concurrency::try_with_http_jet_runtime_at` at `crates/jet-jit/src/Concurrency.rs:448-469`, `crates/jet-jit/src/net_http_hosts.rs:597-607,623-633,2383-2425,2450-2501`, and `crates/jet-jit/src/Web.rs:122-205`. Shutdown clears handles under the runtime guard at `crates/jet-jit/src/net_http_hosts.rs:58-76`; HTTP/2 owns and drains dispatch joins at `crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs:1903-1930,2550-2554`; resident hot swap clears workers before replacing the module/runtime at `crates/jet-jit/src/jit/resident.rs:632-652`. Source regression checks: `tests/jit_run.rs:283-393`. |
| `jit-event-four-capture-abi` | `already-fixed` | JIT invocation has four native capture slots at `crates/jet-jit/src/Reactive.rs:30-95`, and event/async payload insertion is capped at four at `crates/jet-jit/src/Reactive.rs:360-408,722-780`; lowering rejects capture-plus-payload arity above four at `crates/jet-jit/src/jit/lower_ctx.rs:25006-25043`, while callback signatures include captures and params at `crates/jet-jit/src/jit/functions_compile.rs:455-468` and payload params are added at `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs:3611-3733`. Normal backend fallback is `crates/jet-jit/src/jit/backend.rs:130-135`; hostile AOT/interpreter/default-run parity and JIT preflight proof: `jit_event_four_captures_plus_payload_is_rejected_before_native_call` at `tests/jit_run.rs:199-280`. |

### Card #1387 criterion 2 reconciliation (2026-08-31)

The three remaining historical FAIL entries were in the #1378 memory/ABI
lane. The canonical table above now records `d0017-s1-aot-termios-layout` and
`jit-http-worker-runtime-uaf` as `already-fixed`, and
`d0017-s1-jit-termios-layout` as
`duplicate-of-d0017-s1-aot-termios-layout`; their source citations remain on
the single final-disposition rows.

The `duplicate` disposition is not a live finding. No #1377-#1386 candidate
remains `confirmed`; the other current dispositions and citations are in the
campaign tables below. This is a source reconciliation; no test command was
run in this pass.

Remaining live findings: **0**.

### Current-revision memory/ABI recitation (2026-09-01)

This table supersedes only the source coordinates in the earlier current-dispositions table. It does not change any disposition or historical discovery record.

| Candidate ID | Disposition | Current root, hostile proof, and tiers |
|---|---|---|
| `jit-jetarena-vec-layout-casts` | `already-fixed` | Root: `crates/jet-rt/src/lib.rs:466-480` and `crates/jet-jit/src/Collections.rs:1761-1770,3680-3726`. Hostile proof: `tests/jit_run.rs:1884-1943`. Tiers: release AOT, resident Cranelift JIT, forced interpreter. |
| `wasm-list-i64-untrusted-ownership` | `already-fixed` | Root: `crates/jet-codegen/src/Codegen/Web.rs:4407-4453,4508-4553`. Hostile proof: `tests/web_build.rs:4677-4746`. Tier: generated WebAssembly under the Node/JavaScript host. |
| `wasm-list-string-untrusted-ownership` | `already-fixed` | Root: `crates/jet-codegen/src/Codegen/Web.rs:4407-4453,4622-4691`. Hostile proof: `tests/web_build.rs:4750-4924`. Tier: generated WebAssembly under the Node/JavaScript host. |
| `wasm-map-untrusted-ownership` | `already-fixed` | Root: `crates/jet-codegen/src/Codegen/Web.rs:4407-4453,4694-4776` and `crates/jet-codegen/src/Prelude/DomRuntime.js:817-870`. Hostile proof: `tests/web_build.rs:4928-5169`. Tier: generated WebAssembly under the Node/JavaScript host. |
| `wasm-string-untrusted-ownership` | `already-fixed` | Root: `crates/jet-codegen/src/Codegen/Web.rs:4407-4497` and `crates/jet-codegen/src/Prelude/DomRuntime.js:720-738,884-894`. Hostile proof: `tests/web_build.rs:4501-4592`. Tier: generated WebAssembly under the Node/JavaScript host. |
| `d0017-s1-aot-termios-layout` | `already-fixed` | Shared root: `crates/jet-codegen/src/Prelude/Term.rs:438-728`; AOT caller: `crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:323-343`. Proof: `tests/os_native.rs:281-330,510-705`. Tiers: release AOT, resident Cranelift JIT, forced interpreter. |
| `d0017-s1-jit-termios-layout` | `duplicate-of-d0017-s1-aot-termios-layout` | Adapters: `crates/jet-jit/src/IO.rs:21-23,267-282` and `crates/jet-jit/src/ambient_interp.rs:5314-5321`; shared root: `crates/jet-codegen/src/Prelude/Term.rs:438-728`. Proof: `tests/os_native.rs:690-705`. Tiers: resident Cranelift JIT and forced interpreter. |
| `jit-http-worker-runtime-uaf` | `already-fixed` | Roots: `crates/jet-jit/src/Concurrency.rs:420-522`, `crates/jet-jit/src/net_http_hosts.rs:58-77,615-664,2426-2468,2493-2544`, `crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs:1964-1990,2510-2519,2611-2615`, and `crates/jet-jit/src/jit/resident.rs:525-548,877-899`. Hostile proof: `tests/jit_run.rs:283-323`. Tier: resident Cranelift JIT, including HTTP/1 and HTTP/2 teardown. |
| `jit-event-four-capture-abi` | `already-fixed` | Roots: `crates/jet-jit/src/jit/lower_ctx.rs:25124-25173`, `crates/jet-jit/src/jit/functions_compile.rs:455-468`, `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs:3779-3845`, `crates/jet-jit/src/Reactive.rs:30-95,360-408,752-803`, and `crates/jet-jit/src/jit/backend.rs:104-135`. Hostile proof: `tests/jit_run.rs:199-279`. Tiers: release AOT, forced interpreter, default-run deopt, and direct JIT preflight rejection. |

## identity-secrets-crypto

### Identity, tokens, secrets, and cryptography

12 candidates. Priority P0. Milestone `e12-m02-identity-secrets`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `cd005-auth-predictable-session-id` | Auth runtime issues predictable session identifiers | crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs<br>crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs | 10 |
| `cd005-auth-predictable-magic-token` | Auth runtime issues predictable magic-login tokens | crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs<br>docs/reference/core-library.md | 8 |
| `cd005-auth-oauth-predictable-state` | OAuth state values are predictable and not bound to a browser session | crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs<br>crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs | 6 |
| `cd005-auth-oauth-unverified-subject` | OAuth completion trusts a caller-supplied subject without provider proof | crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs<br>crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs | 6 |
| `notebook-zero-token-fallback` | Notebook bearer-token generation silently falls back to an all-zero token | Source/CmdNotebook.rs | 5 |
| `signing-key-permission-fail-open` | Signing-key generation writes the seed before fail-open permission tightening | Source/Publish/Sign.rs | 4 |
| `auth-session-show-token-leak` | Session.show includes the live bearer session identifier | crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs<br>crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs | 2 |
| `archive-key-predictable-fallback` | Archive signing-key fallback is predictable | crates/jetpack/src/Store/Archive.rs<br>crates/jetpack/src/TrustRoot.rs | 2 |
| `claude-config-credential-exfil` | Tracked Claude permissions combine broad credential reads with unrestricted GitHub API calls | .claude/settings.local.json | 1 |
| `comptime-aes-table-timing-side-channel` | Interpreter AES-256-GCM uses secret-indexed lookup tables and secret-dependent GHASH branches | crates/jet-comptime/src/Comptime/CorePureParity.rs<br>crates/jet-comptime/src/Comptime/CryptoLite/Aes256Gcm.rs | 1 |
| `comptime-argon2id-nonstandard` | Comptime expert.argon2id uses a simplified, likely incompatible KDF | crates/jet-comptime/src/Comptime/CryptoLite/Argon2id.rs<br>crates/jet-comptime/src/Comptime/CorePureParity.rs | 1 |
| `managed-secret-temp-permission-window` | Sensitive managed-file bytes are written before restrictive permissions | crates/jetpack/src/EnvFiles.rs | 1 |

### Current dispositions

These dispositions trace each candidate through the current source. `confirmed` means the reported source-to-sink path remains live. `already-fixed` means the current source removes the reported path. The Claude row is tracked configuration; its regression proof is a config check, not a cargo test.

| Candidate ID | Disposition | Current source evidence |
|---|---|---|
| `cd005-auth-predictable-session-id` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:144-153,168-182,215-236,320-330` — password and magic-link sessions use 32-byte CSPRNG-backed opaque IDs; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:2877-2911`, `crates/jet-jit/src/Crypto.rs:16-36,1063-1141`, and `crates/jet-comptime/src/Comptime/AuthLite.rs:14-18,149-191` route all tiers to the shared Prelude. Hostile proof: `tests/crypto_entropy.rs:727-766` (`auth_tokens_are_opaque_and_fail_closed_without_entropy`). |
| `cd005-auth-predictable-magic-token` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:144-153,264-291,294-330` — magic-link issue uses a 32-byte CSPRNG-backed opaque token and consume checks it before minting a session; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:2886-2899` exposes the same helpers. Hostile proof: `tests/crypto_entropy.rs:727-766` (`auth_tokens_are_opaque_and_fail_closed_without_entropy`). |
| `cd005-auth-oauth-predictable-state` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:333-352` — the incomplete API issues no state and rejects completion until browser binding and provider proof exist; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:2900-2911` routes both calls to that Prelude seam. Hostile proof: `tests/taint.rs:553-578` (`session_show_redacts_bearer_and_oauth_fails_closed`). |
| `cd005-auth-oauth-unverified-subject` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:342-352` — a caller-supplied subject cannot mint a session because OAuth completion fails closed without verified provider proof; `crates/jet-comptime/src/Comptime/AuthLite.rs:14-18,185-191` and `crates/jet-jit/src/Crypto.rs:16-36,1097-1109` include the same Prelude implementation. Hostile proof: `tests/taint.rs:553-578` (`session_show_redacts_bearer_and_oauth_fails_closed`). |
| `notebook-zero-token-fallback` | `already-fixed` | `Source/CmdNotebook.rs:63-75,139-145` — `read_exact` requires entropy and failure exits; no all-zero fallback reaches `serve_loopback`. Interactive loopback startup launches a one-use, 30-second bootstrap URL without the bearer at `157-205,221-233`; `299-311,641-675,724-730` consume the nonce once and keep the bearer in a fragment, while `221-222` omits secrets from the listener notice. Hostile proofs: `tests/notebook.rs:17-41` (`notebook_token_generation_has_no_zero_entropy_fallback`) and `Source/CmdNotebook.rs:760-834` (`listener_notice_withholds_the_bearer_token`, `bootstrap_nonce_is_single_use_and_not_a_bearer`, `minted_bootstrap_nonce_is_fresh_and_hex_encoded`). |
| `signing-key-permission-fail-open` | `already-fixed` | `Source/Publish/Sign.rs:111-117,401-444` — private seeds use `create_new(true).mode(0600)`, write and sync before publication, and permission errors abort; force replacement uses a separate temporary path. Hostile proof: `Source/Publish/Sign.rs:522-545` (`force_keygen_replaces_a_symlink_without_following_it`) checks the hostile target and resulting `0600` mode. |
| `auth-session-show-token-leak` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:14-67,184-190` uses explicit redacted `Debug` implementations and a redacted `session_show` projection. Hostile proof: `tests/taint.rs:553-571` (`session_show_redacts_bearer_and_oauth_fails_closed`). |
| `archive-key-predictable-fallback` | `already-fixed` | `crates/jetpack/src/Store/Archive.rs:1388-1418,1447-1451,1941-1997` uses bounded OS CSPRNG entropy, stages keys at exact `0600`, checks that mode before writing bytes, removes failed partials, and fails closed before directory or entropy work on non-Unix; `crates/jetpack/src/TrustRoot.rs:47-113` has no predictable production fallback. The deterministic `TrustKey::generate` at `crates/jetpack/src/TrustRoot.rs:310-323` is reached only by the explicitly test/drill-only `fixture_threshold_root` helper at `2430-2443`, not by archive signing. Hostile proofs: `crates/jetpack/src/Store/Archive.rs:2150-2195,2197-2246` and `crates/jetpack/src/TrustRoot.rs:2524-2557` (`generated_archive_key_is_private_before_publication`, `generated_archive_key_fails_closed_without_private_permissions`, `archive_signature_covers_unsigned_payload`, `trust_key_debug_redacts_secret_and_receipt_rejects_tag_mutation`). |
| `claude-config-credential-exfil` | `already-fixed` | `.claude/settings.local.json:2-8` — the tracked allow list no longer grants broad `~/.config` reads or unrestricted `gh api`; the negative permission scan returned `CONFIG_CHECK=PASS`. This is tracked configuration, so the regression proof is a repository config check, not a cargo test. |
| `comptime-aes-table-timing-side-channel` | `already-fixed` | `crates/jet-comptime/src/Comptime/CryptoLite/Aes256Gcm.rs:5-40,145-163` — AES S-box and GHASH use fixed-round arithmetic and masks, with no secret-indexed table or branch; `crates/jet-comptime/src/Comptime/CorePureParity.rs:200-216,2165-2226` routes the comptime implementation. Hostile proof: `crates/jet-comptime/tests/crypto_lite.rs:24-40` (`aes256gcm_known_answer_has_no_secret_table_or_ghash_branch`). |
| `comptime-argon2id-nonstandard` | `already-fixed` | `crates/jet-comptime/src/Comptime/CryptoLite/Argon2id.rs:74-97,420-444,464-490` uses standard address blocks, reference areas, final-lane XOR, exact-block BLAKE2b finalization, and standard BLAKE2b/Argon2id KATs; `crates/jet-comptime/src/Comptime/CorePureParity.rs:216,2235-2289` routes the expert call. Hostile proof: `crates/jet-comptime/tests/crypto_lite.rs:42-56` (`argon2id_matches_the_canonical_expert_known_answer`). |
| `managed-secret-temp-permission-window` | `already-fixed` | `crates/jetpack/src/EnvFiles.rs:48-70,133-139,283-297,342-397,640-658` and `crates/jet-env-model/src/ModuleEval/Environment.rs:381-419` set restrictive mode before sensitive writes, clamp explicit sensitive permissions to owner-only bits, repair externally relaxed copy destinations, and explicitly redact sensitive bytes in `Debug` while preserving public bytes. Hostile proofs: `crates/jetpack/src/EnvFiles.rs:954-971,973-999,1001-1015,1017-1045,1047-1076,1078-1118` (`debug_redacts_sensitive_managed_bytes_but_keeps_public_bytes`, `sensitive_temp_is_restricted_before_first_write`, `sensitive_explicit_permissions_are_restricted_in_object_and_copy`, `sensitive_copy_repairs_external_permission_relaxation`, `content_equal_sensitive_objects_have_separate_restricted_identity`). |

### Shared receipt redaction

`Source/ReceiptStore.rs:19,45-64,273-343,809-858` defines the canonical `jet-receipt-v2` codec; its decoder rejects legacy `jet-receipt-v1` bytes before `lookup_context` can replay them. `ReceiptStore::write` receives argv and derives the same secret-name policy used by argv and environment identity, so direct and recorded writes redact before digest and persistence. Hostile proofs at `Source/ReceiptStore.rs:1263-1358` cover direct write redaction, known-current replay redaction, and unknown/rotated/file-secret legacy lookup/list fail-closed behavior.

`crates/jetpack/src/TrustRoot.rs:115-125` owns `constant_time_eq`. Cache receipts and keyring HMACs use it at `crates/jetpack/src/TrustRoot.rs:531-536,1299-1300`; narinfo HMACs use it at `crates/jetpack/src/Store/Nar.rs:840-850`; archive HMACs use it at `crates/jetpack/src/Store/Archive.rs:1510-1515`. Hostile tag mutation tests cover each path.

## resource-bounds

### Resource bounds, parser depth, and service availability

35 candidates. Priority P1. Milestone `e12-m04-bounded-services`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `tar-unbounded-materialization` | TAR APIs materialize every entry without a size limit | corelib/core.archive/pkgs/archive/src/lib.rs<br>crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs | 46 |
| `zip-runtime-unbounded-output` | Runtime `core.archive` ZIP decompression reads expanded output without a bound | corelib/core.archive/pkgs/archive/src/lib.rs<br>crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs | 26 |
| `s1-root-net-http-unbounded` | Compile-time HTTP fetch buffers an unbounded response before digest verification (root) | crates/jet-net/src/lib.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch.rs | 23 |
| `zip-resident-unbounded-output` | Resident ZIP inflater accepts attacker-declared output without the codec limit | crates/jet-comptime/src/Comptime/ArchiveLite.rs<br>crates/jet-comptime/src/Comptime/Methods/core_calls.rs | 10 |
| `s0-devserver-unbounded-header-line` | Devserver accepts unbounded HTTP request and header lines | crates/jet-devserver/src/lib.rs<br>crates/jet-devserver/src/WebHost.rs | 9 |
| `s0-jit-http-simple-unbounded-response` | Simple JIT HTTP client buffers responses without a total cap | crates/jet-jit/src/net_http_hosts.rs<br>crates/jet-pkg-model/src/Prelude/HTTP.rs | 7 |
| `s0-jit-http-request-unbounded-response` | Configurable JIT HTTP request host buffers responses without a cap | crates/jet-jit/src/net_http_hosts.rs | 7 |
| `s2-runtime-json-depth-dos` | Runtime JSON parser has unbounded recursive nesting | crates/jet-codegen/src/Prelude/CoreLib/JetStd/JSONCodec.rs<br>crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingTraits.rs | 6 |
| `gzip-runtime-unbounded-output` | Gzip decompression has no output budget | crates/jet-pkg-model/src/Prelude/Compress.rs<br>crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs | 6 |
| `zstd-runtime-unbounded-output` | Zstandard decompression has no output budget | crates/jet-pkg-model/src/Prelude/Compress.rs<br>crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs | 6 |
| `s2-comptime-json-depth-dos` | Comptime JSON parsing can overflow the compiler stack | crates/jet-comptime/src/Comptime/JSONInterp.rs<br>crates/jet-comptime/src/Comptime/TypedDecode.rs | 5 |
| `runtime-yaml-depth-dos` | Runtime YAML parser has unbounded recursive nesting | crates/jet-codegen/src/Prelude/CoreLib/JetStd/YAML.rs<br>crates/jet-codegen/src/Prelude/CoreLib/Top/DataFmt.rs | 5 |
| `s2-comptime-toml-depth-dos` | Comptime TOML parsing has unbounded recursive nesting | crates/jet-comptime/src/Comptime/EncodingLite.rs<br>crates/jet-comptime/src/Comptime/TypedDecode.rs | 4 |
| `s2-comptime-yaml-depth-dos` | Comptime YAML parsing has unbounded recursive nesting | crates/jet-comptime/src/Comptime/EncodingLite.rs<br>crates/jet-comptime/src/Comptime/TypedDecode.rs | 4 |
| `processspec-output-limit-late` | ProcessSpec output_limit is enforced only after unbounded capture | crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs<br>crates/jet-jit/src/Process.rs | 4 |
| `compiler-extension-json-depth-dos` | Shared package-model JSON parser permits stack-exhausting nesting | crates/jet-foundation/src/JSON.rs<br>crates/jet-pkg-model/src/CompilerExtension.rs | 3 |
| `worktree-tar-entry-count-dos` | Differing worktree TAR parser has unbounded entry count and quadratic accounting | corelib/core.archive/pkgs/archive/src/lib.rs | 3 |
| `s1-root-net-file-unbounded` | Compile-time file fetch can exhaust memory on an endless special file (root) | crates/jet-net/src/lib.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch.rs | 2 |
| `notebook-preauth-slowloris` | Notebook server authenticates only after a blocking read on its sole connection loop | Source/CmdNotebook.rs | 2 |
| `d0017-s1-tar-pax-allocation` | PAX logical size can bypass stored-size allocation limit | crates/jetpack/src/Provider/fetch.rs | 2 |
| `d0017-s3-studio-slowloris` | One incomplete Studio HTTP request blocks the single-threaded service indefinitely | crates/jetpack/src/CLI/studio_server.rs | 2 |
| `nix-json-depth-dos` | Nix evaluator JSON parser has no nesting limit | crates/jet-nix-eval/src/JSON.rs<br>crates/jet-nix-eval/src/Evaluator.rs | 2 |
| `devserver-slowloris-thread-exhaustion` | Devserver thread-per-connection design permits slowloris thread exhaustion | crates/jet-devserver/src/WebHost.rs<br>crates/jet-devserver/src/lib.rs | 2 |
| `embedded-devserver-unbounded-http-resource-use` | Generated embedded devserver permits unbounded request headers and blocking connection threads | crates/jet-codegen/src/Prelude/DevServer.rs | 2 |
| `d0002-s1-http1-slow-body` | HTTP/1 request bodies have only per-read idle timeouts, allowing prolonged worker occupation | crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs | 1 |
| `s2-fenced-range-expansion-dos` | Numbered fence ranges can exhaust compiler memory | crates/jet-parser/src/FencedNames.rs | 1 |
| `runtime-toml-depth-dos` | Runtime TOML parser has unbounded recursive nesting | crates/jet-codegen/src/Prelude/CoreLib/JetStd/TOML.rs<br>crates/jet-codegen/src/Prelude/CoreLib/Top/DataFmt.rs | 1 |
| `build-action-unbounded-output` | Local build actions capture unbounded output without a timeout | crates/jet-comptime/src/Comptime/Build/execution_runtime.rs | 1 |
| `plugin-call-unbounded-resources` | Sandboxed WASM plugin execution lacks time and memory budgets | crates/jet-pkg-model/src/Prelude/Plugin.rs | 1 |
| `process-pipeline-limits-ignored` | JIT process pipelines ignore configured timeout and output limits | crates/jet-jit/src/Process.rs | 1 |
| `envhook-pretrust-symlink-recursion-dos` | Environment fingerprint follows recursive symlinks before trust gating | crates/jetpack/src/EnvHook.rs<br>crates/jetpack/src/CLI/run_enter_dev.rs | 1 |
| `nix-expression-parser-depth-dos` | Foreign-flake parser recurses on nesting before evaluator depth limits apply | crates/jet-nix-eval/src/lib.rs<br>crates/jet-nix-eval/src/Evaluator.rs | 1 |
| `package-treehash-symlink-recursion` | Package hashing follows directory symlinks without cycle or root containment checks | crates/jet-foundation/src/SHA256.rs | 1 |
| `archive-urandom-read-to-eof` | Archive key generation reads /dev/urandom to EOF | crates/jetpack/src/Store/Archive.rs | 1 |
| `module-discovery-symlink-recursion-dos` | Recursive module discovery follows directory symlink cycles without visited-set or depth limit | crates/jet-driver/src/Loader.rs<br>crates/jet-pkg-model/src/Package/Discovery.rs | 1 |

### Current dispositions

These dispositions trace all 35 candidates through the current source. `confirmed` means the reported source-to-sink path remains live. `already-fixed` means the current source removes the reported path. Each row cites current file:line evidence. Source reconciliation (2026-08-30) keeps all 35 rows `already-fixed` against the current source. The similarly named `security-deep-scan-2026-08-03-full.md` is the separate #1378 memory/ABI trace at this revision and contains no resource-bounds entries; the source-backed traces for this lane are recorded here. This lane also tightened the notebook and Studio request readers from idle-only socket timeouts to absolute request deadlines; hostile trickle-byte regression sources are `Source/CmdNotebook.rs:865-889` and `crates/jetpack/src/CLI/studio_server.rs:503-546`. The independent review found and this lane fixed an alternate comptime `core.process.run` capture path: `crates/jet-comptime/src/Comptime/Methods/repl_process.rs:377-555` now drains both pipes under the shared 64 MiB budget, and `:677-700` is its hostile flood proof; the receipt reports the same limit at `crates/jet-comptime/src/Comptime/Methods/core_calls/impure.rs:482-488`. The criterion-2 proof audit below records 35 present proofs, one for every owned candidate, so the implementation and hostile-proof criterion is complete. This lane ran no build or test commands per the card rules; targeted execution remains deferred to the epoch verification pass, and the independent security review remains open.

| Candidate ID | Disposition | File:line evidence |
|---|---|---|
| `zip-resident-unbounded-output` | `already-fixed` | `crates/jet-comptime/src/Comptime/ArchiveLite.rs:272-274` delegates resident ZIP decompression to the canonical archive kernel; `corelib/core.archive/pkgs/archive/src/lib.rs:200-280,431-451,1107-1255` checks ZIP ranges, declared output, decompression output, and cumulative materialization before returning bytes. |
| `d0002-s1-http1-slow-body` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs:2550-2556,2572-2692,2796-2834` carries one fixed body deadline into the shared chunk reader and checks it before every framing, chunk-data, and trailer read with the fixed-length path's timeout error; hostile proof is `tests/http_i9.rs:319-417` (`hostile_chunked_body_deadline_ends_request_on_both_dev_tiers`); gate is `scripts/agent/lane-check.sh` (`CHECK OK`). |
| `zip-runtime-unbounded-output` | `already-fixed` | `corelib/core.archive/pkgs/archive/src/lib.rs:200-280,283-451,453-495,1107-1255` rejects oversized entry counts and declared/actual expanded ZIP output, bounds writer wire sizes, and emits JSON names with checked control escaping and an output budget; hostile source checks are `tests/archive.rs:462-474`. |
| `tar-unbounded-materialization` | `already-fixed` | `corelib/core.archive/pkgs/archive/src/lib.rs:559-599,613-699,701-761,774-997` enforces 4096 entries, 64 MiB per-entry output, 64 MiB aggregate materialization, and checked TAR padding/name/PAX arithmetic for reads and writes; public aggregate-name hostile source is `tests/archive.rs:476-489`. |
| `s0-devserver-unbounded-header-line` | `already-fixed` | `crates/jet-devserver/src/lib.rs:30-34,68-108,150-182` bounds the request and header lines and enforces cumulative header bytes and header count before body allocation. |
| `s0-jit-http-simple-unbounded-response` | `already-fixed` | `crates/jet-jit/src/net_http_hosts.rs:2130-2169` returns a streaming body bridge for simple calls; `crates/jet-pkg-model/src/Prelude/HTTP.rs:70-71,3579-3617,3738-3902,4014-4111,4213-4350,4359-4360,4608-4628,4770-4795` bounds headers, H1/H2 response bytes, and gzip decoded output. Hostile JIT/interpreter proof is `tests/http_i9.rs:268-319`. |
| `s0-jit-http-request-unbounded-response` | `already-fixed` | `crates/jet-jit/src/net_http_hosts.rs:2130-2169,2634-2641` routes configurable requests through the shared response bridge; `crates/jet-pkg-model/src/Prelude/HTTP.rs:70-71,3579-3617,3738-3902,4014-4111,4213-4350,4359-4360,4608-4628,4770-4795` bounds the response body across H1, H2, and gzip decoding. Hostile JIT/interpreter proof is `tests/http_i9.rs:268-319`. |
| `s1-root-net-file-unbounded` | `already-fixed` | `crates/jet-net/src/lib.rs:32,201-212,696-708` routes caller-selected `file://` reads through `read_limited`, which stops at the 64 MiB fetch budget. |
| `s1-root-net-http-unbounded` | `already-fixed` | `crates/jet-net/src/lib.rs:32,201-215,696-708` routes caller-selected HTTP responses through `read_limited`, which stops at the 64 MiB fetch budget. |
| `s2-fenced-range-expansion-dos` | `already-fixed` | `crates/jet-parser/src/FencedNames.rs:12-14,84-109,222-452,468-760` uses checked range arithmetic plus one cumulative count/byte budget for numbered ranges, named ranges, explicit lists, and expanded statements; hostile source checks are `crates/jet-parser/src/FencedNames.rs:1005-1055`. |
| `s2-runtime-json-depth-dos` | `already-fixed` | `crates/jet-foundation/src/EncodingJson.rs:3,61-85,293-348` applies `MAX_JSON_DEPTH` in the shared recursive parser; `crates/jet-codegen/src/Prelude/CoreLib/JetStd/JSONCodec.rs:1-5` uses that parser. |
| `s2-comptime-json-depth-dos` | `already-fixed` | `crates/jet-comptime/src/Comptime/JSONInterp.rs:153-166` delegates both comptime JSON paths to `EncodingJson`; `crates/jet-foundation/src/EncodingJson.rs:293-348` rejects excessive nesting. |
| `s2-comptime-toml-depth-dos` | `already-fixed` | `crates/jet-comptime/src/Comptime/EncodingLite.rs:401,498-508,522-537,563-578,765-780,812-829` checks `MAX_TOML_DEPTH` and passes depth through array and inline-table recursion. |
| `s2-comptime-yaml-depth-dos` | `already-fixed` | `crates/jet-comptime/src/Comptime/EncodingLite.rs:1008-1018` applies the shared depth/node/byte/line budget to block and flow parsing, including cumulative anchor and alias clones; the one-fixture AOT/JIT/interpreter hostile proof is `tests/corelib_parts/derives.rs:1200-1226` via `tests/corelib.rs:3-7`. |
| `runtime-toml-depth-dos` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/JetStd/TOML.rs:10,351-366,388-403,592-605,635-651` checks `MAX_TOML_DEPTH` and carries depth through nested values. |
| `runtime-yaml-depth-dos` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/JetStd/YAML.rs:10-105,109-425,429-730` applies the same depth/node/byte/line budget to block and flow parsing, including cumulative anchor and alias clones; the one-fixture AOT/JIT/interpreter hostile proof is `tests/corelib_parts/derives.rs:1200-1226` via `tests/corelib.rs:3-7`. |
| `gzip-runtime-unbounded-output` | `already-fixed` | `crates/jet-pkg-model/src/Prelude/Compress.rs:16-29` limits gzip reads to 64 MiB plus one sentinel byte and rejects overflow; hostile runtime bridge proof is `tests/archive.rs:245-285`. |
| `zstd-runtime-unbounded-output` | `already-fixed` | `crates/jet-pkg-model/src/Prelude/Compress.rs:38-53` limits zstd reads to 64 MiB plus one sentinel byte and rejects overflow; hostile runtime bridge proof is `tests/archive.rs:230-285`. |
| `processspec-output-limit-late` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs:1009-1279,1549-2058,2241-2424` drains stdout and stderr under one atomic output budget before wait/receipt assembly, kills the full child tree on overflow, joins every reader, and returns `ResourceLimit`; the default capture limit is set in the shared Prelude at `crates/jet-codegen/src/Prelude/CoreLib/Top/ProcessSpec.rs:1-16`, and `crates/jet-jit/src/Process.rs:315-344,367-369` calls it. The alternate comptime ambient adapter at `crates/jet-comptime/src/Comptime/Methods/repl_process.rs:377-555` uses the same 64 MiB contract and kills the pinned process group on overflow; hostile AOT/JIT/interpreter pipeline source is `tests/jit_run.rs:883-905`, and the alternate-path flood proof is `crates/jet-comptime/src/Comptime/Methods/repl_process.rs:677-700`. |
| `build-action-unbounded-output` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/ProcessSandbox.rs:16-18,428-476,478-557` caps captured stdout/stderr and kills timed-out children; `crates/jet-comptime/src/Comptime/Build/execution_runtime.rs:279-340,911-927` applies the 30-second deadline to local BuildActions; regression test is `crates/jet-comptime/src/Comptime/Build/execution_runtime.rs:2865-2922`. |
| `notebook-preauth-slowloris` | `already-fixed` | `Source/CmdNotebook.rs:17-20,189-207,289-298,563-626` caps connections and headers and applies one absolute ten-second deadline across header and body reads before authentication; hostile trickle-byte proof is `Source/CmdNotebook.rs:865-889`. |
| `plugin-call-unbounded-resources` | `already-fixed` | `crates/jet-pkg-model/src/Prelude/Plugin.rs:33-74,95-105` configures fuel, epoch interruption, linear-memory, table, instance, and memory limits; `crates/jet-pkg-model/src/Prelude/Plugin.rs:147-170,306-321` arms the two-second call timer and bounds argument count/wire size. Hostile 1025-argument proof is `tests/authority.rs:130-172`. |
| `d0017-s1-tar-pax-allocation` | `already-fixed` | `crates/jetpack/src/Provider/fetch.rs:356-420` checks raw `stored` payload size before `Vec::with_capacity`; regression-test source `pax_logical_size_cannot_bypass_stored_payload_limit` is at `crates/jetpack/src/Provider/fetch.rs:672-694`. |
| `d0017-s3-studio-slowloris` | `already-fixed` | `crates/jetpack/src/CLI/studio_server.rs:25,111-115,130-133,454-483` configures ten-second socket I/O limits and applies one absolute deadline across the serial request reader; hostile trickle-byte proof is at `crates/jetpack/src/CLI/studio_server.rs:503-546`. |
| `compiler-extension-json-depth-dos` | `already-fixed` | `crates/jet-pkg-model/src/CompilerExtension.rs:484-488` uses `JSON::parse`; `crates/jet-foundation/src/JSON.rs:43-49,80-85,159-199` bounds input and recursive JSON depth. |
| `nix-json-depth-dos` | `already-fixed` | `crates/jet-nix-eval/src/JSON.rs:6,42-53,80-83,115-138` passes depth through JSON recursion and rejects values beyond the evaluator limit. |
| `process-pipeline-limits-ignored` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs:1549-2058` forwards every limited intermediate stdout through a bounded bridge without replacing ordinary direct pipes, cancels on forwarder read/broken-pipe failure before child wait, reuses the Windows Job Object tree boundary, joins every bridge/drain, observes late overflow, and enforces stage deadlines; hostile AOT/JIT/interpreter source is `tests/jit_run.rs:883-905`; `crates/jet-jit/src/Process.rs:325-344` calls that Prelude pipeline. |
| `envhook-pretrust-symlink-recursion-dos` | `already-fixed` | `crates/jetpack/src/EnvHook.rs:279-317,361-382` rejects external links, records cycles with a visited set, and does not recurse through directory symlink entries; `crates/jetpack/src/CLI/run_enter_dev.rs:1755-1761,2019-2051` uses this bounded fingerprint before environment activation. |
| `nix-expression-parser-depth-dos` | `already-fixed` | `crates/jet-nix-eval/src/Evaluator.rs:627-670` tracks parser depth and returns `ResourceLimit` before nested expression parsing exceeds `MAX_EVAL_DEPTH`. |
| `package-treehash-symlink-recursion` | `already-fixed` | `crates/jet-foundation/src/SHA256.rs:420-425` checks the root and every descendant with `symlink_metadata`, rejects symlinks before directory recursion, and enforces tree depth/file-count bounds; hostile source is `crates/jet-foundation/src/SHA256.rs:624-645`. |
| `devserver-slowloris-thread-exhaustion` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:35,1667-1713,1733-1787,1827-1832` caps active connection threads at 64 and applies a ten-second absolute deadline before `Request::read`; `crates/jet-devserver/src/lib.rs:30-34,68-182` bounds request lines, headers, and bodies. Hostile cap/deadline proof is `crates/jet-devserver/src/WebHost.rs:2873-2898`. |
| `archive-urandom-read-to-eof` | `already-fixed` | `crates/jetpack/src/Store/Archive.rs:13,1388-1408,1447-1451` obtains exactly 32 bytes through the bounded `os_random_bytes` helper and fails on entropy errors; exact-size/read contract proof is `crates/jetpack/src/Store/Archive.rs:2176-2184`. |
| `embedded-devserver-unbounded-http-resource-use` | `already-fixed` | `crates/jet-codegen/src/Prelude/DevServer.rs:37-44,1057-1095,1146-1315,1356-1465,2749-2780` caps lines, aggregate headers, active connection threads, socket deadlines, and static/source response bytes before allocation; HTML validates UTF-8 and streams the original bytes around the script under one aggregate budget. Hostile coverage is `tests/web_dev.rs:2767-2838,2841-2869`. |
| `worktree-tar-entry-count-dos` | `already-fixed` | `corelib/core.archive/pkgs/archive/src/lib.rs:613-699,701-761,774-997` is the current canonical TAR parser and rejects more than 4096 entries while bounding materialized output and aggregate bytes with checked padding, header, long-name, and PAX arithmetic. Hostile 4097-entry proof is `tests/archive.rs:491-509`. |
| `module-discovery-symlink-recursion-dos` | `already-fixed` | `crates/jet-pkg-model/src/Package/Discovery.rs:31-38` routes discovery through `AuthorityResolver`; `crates/jet-pkg-model/src/Authority.rs:1065-1078` rejects symlinks before recursive directory descent. Hostile cycle proof is `crates/jet-pkg-model/src/Package/Discovery.rs:204-220`. |

### Criterion 2 hostile regression proof audit (2026-08-30)

The source spans above were checked against the current tree. `present` names a hostile regression that reaches the shared bound; these are proof statuses, not candidate dispositions. All 35 owned candidates have `present` proof. No test command was run under the card's Round 3 rules.

| Candidate ID | Hostile regression proof |
|---|---|
| `zip-resident-unbounded-output` | `present` — `crates/jet-foundation/src/CoreArchive.rs:115-120,131-137` (`hostile_zip_name_copies_share_materialization_budget`, `hostile_zip_name_copies_are_rejected_before_output_allocation`) exercises the shared ZIP materialization budget used by resident and AOT paths. |
| `d0002-s1-http1-slow-body` | `present` — `tests/http_server_lifecycle.rs:3649-3692`, `body_deadline_rejects_a_successful_byte_trickle`, sends one body byte every 10ms and requires the absolute deadline to reject before 200ms. |
| `zip-runtime-unbounded-output` | `present` — `crates/jet-foundation/src/CoreArchive.rs:115-120,131-137` and `tests/archive.rs:462-474` exercise shared ZIP output/materialization rejection. |
| `tar-unbounded-materialization` | `present` — `crates/jet-foundation/src/CoreArchive.rs:87-112` and `tests/archive.rs:476-489` reject retained-name aggregate materialization. |
| `s0-devserver-unbounded-header-line` | `present` — `crates/jet-devserver/src/lib.rs:1210-1222`, `request_lines_and_headers_are_bounded_before_allocation`, rejects oversized request lines and headers. |
| `s0-jit-http-simple-unbounded-response` | `present` — `tests/http_i9.rs:268-319`, `hostile_http_response_lengths_are_rejected_on_both_dev_tiers`, sends a 64 MiB+1 response and rejects it through `http.get` on JIT and interpreter. |
| `s0-jit-http-request-unbounded-response` | `present` — `tests/http_i9.rs:268-319`, `hostile_http_response_lengths_are_rejected_on_both_dev_tiers`, sends a 64 MiB+1 response and rejects it through configurable `request.send` on JIT and interpreter. |
| `s1-root-net-file-unbounded` | `present` — `crates/jet-net/src/lib.rs:1121-1124`, `fetch_reader_rejects_an_endless_response_at_the_boundary`, exercises the shared `read_limited` sink used by file and HTTP fetches. |
| `s1-root-net-http-unbounded` | `present` — `crates/jet-net/src/lib.rs:1121-1124`, `fetch_reader_rejects_an_endless_response_at_the_boundary`, exercises the shared `read_limited` sink used by file and HTTP fetches. |
| `s2-fenced-range-expansion-dos` | `present` — `crates/jet-parser/src/FencedNames.rs:1005-1055` covers numbered ranges, explicit lists, and expanded statement byte budgets. |
| `s2-runtime-json-depth-dos` | `present` — `crates/jet-foundation/src/EncodingJson.rs:19-29`, `hostile_nested_json_is_rejected_before_unbounded_recursion`, exercises the shared runtime JSON parser. |
| `s2-comptime-json-depth-dos` | `present` — `crates/jet-foundation/src/EncodingJson.rs:19-29` exercises the parser delegated to by `crates/jet-comptime/src/Comptime/JSONInterp.rs:153-166`. |
| `s2-comptime-toml-depth-dos` | `present` — `crates/jet-comptime/src/Comptime/EncodingLite.rs:3573-3627` rejects nested TOML values and dotted keys before assembly. |
| `s2-comptime-yaml-depth-dos` | `present` — `crates/jet-comptime/src/Comptime/EncodingLite.rs:3573-3600` plus `tests/corelib_parts/derives.rs:1200-1226` covers depth and alias budgets across execution tiers. |
| `runtime-toml-depth-dos` | `present` — `crates/jet-codegen/src/Prelude/CoreLib/JetStd/TOML.rs:801-824`, `dotted_key_depth_is_bounded_before_assembly`, rejects the depth boundary. |
| `runtime-yaml-depth-dos` | `present` — `tests/corelib_parts/derives.rs:1200-1226`, `yaml_hostile_alias_and_depth_match_all_execution_tiers`, rejects depth and alias expansion across tiers. |
| `gzip-runtime-unbounded-output` | `present` — `tests/archive.rs:245-285`, `runtime_compressors_reject_output_over_the_shared_budget`, feeds a gzip fixture that expands to 64 MiB+1 through the Core runtime bridge. |
| `zstd-runtime-unbounded-output` | `present` — `tests/archive.rs:230-285`, `runtime_compressors_reject_output_over_the_shared_budget`, feeds an RLE zstd fixture that expands to 64 MiB+1 through the Core runtime bridge. |
| `processspec-output-limit-late` | `present` — `tests/jit_run.rs:794-870`, `process_pipeline_output_limit_matches_aot_resident_jit_and_interpreter`, proves bounded ProcessSpec output through AOT, resident JIT, and interpreter; `crates/jet-comptime/src/Comptime/Methods/repl_process.rs:677-700`, `output_limit_kills_a_flooding_process_before_capture_grows_unbounded`, covers the alternate comptime ambient capture path. |
| `build-action-unbounded-output` | `present` — `crates/jet-comptime/src/Comptime/Build/execution_runtime.rs:2865-2922`, `native_sandbox_output_limit_stops_a_flooding_build_action`, floods stdout and requires bounded failure. |
| `notebook-preauth-slowloris` | `present` — `Source/CmdNotebook.rs:865-889`, `partial_notebook_request_cannot_extend_the_absolute_deadline`, uses a trickle-byte client before authentication. |
| `plugin-call-unbounded-resources` | `present` — `tests/authority.rs:130-172`, `plugin_call_rejects_an_overlarge_argument_list_before_guest_execution`, submits 1025 text arguments and asserts rejection before the guest call; the same proof checks fuel, epoch, memory, table, and timeout guards in the shared Prelude. |
| `d0017-s1-tar-pax-allocation` | `present` — `crates/jetpack/src/Provider/fetch.rs:672-694`, `pax_logical_size_cannot_bypass_stored_payload_limit`, checks raw allocation before PAX logical size. |
| `d0017-s3-studio-slowloris` | `present` — `crates/jetpack/src/CLI/studio_server.rs:503-546`, `studio_slowloris_connections_have_io_deadlines`, uses a trickle-byte client and absolute deadline. |
| `compiler-extension-json-depth-dos` | `present` — `crates/jet-foundation/src/JSON.rs:508-525`, `protocol_json_accepts_rfc_escapes_unicode_and_bounded_nesting`, rejects protocol JSON beyond the depth limit used by the extension. |
| `nix-json-depth-dos` | `present` — `crates/jet-nix-eval/src/tests.rs:296-310`, `native_evaluator_applies_json_depth_budget_before_recursive_conversion`, rejects nested JSON before conversion. |
| `process-pipeline-limits-ignored` | `present` — `tests/jit_run.rs:794-870` proves limited and broken pipeline stages refuse output overflow across AOT, resident JIT, and interpreter. |
| `envhook-pretrust-symlink-recursion-dos` | `present` — `crates/jetpack/src/EnvHook.rs:757-775`, `malformed_env_fingerprint_does_not_follow_recursive_symlink`, exercises a parent-link cycle. |
| `nix-expression-parser-depth-dos` | `present` — `crates/jet-nix-eval/src/tests.rs:265-278`, `native_evaluator_rejects_deeply_nested_syntax_before_stack_overflow`, rejects nested expressions at the parser boundary. |
| `package-treehash-symlink-recursion` | `present` — `crates/jet-foundation/src/SHA256.rs:624-645`, `tree_hash_rejects_recursive_symlink_nodes`, rejects a self-link before recursion. |
| `devserver-slowloris-thread-exhaustion` | `present` — `crates/jet-devserver/src/WebHost.rs:2873-2898`, `slowloris_request_hits_the_absolute_deadline_without_exhausting_admission`, sends a partial request and proves both the admission cap and absolute timeout. |
| `archive-urandom-read-to-eof` | `present` — `crates/jetpack/src/Store/Archive.rs:2176-2184`, `archive_entropy_is_exactly_key_sized_and_not_eof_driven`, proves a fixed 32-byte result and the `read_exact` source contract. |
| `embedded-devserver-unbounded-http-resource-use` | `present` — `tests/web_dev.rs:2841-2869`, `embedded_devserver_slow_header_does_not_block_other_clients`, holds a partial header open while a normal client succeeds; existing static-output proofs remain at `tests/web_dev.rs:2767-2838`. |
| `worktree-tar-entry-count-dos` | `present` — `tests/archive.rs:491-509`, `archive_public_tar_reader_rejects_an_entry_count_bomb`, feeds 4097 entries and requires both TAR readers to fail closed. |
| `module-discovery-symlink-recursion-dos` | `present` — `crates/jet-pkg-model/src/Package/Discovery.rs:204-220`, `recursive_symlink_is_rejected_before_discovery_descent`, creates a parent-link cycle and requires discovery to stop before descent. |

### Current-revision resource-bounds recitation (2026-09-01)

A fresh-context security review retraced all 35 resource-bounds candidates through
their current shared roots, cleanup paths, deadlines, parser-depth controls, and
applicable-tier hostile regressions. No candidate remains live.

The final package-tree correction defines file, aggregate-byte, file-count,
node-count, and depth bounds at
`crates/jet-foundation/src/SHA256.rs:19-25`. `try_tree_hash` creates and threads
one node budget at `crates/jet-foundation/src/SHA256.rs:722-739`. The traversal
charges every directory entry, including empty and ignored entries, before
metadata inspection or descent at
`crates/jet-foundation/src/SHA256.rs:763-835`.
`crates/jet-driver/src/Loader.rs:2868-2903` contains no duplicate pre-walk. It
delegates locked-source verification directly to the bounded hash.

Current hostile proofs are
`crates/jet-foundation/src/SHA256.rs:1019-1089,1192-1213` and
`tests/pkg.rs:3622-3729`. At this revision, `jet-foundation tree_hash_` passed
4/4 tests. The locked wide-tree and deep-tree fail-before-manifest tests each
passed 1/1. Earlier dated evidence remains historical. This recitation is the
current-revision authority for card #1380.

## network-boundaries

### Network egress, SSRF, HTTP framing, and local disclosure

6 candidates. Priority P1. Milestone `e12-m05-network-filesystem-containment`.

| Candidate ID | Discovery title | Primary locations | Disposition | File:line evidence | Source reports |
|---|---|---|---|---|---:|
| `comptime-fetch-ssrf` | Hash-pinned compile-time fetch permits arbitrary outbound requests before verification | crates/jet-comptime/src/Comptime/Methods/dispatch.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch/eval_method.rs | already-fixed | AST and BuildContext routes pass the source root to the shared evaluator at `crates/jet-comptime/src/Comptime/Methods/dispatch/eval_method.rs:763-771,2198-2205`; fetch is redirect-free and uses the shared public-address policy at `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:776-795` and `crates/jet-net/src/lib.rs:23-30,621-650`. Hostile direct and BuildContext no-connect proofs are `tests/net_tls.rs:40-120` and `tests/build_entry.rs:2543-2700` (`build_context_fetch_rejects_private_networks_before_connecting`, `build_context_fetch_rejects_outside_files_before_reading`, `build_context_fetch_rejects_hardlinks_to_outside_inodes`). | 4 |
| `cd005-comptime-fetch-local-disclosure` | Hermetic compile-time fetch can disclose arbitrary local text files | crates/jet-net/src/lib.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch.rs | already-fixed | `crates/jet-net/src/lib.rs:205-324,326-545` resolves containment, held-root descriptor access, no-follow checks, and hardlink identity before reading; `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:776-790` routes callers through it. Direct and BuildContext hostile outside-file/hardlink witnesses are `tests/net_tls.rs:98-120` and `tests/build_entry.rs:2631-2700` (`build_context_fetch_rejects_outside_files_before_reading`, `build_context_fetch_rejects_hardlinks_to_outside_inodes`). | 4 |
| `provider-registry-private-network-ssrf` | Project provider policy can authorize private-network HTTPS fetches | crates/jetpack/src/Provider/fetch.rs<br>crates/jetpack/src/Provider/script_registry.rs | already-fixed | Provider fetch rechecks every redirect, resolves public addresses, pins curl, and disables redirects/proxies/configuration at `crates/jetpack/src/Provider/fetch.rs:137-217,221-258`; script-registry realization reaches `Authority::to_path` at `crates/jetpack/src/Provider/script_registry.rs:194-228,369-396`. Hostile policy/helper coverage is `crates/jetpack/src/Provider/fetch.rs:714-752`; executable fake-curl redirect and configured-credential source-to-sink witnesses are `crates/jetpack/src/Provider/fetch.rs:843-959` (`authority_to_path_rejects_redirect_before_second_curl_request`, `configured_provider_credentials_fail_before_curl_execution`). | 4 |
| `jit-http-crlf-injection` | JIT generic HTTP request serialization permits CRLF request injection | crates/jet-jit/src/net_http_hosts.rs<br>crates/jet-pkg-model/src/Prelude/HTTP.rs | already-fixed | `crates/jet-pkg-model/src/Prelude/HTTP.rs:1578-1644,3064-3078` rejects control bytes in URL targets, methods, header names, and values and rejects caller framing headers before connect. `crates/jet-jit/src/net_http_hosts.rs:2206-2214` now returns the existing request header error before the native bridge, while resident-JIT send reaches it at `:4977-4984`; hostile exact AOT method/header/framing/url and cross-origin credential proofs are `tests/http_client_law.rs:97-225,2924-2987`, and resident-JIT plus forced-interpreter proofs are `tests/http_i9.rs:288-337`. | 4 |
| `jit-websocket-handshake-crlf-injection` | WebSocket URL permits HTTP handshake CRLF injection | crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs | already-fixed | `crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs:247-291,554-609` rejects control and whitespace bytes before connecting or interpolating URL parts into the handshake; AOT public no-connect proof is `tests/ws_law.rs:681-737` (`public_aot_ws_connect_rejects_url_controls_before_connecting`), and resident-JIT/forced-interpreter public-connect proof is `tests/http_i9.rs:288-337` (`hostile_http_and_websocket_urls_are_rejected_on_both_dev_tiers`). | 4 |
| `git-dependency-transport-ssrf` | Git dependency fetch allows attacker-selected network destinations | Source/Fetch.rs | already-fixed | Validation runs before Git availability or either sink at `Source/Fetch.rs:842-858,2987-3061`; validated transports alone reach `ls-remote` and clone at `:2466-2471,2545-2555`, with HTTP(S) resolve pinning and SSH host pinning at `:2807-2908,2969-2984`. Source-to-sink package-resolution witnesses install a fake Git marker and assert exact E1203 policy rejection with no execution for private, reserved, link-local, outside-local, unscoped-local, and credential-bearing URLs in `tests/pkg.rs:4367-4488` (`git_dep_hostile_transports_reject_before_fake_git_execution`); existing transport-pin and SSH-option witnesses remain at `tests/pkg.rs:3905-4112,4490-4578`. | 4 |

### Current-revision network-boundaries recitation (2026-09-01)

This table supersedes only the source coordinates in the earlier network table.
It does not change the six dispositions or their historical records.

| Candidate ID | Disposition | Current root and hostile proof |
|---|---|---|
| `comptime-fetch-ssrf` | `already-fixed` | Roots: `crates/jet-comptime/src/Comptime/Methods/dispatch/eval_method.rs:763-771,2198-2200`, `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:759-827`, and `crates/jet-net/src/lib.rs:23-30,609-711`. Proof: `tests/net_tls.rs:40-121` and `tests/build_entry.rs:2583-2700`. |
| `cd005-comptime-fetch-local-disclosure` | `already-fixed` | Root: `crates/jet-net/src/lib.rs:201-346,354-607`. Proof: `tests/net_tls.rs:40-121` and `tests/build_entry.rs:2583-2700`. |
| `provider-registry-private-network-ssrf` | `already-fixed` | Roots: `crates/jetpack/src/Provider/fetch.rs:137-258,530-564` and `crates/jetpack/src/Provider/script_registry.rs:194-228,369-396`. Proof: `crates/jetpack/src/Provider/fetch.rs:714-752,843-967`. |
| `jit-http-crlf-injection` | `already-fixed` | Roots: `crates/jet-pkg-model/src/Prelude/HTTP.rs:1628-1693,2440-2486,2495-2525,3164-3212` and `crates/jet-jit/src/net_http_hosts.rs:2246-2274`. Proof: `tests/http_client_law.rs:98-338,3153-3243` and `tests/http_i9.rs:354-539,924-970`. The shared validator rejects caller framing, hop-by-hop, connection-nominated, and proxy-authentication headers before any connection. |
| `jit-websocket-handshake-crlf-injection` | `already-fixed` | Root: `crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs:247-291,554-609`. Proof: `tests/ws_law.rs:681-737` and `tests/http_i9.rs:354-539,924-970`. |
| `git-dependency-transport-ssrf` | `already-fixed` | Root: `Source/Fetch.rs:843-864,2480-2486,2610-2626,2894-2968,3032-3048,3050-3128`. Proof: `tests/pkg.rs:4074-4090,4095-4300,4557-4676,4680-4766`. Git transport validation calls the canonical `jet_net::is_public_ip` policy and contains no duplicate address table. |

## filesystem-containment

### Filesystem roots, symlinks, temporary files, and path containment

22 candidates. Priority P1. Milestone `e12-m05-network-filesystem-containment`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `package-store-install-symlink-escape` | Package-store installation follows dependency symlinks outside the source tree | Source/Fetch.rs<br>Source/Store.rs | 20 |
| `canvas-create-package-symlink-write` | Canvas `create_package` writes outside the project through symlink ancestors | crates/jet-devserver/src/Canvas/schema_api.rs<br>crates/jet-devserver/src/Canvas/project_transactions.rs<br>crates/jet-devserver/src/Canvas/source_model.rs | 15 |
| `dependency-name-path-traversal` | Unvalidated package identifiers escape store, project, and registry roots | crates/jet-pkg-model/src/Package/Blocks.rs<br>crates/jet-pkg-model/src/Package/Convert.rs | 13 |
| `devserver-static-symlink-escape` | Devserver static reads follow symlinks outside the build directory | crates/jet-devserver/src/lib.rs<br>crates/jet-devserver/src/WebHost.rs | 7 |
| `d0002-s2-sparse-copy-symlink` | Sparse remote fetch fallback follows dependency-controlled symlinks while copying | crates/jetpack/src/Provider/remote.rs<br>crates/jetpack/src/Provider/package.rs | 6 |
| `s0-web-test-prefix-traversal` | Web-test file server uses prefix-only path containment | scripts/web-test/serve.mjs | 4 |
| `vendor-symlink-escape` | Vendoring follows dependency symlinks outside the source tree | Source/Publish/Vendor.rs | 4 |
| `cd005-comptime-embed-file-symlink` | Compile-time embed_file follows project symlinks outside the source root | crates/jet-comptime/src/Comptime/Methods/dispatch.rs | 2 |
| `cd005-comptime-embed-bytes-symlink` | Compile-time embed_bytes follows project symlinks outside the source root | crates/jet-comptime/src/Comptime/Methods/dispatch.rs | 2 |
| `cd005-build-embed-symlink` | BuildContext embed follows project symlinks outside the source root | crates/jet-comptime/src/Comptime/Methods/dispatch.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch/eval_method.rs | 2 |
| `git-revision-cache-path-traversal` | Manifest Git revisions escape the cache root and shape recursive deletion | Source/Fetch.rs | 2 |
| `jetpack-dotenv-symlink-read` | Project-relative dotenv validation follows symlinks outside the project | crates/jetpack/src/CLI/trust_env_build.rs<br>crates/jet-env-model/src/ModuleEval/Environment.rs | 2 |
| `jetpack-image-files-read-traversal` | Image files entries can read arbitrary host paths outside the project | crates/jetpack/src/CLI/add_remove_push_image.rs<br>crates/jetpack/src/Image.rs | 2 |
| `jetpack-image-layer-path-traversal` | OCI tar builder emits unvalidated and silently truncated attacker-controlled paths | crates/jetpack/src/Image.rs<br>crates/jet-env-model/src/ModuleEval/System.rs | 2 |
| `canvas-action-temp-symlink-overwrite` | Predictable Canvas check file follows workspace symlinks | crates/jet-devserver/src/Canvas/edit_actions.rs | 2 |
| `jetpack-overlay-patch-path-traversal` | A malicious overlay patch can overwrite files outside the source root | crates/jetpack/src/Overlay.rs | 2 |
| `trust-prefix-sibling-overmatch` | Trust path prefix matching authorizes sibling project names | crates/jetpack/src/Trust.rs | 1 |
| `devserver-build-symlink-overwrite` | Project-controlled build symlink redirects finalized web outputs to host paths | Source/CmdCompile.rs<br>crates/jet-devserver/src/WebHost.rs | 1 |
| `lsp-predictable-log-symlink-write` | LSP panic logging follows a predictable shared temporary symlink | Source/LSP/Server.rs | 1 |
| `canvas-source-symlink-read` | Canvas projected-source scan follows directory symlinks and can disclose an external Jet file | crates/jet-devserver/src/Canvas/project_scan.rs<br>crates/jet-devserver/src/Canvas/schema_api.rs<br>crates/jet-devserver/src/Canvas/source_model.rs | 1 |
| `jetpack-remote-symlink-fingerprint-escape` | Remote package fingerprint traversal follows symlinks outside the checkout | crates/jetpack/src/Provider/remote.rs | 1 |
| `repl-run-temp-symlink-overwrite` | REPL :run writes predictable files in the shared temporary directory | crates/jet-repl/src/lib.rs | 1 |

### Current dispositions

These dispositions are source-backed traces against the current tree. `confirmed` means the reported source-to-sink path remains live. `already-fixed` means the current source removes the reported path. Each row cites current file:line evidence.

The similarly named `security-deep-scan-2026-08-03-full.md` in this checkout is
the memory/ABI report for #1378, not the filesystem-containment source artifact
for #1382. The rows below are the complete current-tree traces for all 22
owned candidates. Focused hostile-path proofs now cover the listed package,
Canvas, devserver, image, comptime, dotenv, Jetpack, REPL, and LSP paths;
integrated milestone verification remains with Main.

| Candidate ID | Disposition | Current source evidence |
|---|---|---|
| `package-store-install-symlink-escape` | already-fixed | `Source/Fetch.rs:737-757,789-815` rejects lexical/canonical transitive escapes and freezes each path source before manifest, dependency, hash, and store reads; `Source/Store.rs:502-551,871-1057,1113-1158,1210-1295` uses descriptor-relative no-follow opens, identity checks, snapshot copies, and safe publication; `Source/Store.rs:1812-1897` proves a held authority retains the verified root across an active ancestor swap; `tests/pkg.rs:2889-2916,2926-2974,3287-3369` covers source/destination symlinks, multiply-linked source entry points, and transitive symlink escapes. |
| `canvas-create-package-symlink-write` | already-fixed | `crates/jet-devserver/src/Canvas/schema_api.rs:157-191` and `project_transactions.rs:378-410,1397-1454` enforce project-root containment; `source_model.rs:34-63,181-235` reads and publishes through no-follow authorities; `tests/canvas.rs:5046-5081` rejects a package-path symlink. |
| `dependency-name-path-traversal` | already-fixed | `crates/jet-pkg-model/src/Package/Blocks.rs:407-474` validates manifest keys as one safe component before resolver use and `Package/Convert.rs:19-28` repeats the conversion boundary; `tests/pkg.rs:1857-1862` rejects traversal names. |
| `devserver-static-symlink-escape` | already-fixed | `crates/jet-devserver/src/lib.rs:238-265,474-735` performs normalized component checks, held-directory no-follow opens, identity checks, and bounded reads; `WebHost.rs:3383-3442` serves only the opened result; hostile symlink, hardlink, relocation, and size tests are `lib.rs:1275-1382`, with public race coverage at `tests/web_dev.rs:2926-3063`. |
| `d0002-s2-sparse-copy-symlink` | already-fixed | `crates/jetpack/src/Provider/remote.rs:160-334` validates the held checkout before fast rename and falls back only to checked copy; `remote.rs:1735-1852` rejects symlink, special, and multiply-linked entries during fingerprint/copy; `remote.rs:719-826,849-868` covers hostile source, destination, and hardlink cases. |
| `s0-web-test-prefix-traversal` | already-fixed | `scripts/web-test/serve.mjs:18-107,122-236,238-275` resolves component-boundary containment, pins the root, rejects symlinked ancestors/final entries, and streams only the held regular file; `tests/web_dev.rs:665-758` covers symlink escape and sibling-prefix traversal. |
| `vendor-symlink-escape` | already-fixed | `Source/Publish/Vendor.rs:20-47,86-153,206-228` opens each dependency source, hashes it with hardlink rejection, stages and publishes only checked copies, and validates safe names; that source walk calls `Store::hash_directory_authority` at `Vendor.rs:46-47`, which reaches `Store.rs:947-1003` and `871-944`; `tests/pkg.rs:2926-2974,6204-6246` covers multiply-linked and symlinked sources plus traversal names. |
| `cd005-comptime-embed-file-symlink` | already-fixed | `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:403-456,574-585` routes `embed_file` through checked no-follow-root reads and records only accepted bytes; `dispatch.rs:1685-1764` rejects final and ancestor symlink escapes without recording outside input. |
| `cd005-comptime-embed-bytes-symlink` | already-fixed | `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:403-456,422-445` shares the checked `embed_file`/`embed_bytes` path; `dispatch.rs:1685-1764` rejects symlink escapes and `dispatch.rs:1768-1803` rejects hardlinked inputs before hashing. |
| `cd005-build-embed-symlink` | already-fixed | `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:514-568` checks `b.embed` paths and uses bounded no-follow reads; `Comptime/Methods/dispatch/eval_method.rs:2198-2204` reaches that evaluator; `dispatch.rs:1685-1764` covers the hostile link. |
| `git-revision-cache-path-traversal` | already-fixed | `Source/Fetch.rs:1450-1478,2451-2469,2516-2674,2771-2787` validates one safe revision component, holds cache parents, and snapshots/ publishes only through descriptor-relative authorities; `tests/pkg.rs:4702-4731` rejects traversal-shaped revisions before cache creation. |
| `jetpack-dotenv-symlink-read` | already-fixed | `crates/jet-env-model/src/ModuleEval/Environment.rs:2438-2501` validates normal relative declarations, reads through a pinned authority, rejects hardlinks, and revalidates identity; `crates/jetpack/src/CLI/trust_env_build.rs:1118-1132` consumes those bytes; `Environment.rs:3687-3809` covers final/ancestor symlink swaps and hardlinks. |
| `jetpack-image-files-read-traversal` | already-fixed | `crates/jetpack/src/CLI/add_remove_push_image.rs:1121-1184` normalizes safe project-relative names and calls the bounded no-follow root reader; `tests/image.rs:929-1085` covers project traversal, final/ancestor symlinks, and hardlinks without reading outside bytes. |
| `jetpack-image-layer-path-traversal` | already-fixed | `crates/jetpack/src/Image.rs:362-376,1362-1400` validates OCI path components, size, duplicate, and file/directory collision constraints before tar emission; `crates/jet-env-model/src/ModuleEval/System.rs:435-464` supplies sorted declarations; `tests/image.rs:929-961` rejects a hostile layer path. |
| `canvas-action-temp-symlink-overwrite` | already-fixed | `crates/jet-devserver/src/Canvas/edit_actions.rs:1260-1326` checks staged source through Driver's in-memory overlay instead of reopening a predictable scratch pathname; `edit_actions.rs:4233-4288` covers final and ancestor swaps at the legacy temp name. |
| `jetpack-overlay-patch-path-traversal` | already-fixed | `crates/jetpack/src/Overlay.rs:403-418,494-590,617-735` rejects unsafe patch components, validates the current target through held parents, and stages/replaces bytes with exclusive no-follow files; `Overlay.rs:1258-1274,1418-1449` covers traversal and final-target symlink replacement. |
| `trust-prefix-sibling-overmatch` | already-fixed | `crates/jetpack/src/Trust.rs:616-676` matches exact subjects or component boundaries for raw and canonical paths; `Trust.rs:1428-1435` proves a sibling name is not authorized by a project prefix. |
| `devserver-build-symlink-overwrite` | already-fixed | `Source/CmdCompile.rs:6479-6499,7625-7631` opens staging/build roots through `WebOutputAuthority` before artifact writes and publication; `crates/jet-devserver/src/WebHost.rs:1083-1160,1300-1623,2038-2170` validates held output members and journals swaps; `WebHost.rs:3818-3926` covers symlinked roots, staging ancestors, destinations, and hardlinks. |
| `lsp-predictable-log-symlink-write` | already-fixed | `Source/LSP/Server.rs:366-584` selects Linux/Android or macOS/iOS descriptor-relative no-follow flags, opens each parent component and the final file with no-follow semantics, validates held metadata, and returns `Unsupported` on Windows or other Unix targets without equivalent support; `Server.rs:4383-4411` deterministically rejects an ancestor swap before open, `4457-4466` proves unsupported platforms fail closed, and `4469-4491` proves the opened handle remains safe after a later path swap. |
| `canvas-source-symlink-read` | already-fixed | `crates/jet-devserver/src/Canvas/source_model.rs:34-63,181-235` opens Canvas sources componentwise without following links; `project_scan.rs:29-83,387-529` and `schema_api.rs:157-191,444-452` keep discovery and reads on checked paths; `tests/canvas.rs:5563-5594` rejects a symlink source alias. |
| `jetpack-remote-symlink-fingerprint-escape` | already-fixed | `crates/jetpack/src/Provider/remote.rs:316-334,1735-1852` fingerprints complete held trees and permits publication only through checked rename/copy paths, rejecting symlink and special entries; `remote.rs:802-825,849-868` covers hostile publication and multiply-linked source files. |
| `repl-run-temp-symlink-overwrite` | already-fixed | Native `:run` no longer creates or reopens a temporary pathname: `crates/jet-repl/src/lib.rs:1290-1313` sends the materialized source to the private `__jet_repl_run_stdin` child over a pipe, and `lib.rs:1362-1366` runs that exact stdin snapshot; `tests/repl.rs:1102-1140` plants a replaced predictable temp path and proves that the authoritative stdin snapshot runs instead. |

## devtools-control-plane

### Devserver, Canvas, and Studio control planes

10 candidates. Priority P0. Milestone `e12-m01-control-planes`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `devserver-cross-origin-mutation` | Loopback dev server accepts cross-origin state-changing requests without origin, host, or capability checks | crates/jet-devserver/src/WebHost.rs<br>crates/jet-devserver/src/Canvas/schema_api.rs | 154 |
| `s1-root-studio-remote-run` | Unauthenticated non-loopback Studio clients can invoke JetOS build, proof, and switch actions (root) | crates/jetpack/src/CLI/studio_server.rs<br>crates/jetpack/src/CLI/studio_transactions.rs | 38 |
| `s2-devserver-source-disclosure` | Loopback devserver exposes project source without Host or session authentication | crates/jet-devserver/src/WebHost.rs<br>crates/jet-devserver/src/Canvas/graph_projection.rs | 23 |
| `s1-root-studio-remote-write` | Unauthenticated non-loopback Studio clients can mint sessions and overwrite config.jet (root) | crates/jetpack/src/CLI/studio_server.rs<br>crates/jetpack/src/CLI/studio_transactions.rs | 19 |
| `s2-devserver-debug-trigger` | Unauthenticated devserver endpoint can start project debug execution | crates/jet-devserver/src/WebHost.rs<br>crates/jet-devserver/src/Canvas/schema_api.rs | 19 |
| `s1-root-studio-remote-read` | Non-loopback jetos Studio exposes system projection and config.jet without authentication (root) | crates/jetpack/src/CLI/studio_server.rs<br>crates/jetpack/src/CLI/bridge_os_studio.rs | 13 |
| `devserver-windows-absolute-static-path` | Windows absolute paths can escape the devserver static build root | crates/jet-devserver/src/lib.rs<br>crates/jet-devserver/src/WebHost.rs | 11 |
| `s1-root-studio-loopback-csrf` | A malicious website can trigger loopback Studio JetOS subprocess actions via CSRF (root) | crates/jetpack/src/CLI/studio_server.rs<br>crates/jetpack/src/CLI/studio_transactions.rs | 5 |
| `canvas-project-revision-not-enforced` | Canvas project transactions parse but do not enforce project_revision | crates/jet-devserver/src/Canvas/schema_api.rs | 2 |
| `embedded-devserver-windows-absolute-static-path` | Generated embedded devserver can read outside build root on Windows absolute paths | crates/jet-codegen/src/Prelude/DevServer.rs | 2 |

### Current dispositions

These dispositions trace all 10 candidates through the current source. Each row cites current file:line evidence and hostile regression sources. This lane ran no build or test commands under the card's explicit rule; execution remains for the integrated verification pass.

| Candidate ID | Disposition | File:line evidence |
|---|---|---|
| `devserver-cross-origin-mutation` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:1449-1566` allowlists Canvas/control paths and requires strict method, framing, Host, Origin/same-origin, and Bearer/query-session checks; `1839-1853` applies that gate before Canvas dispatch, including mutation/debug sinks at `2154-2292`. Hostile gate coverage is `WebHost.rs:3633-3758`. |
| `s1-root-studio-remote-read` | `already-fixed` | `crates/jetpack/src/CLI/bridge_os_studio.rs:124-143` defaults ordinary Studio to loopback while allowing explicit `--serve`; `crates/jetpack/src/CLI/studio_server.rs:135-210,265-340` authenticates before projection or config reads. Hostile read and cross-origin coverage is `tests/jetpack_studio.rs:120-182,248-297`. |
| `s1-root-studio-remote-write` | `already-fixed` | `crates/jetpack/src/CLI/studio_server.rs:153-166` gates the transaction route; `crates/jetpack/src/CLI/studio_transactions.rs:85-170` requires a server-issued transaction session and rejects direct writes, while `323-443` applies the staged write with revision/CAS checks. Hostile coverage is `tests/jetpack_studio.rs:140-148,259-297,380-520`. |
| `s1-root-studio-remote-run` | `already-fixed` | `crates/jetpack/src/CLI/studio_server.rs:153-170` gates the run route; `crates/jetpack/src/CLI/studio_transactions.rs:631-695,1107-1147` executes the requested JetOS action only after that gate. Unauthenticated, cross-origin, and authorized action coverage is `tests/jetpack_studio.rs:154-160,259-291,553-610`. |
| `s1-root-studio-loopback-csrf` | `already-fixed` | `crates/jetpack/src/CLI/bridge_os_studio.rs:124-143` defaults to loopback; `crates/jetpack/src/CLI/studio_server.rs:265-340` requires Host, same-origin/Origin, and a valid capability, with POST checks at `285-295`. Hostile cross-origin action coverage is `tests/jetpack_studio.rs:154-160,248-291`. |
| `s2-devserver-source-disclosure` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:1509-1566,1839-1853` protects Canvas reads and `2130-2152` serves source only behind that gate; application assets use the generated-root-only boundary at `2519-2605`, with no watched-source fallback and held source reads at `2607-2619`. Hostile symlink/source-directory, invalid-UTF8, and response-budget coverage is `WebHost.rs:2885-3100`. |
| `s2-devserver-debug-trigger` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:1509-1566,1839-1853` requires Canvas Host, Origin, and session checks before the debug sink at `2267-2292`; hostile gate coverage is `WebHost.rs:3633-3758`. |
| `devserver-windows-absolute-static-path` | `already-fixed` | `crates/jet-devserver/src/lib.rs:237-325` rejects rooted, drive-prefixed, traversal, ADS, and non-normal paths; `469-476,502-1082` reads through platform held-root no-follow authority. `crates/jet-devserver/src/WebHost.rs:2519-2604` serves only the held result. Hostile absolute-path, relocation, hardlink, and special-file coverage is `lib.rs:1220-1425`. |
| `canvas-project-revision-not-enforced` | `already-fixed` | `crates/jet-devserver/src/Canvas/schema_api.rs:406-427` requires `project_revision` and compares it with the current project revision before dispatch; project queries pass the expected revision at `501-569`. Stale-project and stale-touched-file proof is `tests/canvas.rs:4946-5011`. |
| `embedded-devserver-windows-absolute-static-path` | `already-fixed` | The generated Prelude validates and reads static paths through `crates/jet-codegen/src/Prelude/DevServer.rs:1339-1465,1467-1478,2195-2208,2619-2706`; Unix/Windows held-root authorities are emitted from `591-1040,1482-2230`. Generated-code and Windows/native hostile proof is `tests/web_build.rs:3553-3611` and `tests/web_dev.rs:2403-2409,2620-3155`. |

## command-code-injection

### Command, shell, editor, and generated-code injection

20 candidates. Priority P1. Milestone `e12-m06-command-supply-chain`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `git-ls-remote-option-injection` | Manifest-controlled Git URL can inject `git ls-remote` options | Source/Fetch.rs<br>crates/jet-pkg-model/src/Package/Blocks.rs | 25 |
| `git-clone-option-injection` | Manifest-controlled Git URL can inject `git clone` options | Source/Fetch.rs<br>crates/jet-pkg-model/src/Package/Blocks.rs | 10 |
| `s0-bash-prompt-label-injection` | Project prompt label is embedded unescaped in generated Bash startup code | crates/jetpack/src/Shell.rs<br>crates/jetpack/src/EnvFile.rs | 6 |
| `package-git-fetch-option-injection` | Package Git revision fragments can inject fetch options | crates/jetpack/src/Provider/remote.rs | 6 |
| `s0-zsh-prompt-label-injection` | Project prompt label is embedded unescaped in generated Zsh startup code | crates/jetpack/src/Shell.rs<br>crates/jet-env-model/src/ModuleEval/Eval.rs | 4 |
| `s0-fish-prompt-label-injection` | Project prompt label is embedded unescaped in Fish startup commands | crates/jetpack/src/Shell.rs<br>crates/jetpack/src/EnvFile.rs | 4 |
| `lldb-breakpoint-command-injection` | Generated filenames are interpolated into LLDB command text | Source/CmdCompile.rs<br>crates/jet-debug/src/Inferior.rs | 3 |
| `vscode-workspace-lsp-rce` | VS Code extension auto-executes a workspace-controlled language server binary | editors/vscode/extension.js<br>editors/vscode/package.json | 3 |
| `zed-worktree-lsp-rce` | Zed extension selects a worktree-controlled language server binary | editors/zed/wasm-src/src/lib.rs<br>editors/zed/extension.toml.in<br>editors/zed/extension.wasm | 2 |
| `package-git-checkout-option-injection` | Package Git revision fragments can inject checkout options | crates/jetpack/src/Provider/remote.rs | 1 |
| `web-codegen-template-injection` | Jet string literals are emitted raw into generated JavaScript templates | crates/jet-codegen/src/Codegen/Web.rs | 1 |
| `rustc-build-profile-env-injection` | Project build-profile environment reaches the rustc process | Source/main.rs<br>Source/CmdCompile.rs | 1 |
| `jetpack-self-authorized-build-script` | Project policy can self-authorize unsandboxed dependency build script | crates/jetpack/src/Trust.rs<br>crates/jet-pkg-model/src/Package/mod.rs | 1 |
| `package-git-kind-probe-option-injection` | Jetpack provider-kind probe parses an untrusted revision as a Git option | crates/jetpack/src/Provider/remote.rs<br>crates/jetpack/src/Provider.rs | 1 |
| `jetos-storage-disk-command-injection` | JetOS storage disk value becomes executable second-stage shell syntax | crates/jetpack/src/JetOS/module_storage_workload.rs | 1 |
| `jetos-storage-esp-command-injection` | JetOS ESP size is embedded unescaped in an executed apply script | crates/jetpack/src/JetOS/module_storage_workload.rs | 1 |
| `envhook-profile-var-name-shell-injection` | Unvalidated profile variable names inject commands into auto-activation scripts | crates/jet-env-model/src/ModuleEval/Environment.rs<br>crates/jetpack/src/Trust.rs | 1 |
| `envhook-unset-name-shell-injection` | Unvalidated lifecycle unset names inject shell commands into auto-activation | crates/jet-env-model/src/ModuleEval/Environment.rs<br>crates/jetpack/src/Trust.rs | 1 |
| `claude-hook-relative-path-rce` | Claude hooks prefer cwd-relative scripts, enabling path-hijack command execution | .claude/settings.json | 1 |
| `perl-bind-compile-exec` | Perl binding inspection executes project compile-time blocks with host authority | Source/CmdDevTools.rs<br>crates/jet-pkg-model/src/PerlBind.rs | 1 |

### Current dispositions

These dispositions trace all 20 candidates through the current source. `confirmed` means the reported source-to-sink path remains live. `already-fixed` means the current source removes the reported path. Each row cites current file:line evidence.

| Candidate ID | Disposition | File:line evidence |
|---|---|---|
| `git-ls-remote-option-injection` | `already-fixed` | `crates/jet-pkg-model/src/Package/Blocks.rs:558-594` parses the manifest URL and selector as data; `Source/Fetch.rs:842-846,1449-1450,2458-2471,2987-2993` validates a manifest URL before resolving a selector and places `--` before `ls-remote` operands. Hostile leading-option URL rejection through the real fetch boundary is covered by `tests/pkg.rs:4580-4654` (`git_dep_rejects_option_url_before_ls_remote_execution`). |
| `git-clone-option-injection` | `already-fixed` | `Source/Fetch.rs:842-846,2502-2556,2572-2579,2987-2993` validates a manifest URL before cloning, places `--` before the clone URL, and checks out only the validated revision. Hostile leading-option URL rejection through the real fetch boundary is covered by `tests/pkg.rs:4580-4645,4656-4663` (`git_dep_rejects_option_url_before_clone_execution`). |
| `s0-bash-prompt-label-injection` | `already-fixed` | `crates/jetpack/src/Shell.rs:802-805,997-1003` keeps the label in `JETPACK_PROMPT_LABEL` and renders it through quoted `printf`; `crates/jetpack/src/Shell.rs:1426-1495` proves hostile Bash labels do not execute. |
| `s0-zsh-prompt-label-injection` | `already-fixed` | `crates/jetpack/src/Shell.rs:802-805,1038-1043` keeps the label in the environment and renders it through `print -r --`; `crates/jetpack/src/Shell.rs:1460-1526` proves hostile Zsh labels do not execute. |
| `s0-fish-prompt-label-injection` | `already-fixed` | `crates/jetpack/src/Shell.rs:802-805,1073-1077` keeps the label in the environment and renders it as a quoted `printf` argument; `crates/jetpack/src/Shell.rs:1528-1554` proves hostile Fish labels do not execute. |
| `package-git-fetch-option-injection` | `already-fixed` | `crates/jetpack/src/Provider/remote.rs:160-217,461-489` routes sparse fetch through the shared Git policy and rejects control characters or leading `-` in the revision; hostile parser/allowlist coverage is `crates/jetpack/src/Provider/remote.rs:624-632`. |
| `package-git-checkout-option-injection` | `already-fixed` | `crates/jetpack/src/Provider/remote.rs:491-565` validates the parsed revision and rechecks it immediately before the `git checkout` argument; hostile parser/allowlist coverage is `crates/jetpack/src/Provider/remote.rs:624-632`. |
| `lldb-breakpoint-command-injection` | `already-fixed` | `crates/jet-debug/src/Inferior.rs:523-533,1214-1225` quotes breakpoint paths and rejects controls; `crates/jet-debug/src/Inferior.rs:1871-1876` proves the hostile path case. |
| `web-codegen-template-injection` | `already-fixed` | `crates/jet-codegen/src/Codegen/Web.rs:2945-2965` escapes backslashes, backticks, `${`, controls, and line separators; `crates/jet-codegen/src/Codegen/Web.rs:11190-11229` applies it to literal template parts, with hostile coverage at `12668-12680` and `tests/web_build.rs:1013-1032`. |
| `rustc-build-profile-env-injection` | `already-fixed` | `crates/jet-pkg-model/src/Package/Blocks.rs:904-910` rejects retired profile `env`; `Source/main.rs:430-487` derives only typed profile flags, while `Source/CmdCompile.rs:7504-7514,7604-7618` passes those flags to `rustc` with no profile environment. Hostile profile rejection is `crates/jet-pkg-model/src/Package/Blocks.rs:2469-2477`. |
| `jetpack-self-authorized-build-script` | `already-fixed` | `crates/jetpack/src/CLI/realize.rs:597-619` gates Core Cargo before Store realization; `crates/jetpack/src/Trust.rs:927-960` requires an exact build identity grant or explicit approval; `crates/jetpack/src/Provider.rs:511-518` dispatches to `Provider::approval_facts`, and `crates/jetpack/src/Provider/core.rs:376-443` derives the identity from the resolved upstream, validated source tree, source digest, Cargo recipe, platform, and exec capability. Hostile identity-mismatch proof is `tests/jetpack_trust_root.rs:445-472`. |
| `package-git-kind-probe-option-injection` | `already-fixed` | `crates/jetpack/src/Provider.rs:2189-2243` rejects an unsafe probe revision and routes every probe command through the hardened Git policy; `crates/jetpack/src/Provider/remote.rs:406-489` validates the parsed remote first, with hostile parser/allowlist coverage at `crates/jetpack/src/Provider/remote.rs:624-632`. |
| `jetos-storage-disk-command-injection` | `already-fixed` | `crates/jetpack/src/JetOS/module_storage_workload.rs:48-127` emits the storage plan/apply script and quotes the disk value; `129-142` allowlist storage sizes and filesystem tokens. Hostile disk execution is rejected with no marker by `tests/jetpack_jetos.rs:1429-1440`; generator coverage is `crates/jetpack/src/JetOS/module_storage_workload.rs:150-188`. |
| `jetos-storage-esp-command-injection` | `already-fixed` | `crates/jetpack/src/JetOS/module_storage_workload.rs:61-63,114-119` accepts only allowlisted storage sizes before script interpolation; `129-137,151-157` enforce and test the boundary. |
| `envhook-profile-var-name-shell-injection` | `already-fixed` | `crates/jet-env-model/src/ModuleEval/Environment.rs:1614-1618,2497-2501,2673-2681` validates environment names; `crates/jetpack/src/EnvHook.rs:470-477,579-587` validates again before rendering; hostile variable-name proof is `tests/env_hook.rs:186-222`. |
| `envhook-unset-name-shell-injection` | `already-fixed` | `crates/jet-env-model/src/ModuleEval/Environment.rs:2309-2325,2653-2663` validates lifecycle unset names; `crates/jetpack/src/EnvHook.rs:569-587` enforces the render boundary; hostile unset-name proof is `tests/env_hook.rs:186-222`. |
| `claude-hook-relative-path-rce` | `already-fixed` | `.claude/settings.json:30-34,45` anchors each hook at the absolute `CLAUDE_PROJECT_DIR` and passes the resulting script path as a quoted argument to `bash`/`exec bash`, without treating the path as shell source. The hostile project/cwd witness is `tests/event_hooks.rs:220-291`, which confirms project scripts run and the injected marker stays absent; no `shell:false` claim is made. |
| `vscode-workspace-lsp-rce` | `already-fixed` | `editors/vscode/extension.js:25-59` refuses all server discovery in untrusted workspaces; `98-109` refuses manual terminal launches; `147-179,209-217` create and start the LSP only when a server path exists; `editors/vscode/package.json:21-25` disables activation in untrusted workspaces. The hostile PATH/marker and zero-launch LSP, terminal, and debug witness is `tests/zed_extension_security.rs:68-331`, with debug registration/descriptor records at `165-193` and assertions at `242-284`. |
| `zed-worktree-lsp-rce` | `already-fixed` | `editors/zed/wasm-src/src/lib.rs:12-24` returns only the literal approved `jet self lsp` command; source/manifest checks are `tests/zed_extension_security.rs:39-50`, and the tracked `editors/zed/extension.wasm` is checked for neither `worktree.which` nor `/target/debug/jet` at `tests/zed_extension_security.rs:51-58`. Zed's process capability and Worktree Trust controls are recorded at `editors/zed/extension.toml.in:15-18` and `editors/zed/README.md:62-67`. |
| `perl-bind-compile-exec` | `already-fixed` | `Source/CmdDevTools.rs:4091-4106` reads the script and invokes `PerlBind::bind`; `crates/jet-pkg-model/src/PerlBind.rs:57-68,131-194` parses source without `perl -c`, with hostile `BEGIN` and `use` import coverage at `tests/cli_parts/bindings.rs:1317-1389` (`perl_bind_does_not_execute_compile_time_code`, `perl_bind_does_not_execute_use_imports`). |

## package-supply-chain

### Package, Git, provider, store, and dependency integrity

7 candidates. Priority P1. Milestone `e12-m06-command-supply-chain`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `package-store-incomplete-content-hash` | Package store integrity hashes omit copied non-.jet files | Source/Store.rs<br>crates/jet-foundation/src/SHA256.rs | 10 |
| `jetpack-typed-environment-trust-bypass` | Typed Jetpack environments bypass trust for lifecycle hooks and services when packages and secrets are empty | crates/jetpack/src/Trust.rs<br>crates/jetpack/src/CLI/run_enter_dev.rs | 8 |
| `locked-dependency-integrity-bypass` | Locked builds use mutable dependency sources without content verification | Source/Fetch.rs<br>crates/jet-driver/src/Loader.rs | 1 |
| `git-revision-utf8-slice-panic` | Multibyte Git revisions panic dependency resolution at a byte boundary | crates/jet-pkg-model/src/Package/Blocks.rs<br>Source/Fetch.rs | 1 |
| `buildrecipe-exec-unsandboxed` | BuildRecipe run executes tools with ambient host authority | crates/jetpack/src/Recipe.rs | 1 |
| `buildrecipe-logged-unsandboxed` | Logged BuildRecipe execution bypasses the promised sandbox | crates/jetpack/src/Provider.rs<br>crates/jetpack/src/Recipe.rs | 1 |
| `transitive-path-dependency-escape` | Transitive path dependencies can escape their fetched dependency root | Source/Fetch.rs<br>crates/jet-driver/src/Loader.rs | 1 |

### Current dispositions

These dispositions trace the seven owned candidates against the current source. `confirmed` means the reported path remains live. `already-fixed` means the current source removes the reported path.

| Candidate ID | Disposition | Current source evidence |
|---|---|---|
| `locked-dependency-integrity-bypass` | `already-fixed` | Locked compiler entry points call `crates/jet-driver/src/Loader.rs:2060-2097` before loading; `:2111-2212` compares exact manifest/lock path and Git identities, with the Git branch at `:2151-2187` checking the URL, selector kind/value, and explicit revision as exact canonical lock identities rather than normalized values; `:2440-2455` rejects a dependency name resolving to multiple source identities; `:2222-2344` verifies the selected immutable source tree against its recorded hash, and `:2347-2371` derives the store path only from validated lock identity. The compiler calls this gate at `crates/jet-driver/src/Driver/mod.rs:2760-2762,3482-3484,5438-5440`; E1204 remains the refusal path. Hostile URL, selector-kind/value, and explicit-revision lock mutations are covered by `tests/pkg.rs:8729-8918` (`locked_build_rejects_manifest_lock_git_identity_mismatches`). |
| `package-store-incomplete-content-hash` | `already-fixed` | `Source/Store.rs:62-80,100-139,393-431,492-551` hashes the copied store tree, while `crates/jet-foundation/src/SHA256.rs:336-440` includes every regular non-hidden file, not only `.jet`; `tests/pkg.rs:3041-3070` proves tampering `runtime.data` returns E1204. |
| `git-revision-utf8-slice-panic` | `already-fixed` | `crates/jet-pkg-model/src/Package/Blocks.rs:82-88,558-595` strips only surrounding quotes and parses exactly one Git URL selector (tag, branch, or revision), retaining the resulting identity as exact lock text; `Source/Fetch.rs:2436-2455` builds the cache prefix with `char_indices`, not a byte-invalid slice; `tests/pkg.rs:3799-3835` proves the multibyte revision returns an error without panic. This was a crash/DoS path, not code execution. |
| `jetpack-typed-environment-trust-bypass` | `already-fixed` | `crates/jetpack/src/Trust.rs:849-925,1012-1069` classifies typed facts independently of package/secret presence and requires an external exact grant; `crates/jetpack/src/CLI/run_enter_dev.rs:95-104,201-210,323-332,1718-1727,2293-2303,2381-2392,3393-3425,3679-3688` routes selected and ordinary `jet run`, project jobs, `env`, `env test`, `env sync`, `env export`, and `dev` through that gate; `tests/jetpack_engine.rs:4146-4261` proves a service-only environment gets E1255 on `env`, ordinary `run`, and selected-workspace `run`, without running its command. |
| `buildrecipe-exec-unsandboxed` | `already-fixed` | `crates/jetpack/src/Recipe.rs:971-995,1127-1133,2520-2635` sends ordinary recipe execution through the native child sandbox and maps unavailable enforcement to E1275; `crates/jet-comptime/src/Comptime/Build/execution_runtime.rs:239-357` has no unsandboxed fallback; hostile coverage is `tests/build_sandbox.rs:249-305,852-897`. |
| `buildrecipe-logged-unsandboxed` | `already-fixed` | `crates/jetpack/src/Provider/adapter.rs:57-72` reaches `Recipe::run_logged`; `crates/jetpack/src/Recipe.rs:1039-1110,2542-2635` sends logged execution through the same native sandbox; `tests/build_sandbox.rs:307-363` attacks the logged path and checks host non-write. |
| `transitive-path-dependency-escape` | `already-fixed` | `Source/Fetch.rs:723-750` enforces lexical and canonical containment below the declaring dependency, with the locked compiler boundary repeated at `crates/jet-driver/src/Loader.rs:2261-2305`; `tests/pkg.rs:3228-3311,3445-3469` cover traversal, symlink, and compiler paths. |

## policy-integrity

### Trust policy, sandbox claims, concurrency, and remaining integrity gaps

2 candidates. Priority P1. Milestone `e12-m06-command-supply-chain`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `secrets-key-permission-window` | Age identity is created with ambient permissions before best-effort chmod | crates/jetpack/src/Secrets.rs | 2 |
| `jetpack-project-trust-self-allow` | Untrusted projects can self-authorize the trust gate that precedes environment hooks | crates/jetpack/src/Trust.rs<br>crates/jetpack/src/CLI/run_enter_dev.rs | 1 |

### Current dispositions

These dispositions trace both candidates through the current source. `confirmed` means the reported source-to-sink path remains live.

| Candidate ID | Disposition | Current source evidence |
|---|---|---|
| `secrets-key-permission-window` | `already-fixed` | `crates/jetpack/src/Secrets.rs:321-357` passes mode `0600` to `OpenOptions` before `create_new` writes the temporary identity; `tests/secrets.rs:80-96` and `crates/jetpack/src/Secrets.rs:817-835` assert the restricted mode. |
| `jetpack-project-trust-self-allow` | `already-fixed` | `crates/jetpack/src/Trust.rs:1012-1057` treats project `Allow` as insufficient external approval and continues to the terminal/prompt gate; `crates/jetpack/src/CLI/run_enter_dev.rs:1718-1727` invokes that gate before environment realization; `crates/jetpack/src/Trust.rs:1363-1402` proves both allow and deny are rejected non-interactively. |

## Source artifacts

The Tower campaign has a dedicated [full source artifact](security-deep-scan-2026-08-03-full-tower-control-plane.md)
for #1377. It records source, control, sink, impact, precondition, and validation evidence for all 11 candidates.

The similarly named [full discovery artifact](security-deep-scan-2026-08-03-full.md) belongs to the separate #1378 memory/ABI lane.

That separate full artifact preserves source, control, sink, impact, evidence,
preconditions, uncertainty, CWE data, validation guidance, and source-ledger paths
for #1378.

This summary is the durable candidate inventory and Tower campaign map. The
dedicated Tower artifact above is the detailed authority for #1377; all named
hostile regressions are source evidence only until the integrated verification
pass runs them.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| `TOWER-CONTROL-PLANE` | card | #1377 |
| `MEMORY-ABI-SAFETY` | card | #1378 |
| `IDENTITY-SECRETS-CRYPTO` | card | #1379 |
| `RESOURCE-BOUNDS` | card | #1380 |
| `NETWORK-BOUNDARIES` | card | #1381 |
| `FILESYSTEM-CONTAINMENT` | card | #1382 |
| `DEVTOOLS-CONTROL-PLANE` | card | #1383 |
| `COMMAND-CODE-INJECTION` | card | #1384 |
| `PACKAGE-SUPPLY-CHAIN` | card | #1385 |
| `POLICY-INTEGRITY` | card | #1385 (absorbed #1386) |
| `SECURITY-GATE` | card | #1387 |
<!-- /audit-dispositions -->
