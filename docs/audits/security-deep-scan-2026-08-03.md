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
| [Tower authorization, CSRF, and document containment](#tower-control-plane) | 11 | #1377 | `e12-security-boundaries` |
| [JIT, WebAssembly, FFI, and ABI memory safety](#memory-abi-safety) | 9 | #1378 | `e12-security-runtime` |
| [Identity, tokens, secrets, and cryptography](#identity-secrets-crypto) | 12 | #1379 | `e12-security-data` |
| [Resource bounds, parser depth, and service availability](#resource-bounds) | 35 | #1380 | `e12-security-runtime` |
| [Network egress, SSRF, HTTP framing, and local disclosure](#network-boundaries) | 6 | #1381 | `e12-security-data` |
| [Filesystem roots, symlinks, temporary files, and path containment](#filesystem-containment) | 22 | #1382 | `e12-security-data` |
| [Devserver, Canvas, Studio, and notebook control planes](#devtools-control-plane) | 10 | #1383 | `e12-security-boundaries` |
| [Command, shell, editor, and generated-code injection](#command-code-injection) | 20 | #1384 | `e12-security-runtime` |
| [Package, Git, provider, store, and dependency integrity](#package-supply-chain) | 7 | #1385 | `e12-security-data` |
| [Trust policy, sandbox claims, concurrency, and remaining integrity gaps](#policy-integrity) | 2 | #1386 | `e12-security-validation` |

Card #1387 is the final security gate. It depends on all ten campaign cards.

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
overflow or timeout, and joins every drain worker before returning. The
production-path checks are `core_process_pipeline_honors_stage_timeout`,
`core_process_live_stream_does_not_block_on_sibling_output`, and
`core_process_limits_kill_descendants_and_stop_output_early`. The historical
candidate record below remains unchanged; this note records its current source
disposition and does not replace independent security validation.

The `processspec-output-limit-late` candidate is also addressed in the current
tree. The shared `core.process` Prelude bounds captured and streamed output
before wait or receipt assembly, stops the full child tree on overflow, and
returns a typed `IOError`. The production-path checks are
`core_process_limits_kill_descendants_and_stop_output_early` and
`process_session_resource_limits_match_all_execution_tiers`. The historical
candidate record below remains unchanged; this note records its current source
disposition and does not replace independent security validation.

## tower-control-plane

### Tower authorization, CSRF, and document containment

11 candidates. Priority P0. Milestone `e12-security-boundaries`.

| Candidate ID | Discovery title | Primary locations | Source reports | Disposition | Current source evidence |
|---|---|---|---:|---|---|
| `tower-default-network-auth-bypass` | Tower's default network authentication bypass exposes read and mutation APIs | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 116 | confirmed | plugins/tower/app/server.mjs:205-206,457-487,504-506 |
| `tower-docs-symlink-read` | Tower document reads follow symlinks outside the docs root | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 31 | confirmed | plugins/tower/app/docs.mjs:83-100,209-213; plugins/tower/app/server.mjs:343-350 |
| `tower-owner-authorization-bypass` | Tower grants owner-acceptance authority without establishing owner identity | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 29 | confirmed | plugins/tower/app/server.mjs:176-181,280-284,422-455 |
| `tower-loopback-csrf` | Tower loopback mutation APIs lack browser-origin and CSRF controls | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 20 | confirmed | plugins/tower/app/server.mjs:32-35,161-163,205-206,457-487 |
| `tower-owner-payload-forgery` | Tower trusts caller-supplied owner attribution for privileged mutations | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 18 | confirmed | plugins/tower/app/server.mjs:457-487; plugins/tower/app/store.mjs:1453-1468 |
| `tower-docs-symlink-write` | Tower docs API writes through symlinked directories outside the repository | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 17 | confirmed | plugins/tower/app/docs.mjs:76-100,225-259; plugins/tower/app/server.mjs:351-359 |
| `tower-docs-symlink-delete` | Tower docs API deletes or moves files through symlinked directories outside the repository | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 14 | confirmed | plugins/tower/app/docs.mjs:76-100,262-293; plugins/tower/app/server.mjs:360-366 |
| `tower-docs-symlink-walk` | Tower docs inventory recursively traverses symlinked directories | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 4 | confirmed | plugins/tower/app/docs.mjs:142-158,182-187; plugins/tower/app/server.mjs:343-350 |
| `cd005-tower-token-dns-rebind` | Tower token authentication is bypassed for DNS-rebound loopback requests | plugins/tower/app/server.mjs | 2 | confirmed | plugins/tower/app/server.mjs:161-163,205-206,504-506 |
| `tower-tracked-state-priority-xss` | Tracked Tower card priority reaches innerHTML without validation or escaping | plugins/tower/app/store.mjs<br>plugins/tower/app/ui/tower.js | 2 | confirmed | plugins/tower/app/store.mjs:303-329; plugins/tower/app/ui/tower.js:553-562,1016-1022 |
| `tower-ratified-decision-integrity-bypass` | Generic API callers can reopen or delete ratified owner decisions without an owner check | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 1 | confirmed | plugins/tower/app/server.mjs:120-125,457-487; plugins/tower/app/store.mjs:1514-1520,1578-1587 |

## memory-abi-safety

### JIT, WebAssembly, FFI, and ABI memory safety

9 candidates. Priority P0. Milestone `e12-security-runtime`.

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

| Candidate ID | Disposition | HEAD evidence |
|---|---|---|
| `jit-jetarena-vec-layout-casts` | `already-fixed` | The live list hosts use `rt.heap.list_values_mut` and shared `collection_semantics` at `crates/jet-jit/src/Collections.rs:3465-3506`; no `JetArena`-to-`Vec` cast remains. |
| `wasm-list-i64-untrusted-ownership` | `confirmed` | Generated WASM reconstructs `Box<[i64]>` from the host pointer and length at `crates/jet-codegen/src/Codegen/Web.rs:3791-3805` and frees the same unchecked allocation at `crates/jet-codegen/src/Codegen/Web.rs:3812-3818`. |
| `wasm-list-string-untrusted-ownership` | `confirmed` | Generated WASM reconstructs host bytes and trusts the embedded count at `crates/jet-codegen/src/Codegen/Web.rs:3903-3931`, then frees the unchecked pointer at `crates/jet-codegen/src/Codegen/Web.rs:3939-3943`. |
| `wasm-map-untrusted-ownership` | `confirmed` | Generated WASM reconstructs host bytes and trusts serialized counts and lengths at `crates/jet-codegen/src/Codegen/Web.rs:3974-4012`, then frees the unchecked pointer at `crates/jet-codegen/src/Codegen/Web.rs:4019-4024`. |
| `wasm-string-untrusted-ownership` | `confirmed` | Generated WASM reconstructs `Box<[u8]>` from the host pointer and length at `crates/jet-codegen/src/Codegen/Web.rs:3742-3756` and frees the unchecked pointer at `crates/jet-codegen/src/Codegen/Web.rs:3763-3768`. |
| `d0017-s1-aot-termios-layout` | `confirmed` | AOT embeds the shared terminal Prelude at `crates/jet-codegen/src/Codegen/mod.rs:255-260`; its hand-written Unix `Termios` reaches `tcgetattr`/`tcsetattr` at `crates/jet-codegen/src/Prelude/Term.rs:446-500` from `crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:488-507`. |
| `d0017-s1-jit-termios-layout` | `duplicate-of-d0017-s1-aot-termios-layout` | JIT includes the same terminal Prelude at `crates/jet-jit/src/IO.rs:21-23` and calls `jet_term_input_secret` at `crates/jet-jit/src/IO.rs:257-272`; the shared `Termios` FFI is at `crates/jet-codegen/src/Prelude/Term.rs:446-500`. |
| `jit-http-worker-runtime-uaf` | `confirmed` | HTTP handlers call through the published raw runtime pointer at `crates/jet-jit/src/net_http_hosts.rs:527-564,2247-2279` via `crates/jet-jit/src/Concurrency.rs:339-372`; resident teardown drops runtime/module before clearing that pointer at `crates/jet-jit/src/jit/resident.rs:279-299`. |
| `jit-event-four-capture-abi` | `confirmed` | JIT callback invocation supports only four capture slots at `crates/jet-jit/src/Reactive.rs:31-62`, event payload insertion is capped at four at `crates/jet-jit/src/Reactive.rs:360-404,722-772`, and lowering passes the payload-bearing callback through `crates/jet-jit/src/jit/lower_ctx.rs:24080-24211` while compilation adds captures plus parameters at `crates/jet-jit/src/jit/functions_compile.rs:448-476` and `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs:3508-3569`. |

## identity-secrets-crypto

### Identity, tokens, secrets, and cryptography

12 candidates. Priority P0. Milestone `e12-security-data`.

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

| Candidate ID | Disposition | HEAD evidence |
|---|---|---|
| `cd005-auth-predictable-session-id` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:116-125,140-153,293-303,333-359` — password, magic-link, and OAuth sessions use CSPRNG-backed opaque IDs; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:2828-2862` lowers the public calls to those Prelude helpers. |
| `cd005-auth-predictable-magic-token` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:116-125,237-264,267-303` — issue uses a CSPRNG-backed opaque token and consume checks it before minting a session; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:2837-2849` exposes the same helpers. |
| `cd005-auth-oauth-predictable-state` | `confirmed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:306-318,321-343` — state is random but stored only with its provider, and finish accepts it without an initiating-browser binding; `crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs:636-643,661-664` exposes no browser/session binding argument. |
| `cd005-auth-oauth-unverified-subject` | `confirmed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:321-359` — syntax-only caller subject becomes `provider:subject`, creates a user, and receives a session; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:2856-2862` lowers the path and `crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs:636-643` accepts the subject. |
| `notebook-zero-token-fallback` | `already-fixed` | `Source/CmdNotebook.rs:56-67,131-137` — token creation uses `read_exact` and entropy failure exits with an error; no all-zero fallback reaches `serve_loopback`. |
| `signing-key-permission-fail-open` | `confirmed` | `Source/Publish/Sign.rs:110-121,401-408` — seed bytes are written before best-effort `set_mode(0600)`, and permission errors are ignored. |
| `auth-session-show-token-leak` | `confirmed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:156-164,466-468` — `session_show` formats the live `session.id`; `crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs:653-656` exposes the method. |
| `archive-key-predictable-fallback` | `already-fixed` | `crates/jetpack/src/Store/Archive.rs:1385-1407,1439-1443` — automatic archive-key creation calls bounded OS CSPRNG entropy and fails on error; `crates/jetpack/src/TrustRoot.rs:47-113` has no predictable fallback. |
| `claude-config-credential-exfil` | `confirmed` | `.claude/settings.local.json:3-8` — tracked permissions allow reads below `~/.config` and unrestricted `gh api`; regression proof is a repository config check, not a cargo test. |
| `comptime-aes-table-timing-side-channel` | `confirmed` | `crates/jet-comptime/src/Comptime/CryptoLite/Aes256Gcm.rs:53-62,82-85,133-151` — secret-dependent S-box lookups and GHASH branches remain; `crates/jet-comptime/src/Comptime/CorePureParity.rs:1774-1834` dispatches the comptime AEAD calls to this implementation. |
| `comptime-argon2id-nonstandard` | `confirmed` | `crates/jet-comptime/src/Comptime/CryptoLite/Argon2id.rs:338-390,401-422` — simplified address generation and final-block handling remain; `crates/jet-comptime/src/Comptime/CorePureParity.rs:1844-1904` returns its output from `expert.argon2id`. |
| `managed-secret-temp-permission-window` | `confirmed` | `crates/jetpack/src/EnvFiles.rs:370-379,472-507,579-604` — temporary files are created with default permissions, written and synced, then restricted before publication. |

## resource-bounds

### Resource bounds, parser depth, and service availability

35 candidates. Priority P1. Milestone `e12-security-runtime`.

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
| `compiler-extension-json-depth-dos` | Shared package-model JSON parser permits stack-exhausting nesting | crates/jet-pkg-model/src/JSON.rs<br>crates/jet-pkg-model/src/CompilerExtension.rs | 3 |
| `worktree-tar-entry-count-dos` | Differing worktree TAR parser has unbounded entry count and quadratic accounting |  | 3 |
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
| `package-treehash-symlink-recursion` | Package hashing follows directory symlinks without cycle or root containment checks |  | 1 |
| `archive-urandom-read-to-eof` | Archive key generation reads /dev/urandom to EOF | crates/jetpack/src/Store.rs<br>crates/jetpack/src/Store/Archive.rs | 1 |
| `module-discovery-symlink-recursion-dos` | Recursive module discovery follows directory symlink cycles without visited-set or depth limit | crates/jet-driver/src/Loader.rs<br>crates/jet-pkg-model/src/PackageManifest/Discovery.rs | 1 |

### Current dispositions

These dispositions trace all 35 candidates through the current source. `confirmed` means the reported source-to-sink path remains live. `already-fixed` means the current source removes the reported path. Each row cites current file:line evidence.

| Candidate ID | Disposition | File:line evidence |
|---|---|---|
| `zip-resident-unbounded-output` | `already-fixed` | `crates/jet-comptime/src/Comptime/ArchiveLite.rs:283-288` delegates resident ZIP decompression to the canonical archive kernel; `corelib/core.archive/pkgs/archive/src/lib.rs:128-149,180-189` bounds entry count and expanded output. |
| `d0002-s1-http1-slow-body` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs:2860-2969` starts a fixed body deadline when body parsing begins and uses it alongside the idle timeout, so successful trickle reads cannot extend the request indefinitely. |
| `zip-runtime-unbounded-output` | `already-fixed` | `corelib/core.archive/pkgs/archive/src/lib.rs:128-149,180-189` rejects oversized entry counts and declared/actual expanded ZIP output before returning materialized bytes. |
| `tar-unbounded-materialization` | `already-fixed` | `corelib/core.archive/pkgs/archive/src/lib.rs:13-14,447-483,500` enforces 4096 entries, 64 MiB per-entry output, and 64 MiB aggregate materialization. |
| `s0-devserver-unbounded-header-line` | `already-fixed` | `crates/jet-devserver/src/lib.rs:24-27,55-98,152-166` bounds each request/header line and enforces cumulative header bytes and header count. |
| `s0-jit-http-simple-unbounded-response` | `already-fixed` | `crates/jet-jit/src/net_http_hosts.rs:2029-2068,2099-2113` returns a streaming body bridge for simple calls; `crates/jet-pkg-model/src/Prelude/HTTP.rs:4046-4049,4196-4205,4286-4321` bounds declared and streamed response bytes. |
| `s0-jit-http-request-unbounded-response` | `already-fixed` | `crates/jet-jit/src/net_http_hosts.rs:2071-2096,2509-2517` routes configurable requests through the shared response bridge; `crates/jet-pkg-model/src/Prelude/HTTP.rs:4046-4049,4196-4205,4286-4321` bounds the response body. |
| `s1-root-net-file-unbounded` | `already-fixed` | `crates/jet-net/src/lib.rs:182-209` routes caller-selected `file://` reads through `read_limited`, which stops at the 64 MiB fetch budget. |
| `s1-root-net-http-unbounded` | `already-fixed` | `crates/jet-net/src/lib.rs:182-209` routes caller-selected HTTP responses through `read_limited`, which stops at the 64 MiB fetch budget. |
| `s2-fenced-range-expansion-dos` | `confirmed` | `crates/jet-parser/src/FencedNames.rs:450-476` collects every number in an ascending named range without a cardinality bound. |
| `s2-runtime-json-depth-dos` | `already-fixed` | `crates/jet-foundation/src/EncodingJson.rs:3,87-98,250-292` applies `MAX_JSON_DEPTH` in the shared recursive parser; `crates/jet-codegen/src/Prelude/CoreLib/JetStd/JSONCodec.rs:1-5` uses that parser. |
| `s2-comptime-json-depth-dos` | `already-fixed` | `crates/jet-comptime/src/Comptime/JSONInterp.rs:147-160` delegates both comptime JSON paths to `EncodingJson`; `crates/jet-foundation/src/EncodingJson.rs:250-292` rejects excessive nesting. |
| `s2-comptime-toml-depth-dos` | `already-fixed` | `crates/jet-comptime/src/Comptime/EncodingLite.rs:628-636,830-845,877-894` checks `MAX_TOML_DEPTH` and passes depth through array and inline-table recursion. |
| `s2-comptime-yaml-depth-dos` | `already-fixed` | `crates/jet-comptime/src/Comptime/EncodingLite.rs:1104-1148,1276-1309,1327-1359` checks `MAX_YAML_DEPTH` in block and flow recursion. |
| `runtime-toml-depth-dos` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/JetStd/TOML.rs:10,374-382,578-592,621-638` checks `MAX_TOML_DEPTH` and carries depth through nested values. |
| `runtime-yaml-depth-dos` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/JetStd/YAML.rs:10,82-105,109-139,235-273,288-323` checks `MAX_YAML_DEPTH` in block and flow parsing. |
| `gzip-runtime-unbounded-output` | `already-fixed` | `crates/jet-pkg-model/src/Prelude/Compress.rs:31-44` limits gzip reads to 64 MiB plus one sentinel byte and rejects overflow. |
| `zstd-runtime-unbounded-output` | `already-fixed` | `crates/jet-pkg-model/src/Prelude/Compress.rs:53-68` limits zstd reads to 64 MiB plus one sentinel byte and rejects overflow. |
| `processspec-output-limit-late` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs:422-465,1981-2068,2133-2143` drains output under a shared budget, kills on overflow, and returns `ResourceLimit`; `crates/jet-jit/src/Process.rs:315-344,367-369` calls the Prelude. |
| `build-action-unbounded-output` | `confirmed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/ProcessSandbox.rs:318-347` captures sandbox stdout/stderr with `wait_with_output`; `crates/jet-comptime/src/Comptime/Build/execution_runtime.rs:849-864` consumes the unbounded `Output` after completion. |
| `notebook-preauth-slowloris` | `already-fixed` | `Source/CmdNotebook.rs:186-204` sets a 10-second read timeout before request parsing; `Source/CmdNotebook.rs:444-459` also caps headers at 64 KiB. |
| `plugin-call-unbounded-resources` | `already-fixed` | `crates/jet-pkg-model/src/Prelude/Plugin.rs:33-74,95-105` configures fuel, epoch interruption, linear-memory, table, instance, and memory limits; `crates/jet-pkg-model/src/Prelude/Plugin.rs:147-170` arms the two-second call timer and argument budget. |
| `d0017-s1-tar-pax-allocation` | `confirmed` | `crates/jetpack/src/Provider/fetch.rs:351-397` checks PAX logical size but then allocates metadata with `Vec::with_capacity(stored as usize)`, leaving stored-size allocation unchecked. |
| `d0017-s3-studio-slowloris` | `confirmed` | `crates/jetpack/src/CLI/studio_server.rs:88-114,192-203` services connections serially and blocks in `read` without a socket read timeout. |
| `compiler-extension-json-depth-dos` | `already-fixed` | `crates/jet-pkg-model/src/CompilerExtension.rs:484-488` uses `JSON::parse`; `crates/jet-foundation/src/JSON.rs:43-49,80-85,159-199` bounds input and recursive JSON depth. |
| `nix-json-depth-dos` | `already-fixed` | `crates/jet-nix-eval/src/JSON.rs:6,42-53,80-83,115-138` passes depth through JSON recursion and rejects values beyond the evaluator limit. |
| `process-pipeline-limits-ignored` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs:1645-1777` starts per-stage output drains and enforces timeout/output limits; `crates/jet-jit/src/Process.rs:325-344` calls that Prelude pipeline. |
| `envhook-pretrust-symlink-recursion-dos` | `already-fixed` | `crates/jetpack/src/EnvHook.rs:279-317,363-392` rejects external links, records cycles with a visited set, and does not recurse through directory symlink entries; `crates/jetpack/src/CLI/run_enter_dev.rs:2814-2820` uses this bounded fingerprint before activation. |
| `nix-expression-parser-depth-dos` | `already-fixed` | `crates/jet-nix-eval/src/Evaluator.rs:627-670` tracks parser depth and returns `ResourceLimit` before nested expression parsing exceeds `MAX_EVAL_DEPTH`. |
| `package-treehash-symlink-recursion` | `confirmed` | `crates/jet-foundation/src/SHA256.rs:186-230` recurses on `Path::is_dir` without symlink metadata, a visited set, or root containment; `Source/Fetch.rs:755-756,831-839` uses this tree hash for fetched packages. |
| `devserver-slowloris-thread-exhaustion` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:35,1414-1429,1452-1466,1483-1486` caps active connection threads at 64 and applies a ten-second socket read timeout before the bounded request reader. |
| `archive-urandom-read-to-eof` | `already-fixed` | `crates/jetpack/src/Store/Archive.rs:1399-1407,1439-1443` obtains exactly 32 bytes through the bounded `os_random_bytes` helper and fails on entropy errors. |
| `embedded-devserver-unbounded-http-resource-use` | `already-fixed` | `crates/jet-codegen/src/Prelude/DevServer.rs:36-39,343-377,379-394,396-430` caps lines, aggregate headers, and active connection threads, and sets ten-second socket timeouts before parsing. |
| `worktree-tar-entry-count-dos` | `already-fixed` | `corelib/core.archive/pkgs/archive/src/lib.rs:13-14,447-483` is the current canonical TAR parser and rejects more than 4096 entries while bounding materialized output and aggregate bytes. |
| `module-discovery-symlink-recursion-dos` | `already-fixed` | `crates/jet-pkg-model/src/Package/Discovery.rs:31-38` routes discovery through `AuthorityResolver`; `crates/jet-pkg-model/src/Authority.rs:803-810,1035-1048` rejects symlinks before recursive directory descent. |

## network-boundaries

### Network egress, SSRF, HTTP framing, and local disclosure

6 candidates. Priority P1. Milestone `e12-security-data`.

| Candidate ID | Discovery title | Primary locations | Disposition | File:line evidence | Source reports |
|---|---|---|---|---|---:|
| `comptime-fetch-ssrf` | Hash-pinned compile-time fetch permits arbitrary outbound requests before verification | crates/jet-comptime/src/Comptime/Methods/dispatch.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch/eval_method.rs | confirmed | `crates/jet-comptime/src/Comptime/Methods/dispatch/eval_method.rs:763-765` — the fetch route has no effect gate; `crates/jet-net/src/lib.rs:180-188` — the caller URL selects a file or HTTP(S) fetch before the hash check | 11 |
| `cd005-comptime-fetch-local-disclosure` | Hermetic compile-time fetch can disclose arbitrary local text files | crates/jet-net/src/lib.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch.rs | confirmed | `crates/jet-net/src/lib.rs:180-183` — a caller-selected `file://` path reaches `std::fs::read`; `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:737-756` — the read occurs before digest verification | 9 |
| `provider-registry-private-network-ssrf` | Project provider policy can authorize private-network HTTPS fetches | crates/jetpack/src/Provider/fetch.rs<br>crates/jetpack/src/Provider/script_registry.rs | confirmed | `crates/jetpack/src/Provider/fetch.rs:204-212` — policy checks only textual allow/deny authority; `crates/jetpack/src/Provider/fetch.rs:520-540` — host validation does not classify resolved addresses; `crates/jetpack/src/Provider/script_registry.rs:141-155` — provider URLs reach the fetch sink | 5 |
| `jit-http-crlf-injection` | JIT generic HTTP request serialization permits CRLF request injection | crates/jet-jit/src/net_http_hosts.rs<br>crates/jet-pkg-model/src/Prelude/HTTP.rs | confirmed | `crates/jet-jit/src/net_http_hosts.rs:2421-2426,2509-2515` — JIT strings reach the shared HTTP sender; `crates/jet-pkg-model/src/Prelude/HTTP.rs:1623-1635,3428-3442` — the request target is serialized without control-byte rejection | 4 |
| `jit-websocket-handshake-crlf-injection` | WebSocket URL permits HTTP handshake CRLF injection | crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs | confirmed | `crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs:247-283` — URL path and authority accept control bytes; `crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs:573-598` — path and host are written into the raw handshake | 1 |
| `git-dependency-transport-ssrf` | Git dependency fetch allows attacker-selected network destinations |  | confirmed | `Source/Fetch.rs:2301-2304` — the manifest URL reaches `git ls-remote`; `Source/Fetch.rs:2332-2349` — the same URL reaches `git clone` without destination policy | 1 |

## filesystem-containment

### Filesystem roots, symlinks, temporary files, and path containment

22 candidates. Priority P1. Milestone `e12-security-data`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `package-store-install-symlink-escape` | Package-store installation follows dependency symlinks outside the source tree | Source/Fetch.rs<br>Source/Store.rs | 20 |
| `canvas-create-package-symlink-write` | Canvas `create_package` writes outside the project through symlink ancestors | crates/jet-devserver/src/Canvas/schema_api.rs<br>crates/jet-devserver/src/Canvas/project_transactions.rs | 15 |
| `dependency-name-path-traversal` | Unvalidated package identifiers escape store, project, and registry roots | crates/jet-pkg-model/src/PackageManifest/ParseBlocks.rs<br>crates/jet-pkg-model/src/PackageManifest/Convert.rs | 13 |
| `devserver-static-symlink-escape` | Devserver static reads follow symlinks outside the build directory | crates/jet-devserver/src/lib.rs<br>crates/jet-devserver/src/WebHost.rs | 7 |
| `d0002-s2-sparse-copy-symlink` | Sparse remote fetch fallback follows dependency-controlled symlinks while copying | crates/jetpack/src/Provider/remote.rs<br>crates/jetpack/src/Provider/package.rs | 6 |
| `s0-web-test-prefix-traversal` | Web-test file server uses prefix-only path containment | scripts/web-test/serve.mjs | 4 |
| `vendor-symlink-escape` | Vendoring follows dependency symlinks outside the source tree | Source/Publish/Vendor.rs<br>Source/Fetch.rs | 4 |
| `cd005-comptime-embed-file-symlink` | Compile-time embed_file follows project symlinks outside the source root | crates/jet-comptime/src/Comptime/Methods/dispatch.rs | 2 |
| `cd005-comptime-embed-bytes-symlink` | Compile-time embed_bytes follows project symlinks outside the source root | crates/jet-comptime/src/Comptime/Methods/dispatch.rs | 2 |
| `cd005-build-embed-symlink` | BuildContext embed follows project symlinks outside the source root | crates/jet-comptime/src/Comptime/Methods/dispatch.rs<br>crates/jet-comptime/src/Comptime/eval_method.rs | 2 |
| `git-revision-cache-path-traversal` | Manifest Git revisions escape the cache root and shape recursive deletion | crates/jet-pkg-model/src/PackageManifest/ParseBlocks.rs<br>Source/Fetch.rs | 2 |
| `jetpack-dotenv-symlink-read` | Project-relative dotenv validation follows symlinks outside the project | crates/jetpack/src/CLI/trust_env_build.rs<br>crates/jet-env-model/src/ModuleEval/Environment.rs | 2 |
| `jetpack-image-files-read-traversal` | Image files entries can read arbitrary host paths outside the project | crates/jetpack/src/CLI/add_remove_push_image.rs<br>crates/jetpack/src/Image.rs | 2 |
| `jetpack-image-layer-path-traversal` | OCI tar builder emits unvalidated and silently truncated attacker-controlled paths | crates/jetpack/src/Image.rs<br>crates/jet-env-model/src/ModuleEval/System.rs | 2 |
| `canvas-action-temp-symlink-overwrite` | Predictable Canvas check file follows workspace symlinks | crates/jet-devserver/src/Canvas/edit_actions.rs | 2 |
| `jetpack-overlay-patch-path-traversal` | A malicious overlay patch can overwrite files outside the source root | crates/jetpack/src/Overlay.rs | 2 |
| `trust-prefix-sibling-overmatch` | Trust path prefix matching authorizes sibling project names | crates/jetpack/src/Trust.rs | 1 |
| `devserver-build-symlink-overwrite` | Project-controlled build symlink redirects finalized web outputs to host paths | Source/CmdCompile.rs<br>crates/jet-codegen/src/Prelude/DevServer.rs | 1 |
| `lsp-predictable-log-symlink-write` | LSP panic logging follows a predictable shared temporary symlink | Source/LSP/Server.rs | 1 |
| `canvas-source-symlink-read` | Canvas projected-source scan follows directory symlinks and can disclose an external Jet file | crates/jet-devserver/src/Canvas/project_scan.rs<br>crates/jet-devserver/src/Canvas/schema_api.rs | 1 |
| `jetpack-remote-symlink-fingerprint-escape` | Remote package fingerprint traversal follows symlinks outside the checkout | crates/jetpack/src/Provider/remote.rs<br>crates/jetpack/src/Provider.rs | 1 |
| `repl-run-temp-symlink-overwrite` | REPL :run writes predictable files in the shared temporary directory |  | 1 |

### Current dispositions

These dispositions are source-backed traces against the current tree. `confirmed` means the reported source-to-sink path remains live. `already-fixed` means the current source removes the reported path. Each row cites current file:line evidence.

| Candidate ID | Disposition | HEAD evidence |
|---|---|---|
| `package-store-install-symlink-escape` | confirmed | `Source/Store.rs:31-58,107-115,223-260` — raw store/link paths reach recursive `is_dir` and copy operations; `Source/Fetch.rs:779-792,889-895,1197-1204` — dependency sources reach those paths. |
| `canvas-create-package-symlink-write` | confirmed | `crates/jet-devserver/src/Canvas/project_transactions.rs:518-555,1047-1081,1287-1314` — lexical-only project paths are joined and written; `crates/jet-devserver/src/Canvas/source_model.rs:45-65,78-126` — the write reads and renames the caller path without no-follow containment. |
| `dependency-name-path-traversal` | confirmed | `crates/jet-pkg-model/src/Package/Blocks.rs:20-33,360-405` — dependency keys are retained without safe-component validation; `crates/jet-pkg-model/src/Package/Convert.rs:19-74` and `Source/Fetch.rs:787-792,890-895,1198-1204` preserve and join the raw name. |
| `devserver-static-symlink-escape` | confirmed | `crates/jet-devserver/src/lib.rs:180-189` — static paths only reject the substring `..`; `crates/jet-devserver/src/WebHost.rs:2126-2140` joins and reads the accepted path without real-root containment. |
| `d0002-s2-sparse-copy-symlink` | already-fixed | `crates/jetpack/src/Provider/remote.rs:286-296,648-675` — the sparse-copy fallback checks `symlink_metadata` and rejects symlink or non-file source entries before copying. |
| `s0-web-test-prefix-traversal` | confirmed | `scripts/web-test/serve.mjs:27-32,35-54` — decoded paths use string-prefix containment, then `statSync` and `createReadStream`, with no component-boundary or realpath check. |
| `vendor-symlink-escape` | confirmed | `Source/Publish/Vendor.rs:39-55,110-124` — vendoring recursively uses `is_dir` and `fs::copy`, both following dependency symlinks. |
| `cd005-comptime-embed-file-symlink` | confirmed | `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:290-304,393-415` — only lexical absolute/parent checks run before `fs::read(base_dir.join(rel))`. |
| `cd005-comptime-embed-bytes-symlink` | confirmed | `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:290-304,393-425` — only lexical absolute/parent checks run before the bytes read follows `base_dir.join(rel)`. |
| `cd005-build-embed-symlink` | confirmed | `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:480-513` — `b.embed` performs lexical-only validation before `fs::read(base_dir.join(path))`. |
| `git-revision-cache-path-traversal` | confirmed | `crates/jet-pkg-model/src/Package/Blocks.rs:451-480` — a manifest `rev` is retained verbatim; `Source/Fetch.rs:819-824,2289-2299,2332-2386` uses it to shape cache creation, cleanup, and clone paths. |
| `jetpack-dotenv-symlink-read` | confirmed | `crates/jetpack/src/CLI/trust_env_build.rs:668-683,990-994` — a lexical-only relative path reaches `read_to_string`; `crates/jet-env-model/src/ModuleEval/Environment.rs:2414-2425` has no physical-root check. |
| `jetpack-image-files-read-traversal` | already-fixed | `crates/jetpack/src/CLI/add_remove_push_image.rs:1121-1188` — path components are normalized, symlinks are rejected, the target is canonicalized, and canonical containment is enforced before the bounded read. |
| `jetpack-image-layer-path-traversal` | already-fixed | `crates/jetpack/src/Image.rs:364-376,1362-1400` — every layer path is checked for absolute, parent, prefix, control-byte, length, duplicate, and collision violations before tar emission. |
| `canvas-action-temp-symlink-overwrite` | confirmed | `crates/jet-devserver/src/Canvas/edit_actions.rs:950-972` — a predictable sibling path is written with `fs::write` and removed afterward, without exclusive or symlink-safe creation. |
| `jetpack-overlay-patch-path-traversal` | already-fixed | `crates/jetpack/src/Overlay.rs:21-44,67-124,268-309` — patch and target paths reject symlink components and require canonical containment before read or staged write. |
| `trust-prefix-sibling-overmatch` | confirmed | `crates/jetpack/src/Trust.rs:616-658` — exact and wildcard grants use `starts_with` without a path-component boundary, including canonical subjects. |
| `devserver-build-symlink-overwrite` | confirmed | `Source/CmdCompile.rs:5636-5651,5688-5707,5774-5783`, `crates/jet-devserver/src/WebHost.rs:1009-1038`, and `crates/jet-codegen/src/Prelude/DevServer.rs:302-316` write or rename web outputs through `build` without real-root validation. |
| `lsp-predictable-log-symlink-write` | confirmed | `Source/LSP/Server.rs:269-298` turns a caught handler panic into an append through fixed `/tmp/jet-lsp.log`; ordinary `OpenOptions` follows a prepositioned symlink. |
| `canvas-source-symlink-read` | already-fixed | `crates/jet-devserver/src/Canvas/project_scan.rs:453-465,477-525`, `crates/jet-devserver/src/Canvas/schema_api.rs:287-310`, and `crates/jet-pkg-model/src/Authority.rs:776-800,1035-1063` route discovered files through descriptor-relative no-follow checks and reject symlink entries. |
| `jetpack-remote-symlink-fingerprint-escape` | confirmed | `crates/jetpack/src/Provider/remote.rs:608-644` recursively follows directory symlinks and reads them through `fs::read`/`metadata`; `crates/jetpack/src/Provider.rs:638-652` applies it to staged provider source. |
| `repl-run-temp-symlink-overwrite` | confirmed | `crates/jet-repl/src/lib.rs:1232-1239,1282-1301` creates a PID/counter-predictable shared-temp name, writes it with `fs::write`, and later removes it without exclusive or no-follow creation. |

## devtools-control-plane

### Devserver, Canvas, Studio, and notebook control planes

10 candidates. Priority P0. Milestone `e12-security-boundaries`.

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

These dispositions trace all 10 candidates through the current source. Each row cites current file:line evidence.

| Candidate ID | Disposition | File:line evidence |
|---|---|---|
| `devserver-cross-origin-mutation` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:1241-1297,1496-1500` rejects requests with an invalid Host, Origin, or Canvas session before any Canvas route. |
| `s1-root-studio-remote-read` | `confirmed` | `crates/jetpack/src/CLI/bridge_os_studio.rs:122-140` accepts the requested bind address; `crates/jetpack/src/CLI/studio_server.rs:108-170` returns project data and config.jet without request authentication. |
| `s1-root-studio-remote-write` | `confirmed` | `crates/jetpack/src/CLI/studio_server.rs:128-136` forwards unauthenticated transactions; `crates/jetpack/src/CLI/studio_transactions.rs:104-137,323-385` issues sessions to callers and applies their changes to config.jet. |
| `s1-root-studio-remote-run` | `confirmed` | `crates/jetpack/src/CLI/studio_server.rs:138-146` forwards unauthenticated run requests; `crates/jetpack/src/CLI/studio_transactions.rs:631-695,1107-1147` dispatches them to JetOS subprocess actions. |
| `s1-root-studio-loopback-csrf` | `confirmed` | `crates/jetpack/src/CLI/bridge_os_studio.rs:131-140` starts the default loopback service; `crates/jetpack/src/CLI/studio_server.rs:108-146,194-208` parses POST requests without Host or Origin validation. |
| `s2-devserver-source-disclosure` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:1241-1297,1496-1500` requires the Canvas Host, Origin, and session checks before the graph, project, and source routes at `1638-1786`. |
| `s2-devserver-debug-trigger` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:1241-1297,1496-1500` requires the Canvas Host, Origin, and session checks before the debug route at `1898-1923`. |
| `devserver-windows-absolute-static-path` | `confirmed` | `crates/jet-devserver/src/lib.rs:215-224` rejects only `..` before joining the request path; `crates/jet-devserver/src/WebHost.rs:2154-2167` reads the joined path. |
| `canvas-project-revision-not-enforced` | `already-fixed` | `crates/jet-devserver/src/Canvas/schema_api.rs:527-545` parses `project_revision` and compares it with the current project revision before dispatch. |
| `embedded-devserver-windows-absolute-static-path` | `confirmed` | `crates/jet-codegen/src/Prelude/DevServer.rs:460-475` rejects only `..` before `Path::join`, then reads the resulting path. |

## command-code-injection

### Command, shell, editor, and generated-code injection

20 candidates. Priority P1. Milestone `e12-security-runtime`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `git-ls-remote-option-injection` | Manifest-controlled Git URL can inject `git ls-remote` options | Source/Fetch.rs<br>crates/jet-pkg-model/src/PackageManifest/ParseBlocks.rs | 25 |
| `git-clone-option-injection` | Manifest-controlled Git URL can inject `git clone` options | Source/Fetch.rs<br>crates/jet-pkg-model/src/PackageManifest/ParseBlocks.rs | 10 |
| `s0-bash-prompt-label-injection` | Project prompt label is embedded unescaped in generated Bash startup code | crates/jetpack/src/Shell.rs<br>crates/jetpack/src/EnvFile.rs | 6 |
| `package-git-fetch-option-injection` | Package Git revision fragments can inject fetch options | crates/jetpack/src/Provider/remote.rs | 6 |
| `s0-zsh-prompt-label-injection` | Project prompt label is embedded unescaped in generated Zsh startup code | crates/jetpack/src/Shell.rs<br>crates/jet-env-model/src/ModuleEval/Eval.rs | 4 |
| `s0-fish-prompt-label-injection` | Project prompt label is embedded unescaped in Fish startup commands | crates/jetpack/src/Shell.rs<br>crates/jetpack/src/EnvFile.rs | 4 |
| `lldb-breakpoint-command-injection` | Generated filenames are interpolated into LLDB command text | Source/CmdCompile.rs<br>crates/jet-debug/src/Inferior.rs | 3 |
| `vscode-workspace-lsp-rce` | VS Code extension auto-executes a workspace-controlled language server binary | editors/vscode/extension.js<br>editors/vscode/package.json | 3 |
| `zed-worktree-lsp-rce` | Zed extension selects a worktree-controlled language server binary | editors/zed/wasm-src/src/lib.rs<br>editors/zed/extension.toml | 2 |
| `package-git-checkout-option-injection` | Package Git revision fragments can inject checkout options | crates/jetpack/src/Provider/remote.rs | 1 |
| `web-codegen-template-injection` | Jet string literals are emitted raw into generated JavaScript templates | crates/jet-codegen/src/Codegen/Web.rs | 1 |
| `rustc-build-profile-env-injection` | Project build-profile environment reaches the rustc process | Source/main.rs<br>Source/CmdCompile.rs | 1 |
| `jetpack-self-authorized-build-script` | Project policy can self-authorize unsandboxed dependency build script | crates/jetpack/src/Trust.rs<br>crates/jet-pkg-model/src/PackageManifest/mod.rs | 1 |
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
| `git-ls-remote-option-injection` | `already-fixed` | `crates/jet-pkg-model/src/Package/Blocks.rs:453-487` carries the URL and selector as data; `Source/Fetch.rs:813-828,2305-2314,2452-2461` validates both values and places `--` before the Git operands. |
| `git-clone-option-injection` | `already-fixed` | `Source/Fetch.rs:813-828,2342-2348,2363-2365,2452-2461` rejects unsafe URL/revision values and invokes `git clone` with `--` before the URL. |
| `s0-bash-prompt-label-injection` | `confirmed` | `crates/jetpack/src/EnvFile.rs:57-63` and `crates/jet-env-model/src/ModuleEval/Eval.rs:1282-1297` preserve the label; `crates/jetpack/src/Shell.rs:791-799,975-979` embeds it in Bash startup code. |
| `s0-zsh-prompt-label-injection` | `confirmed` | `crates/jetpack/src/EnvFile.rs:57-63` and `crates/jet-env-model/src/ModuleEval/Eval.rs:1282-1297` preserve the label; `crates/jetpack/src/Shell.rs:801-810,1015-1019` embeds it in Zsh startup code. |
| `s0-fish-prompt-label-injection` | `confirmed` | `crates/jetpack/src/EnvFile.rs:57-63` and `crates/jet-env-model/src/ModuleEval/Eval.rs:1282-1297` preserve the label; `crates/jetpack/src/Shell.rs:812-815,1049-1053` embeds it in Fish startup code. |
| `package-git-fetch-option-injection` | `already-fixed` | `crates/jetpack/src/Provider/remote.rs:418-487` preserves `#rev` but rejects control characters and leading `-`; `crates/jetpack/src/Provider/remote.rs:206-221` rechecks before `git fetch`. |
| `package-git-checkout-option-injection` | `already-fixed` | `crates/jetpack/src/Provider/remote.rs:418-487` validates the parsed revision; `crates/jetpack/src/Provider/remote.rs:543-554` rechecks it immediately before the `git checkout` argument. |
| `lldb-breakpoint-command-injection` | `already-fixed` | `crates/jet-debug/src/Inferior.rs:521-532,1214-1225` quotes breakpoint paths and rejects controls; `crates/jet-debug/src/Inferior.rs:1870-1875` proves the hostile path case. |
| `web-codegen-template-injection` | `confirmed` | `crates/jet-codegen/src/Codegen/Web.rs:9781-9809` appends interpolated Jet literal parts directly inside JavaScript backticks; `Source/CmdCompile.rs:5680-5694` writes the generated JS app. |
| `rustc-build-profile-env-injection` | `already-fixed` | `crates/jet-pkg-model/src/Package/Blocks.rs:778-807` rejects retired profile `env`; `Source/main.rs:271-326,358-408` derives only typed profile flags, and `Source/CmdCompile.rs:6494-6526` launches rustc without profile environment injection. |
| `jetpack-self-authorized-build-script` | `already-fixed` | `crates/jetpack/src/CLI/realize.rs:515-529` gates Core Cargo before Store realization; `crates/jetpack/src/Trust.rs:910-966` requires an exact build identity grant or explicit approval, while `crates/jetpack/src/Provider.rs:558-562` excludes project metadata from build authorization. |
| `package-git-kind-probe-option-injection` | `already-fixed` | `crates/jetpack/src/Provider.rs:2220-2237,2289-2304` rejects an unsafe probe revision before `git fetch`; `crates/jetpack/src/Provider/remote.rs:418-487` validates the parsed remote first. |
| `jetos-storage-disk-command-injection` | `confirmed` | `crates/jetpack/src/JetOS/module_storage_workload.rs:56-62` accepts disk input; `crates/jetpack/src/JetOS/module_storage_workload.rs:112-117` writes the expanded value into a script later executed by `sh`. |
| `jetos-storage-esp-command-injection` | `confirmed` | `crates/jetpack/src/JetOS/module_storage_workload.rs:61-62` accepts ESP size input; `crates/jetpack/src/JetOS/module_storage_workload.rs:112-117` interpolates it into generated shell source. |
| `envhook-profile-var-name-shell-injection` | `already-fixed` | `crates/jet-env-model/src/ModuleEval/Environment.rs:2442-2446,2610-2642` validates profile variable names; `crates/jetpack/src/EnvHook.rs:471-480,582-590` validates again at render. |
| `envhook-unset-name-shell-injection` | `already-fixed` | `crates/jet-env-model/src/ModuleEval/Environment.rs:2300-2302,2598-2607` validates unset names; `crates/jetpack/src/EnvHook.rs:582-590,783-788` enforces and tests the render boundary. |
| `claude-hook-relative-path-rce` | `confirmed` | `.claude/settings.json:24-45` runs `scripts/agent/*` through cwd-relative lookup before the `$CLAUDE_PROJECT_DIR` fallback. |
| `vscode-workspace-lsp-rce` | `already-fixed` | `editors/vscode/extension.js:25-50,114-116,151-159` still selects the workspace/server command for trusted workspaces; `editors/vscode/package.json:21-24` disables the extension in untrusted workspaces before activation. |
| `zed-worktree-lsp-rce` | `confirmed` | `editors/zed/wasm-src/src/lib.rs:12-39` selects `{worktree}/target/debug/jet` and launches it; `editors/zed/extension.toml:15-23` grants process execution. |
| `perl-bind-compile-exec` | `confirmed` | `Source/CmdDevTools.rs:3751-3817` invokes `PerlBind::bind`; `crates/jet-pkg-model/src/PerlBind.rs:40-68,139-153` runs Perl `-c` with compile-phase hooks. |

## package-supply-chain

### Package, Git, provider, store, and dependency integrity

7 candidates. Priority P1. Milestone `e12-security-data`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `package-store-incomplete-content-hash` | Package store integrity hashes omit copied non-.jet files | Source/Store.rs<br>crates/jet-foundation/src/SHA256.rs | 10 |
| `jetpack-typed-environment-trust-bypass` | Typed Jetpack environments bypass trust for lifecycle hooks and services when packages and secrets are empty | crates/jetpack/src/Trust.rs<br>crates/jetpack/src/CLI/run_enter_dev.rs | 8 |
| `locked-dependency-integrity-bypass` | Locked builds use mutable dependency sources without content verification | Source/Fetch.rs | 1 |
| `git-revision-utf8-slice-panic` | Multibyte Git revisions panic dependency resolution at a byte boundary | crates/jet-pkg-model/src/PackageManifest/ParseBlocks.rs<br>Source/Fetch.rs | 1 |
| `buildrecipe-exec-unsandboxed` | BuildRecipe run executes tools with ambient host authority | crates/jetpack/src/Recipe.rs | 1 |
| `buildrecipe-logged-unsandboxed` | Logged BuildRecipe execution bypasses the promised sandbox | crates/jetpack/src/Provider.rs<br>crates/jetpack/src/Recipe.rs | 1 |
| `transitive-path-dependency-escape` | Transitive path dependencies can escape their fetched dependency root |  | 1 |

### Current dispositions

These dispositions trace the seven owned candidates against the current source. `confirmed` means the reported path remains live. `already-fixed` means the current source removes the reported path.

| Candidate ID | Disposition | HEAD evidence |
|---|---|---|
| `locked-dependency-integrity-bypass` | `confirmed` | `Source/Fetch.rs:54-82,1929-1947` — locked path and Git dependencies return project or cache directories without store content verification. |
| `package-store-incomplete-content-hash` | `confirmed` | `crates/jet-foundation/src/SHA256.rs:203-238` hashes only `.jet` files, while `Source/Store.rs:131-150,223-237` verifies that partial hash after copying the wider non-hidden tree. |
| `git-revision-utf8-slice-panic` | `confirmed` | `crates/jet-pkg-model/src/Package/Blocks.rs:451-487` accepts arbitrary revision text; `Source/Fetch.rs:822-828,2293-2303` slices it at a byte offset. A multibyte revision causes a deterministic crash/DoS, not code execution. |
| `jetpack-typed-environment-trust-bypass` | `confirmed` | `crates/jetpack/src/Trust.rs:528-549,1006-1015` classifies service-only typed environments but returns before typed trust when refs, secrets, and lifecycle hooks are empty; `crates/jetpack/src/CLI/services_secrets_config.rs:571-581,630-655` then starts the service. |
| `buildrecipe-exec-unsandboxed` | `already-fixed` | `crates/jetpack/src/Recipe.rs:968-992,1101-1124,1577-1595,1621-1670` routes recipe execution through the native sandbox and maps unavailable enforcement to an error; `crates/jet-comptime/src/Comptime/Build/execution_runtime.rs:315-317` has no unsandboxed fallback. |
| `buildrecipe-logged-unsandboxed` | `already-fixed` | `crates/jetpack/src/Provider/adapter.rs:57-72` reaches `Recipe::run_logged`; `crates/jetpack/src/Recipe.rs:1027-1064,1599-1670` sends logged execution through the same native sandbox. |
| `transitive-path-dependency-escape` | `confirmed` | `Source/Fetch.rs:722-731,756-785` accepts absolute or normalized `..` paths without containment, then loads, hashes, stores, and links the escaped directory. |

## policy-integrity

### Trust policy, sandbox claims, concurrency, and remaining integrity gaps

2 candidates. Priority P1. Milestone `e12-security-validation`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `secrets-key-permission-window` | Age identity is created with ambient permissions before best-effort chmod | crates/jetpack/src/Secrets.rs | 2 |
| `jetpack-project-trust-self-allow` | Untrusted projects can self-authorize the trust gate that precedes environment hooks | crates/jetpack/src/Trust.rs<br>crates/jetpack/src/CLI/run_enter_dev.rs | 1 |

### Current dispositions

These dispositions trace both candidates through the current source. `confirmed` means the reported source-to-sink path remains live.

| Candidate ID | Disposition | HEAD evidence |
|---|---|---|
| `secrets-key-permission-window` | `confirmed` | `crates/jetpack/src/Secrets.rs:166-191` writes the identity before mode tightening; `crates/jetpack/src/Secrets.rs:675-682` ignores mode-setting errors and uses a no-op on non-Unix platforms. |
| `jetpack-project-trust-self-allow` | `confirmed` | `crates/jetpack/src/Trust.rs:1009-1026,1103-1107` loads project-supplied trust policy and returns success for `TrustDecision::Allow`; `crates/jetpack/src/CLI/run_enter_dev.rs:285-295,1663-1674` calls this gate before environment entry continues. |

## Source artifacts

The repository includes the [full discovery evidence](security-deep-scan-2026-08-03-full.md).

The full report preserves source, control, sink, impact, evidence, preconditions,
uncertainty, CWE data, validation guidance, and source-ledger paths.

This summary is the durable candidate inventory and Tower campaign map.

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
| `POLICY-INTEGRITY` | card | #1387 |
| `SECURITY-GATE` | card | #1387 |
<!-- /audit-dispositions -->
