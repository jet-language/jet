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
| `tower-default-network-auth-bypass` | Tower's default network authentication bypass exposes read and mutation APIs | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 116 | already-fixed | plugins/tower/app/server.mjs:162-177,231-248,483-517,534-543; plugins/tower/test/server.test.mjs:68-103 |
| `tower-docs-symlink-read` | Tower document reads follow symlinks outside the docs root | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 31 | already-fixed | plugins/tower/app/docs.mjs:79-133,198-207,216-221,367-390; plugins/tower/app/server.mjs:367-374; plugins/tower/test/docs.test.mjs:84-115 |
| `tower-owner-authorization-bypass` | Tower grants owner-acceptance authority without establishing owner identity | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 29 | already-fixed | plugins/tower/app/server.mjs:183-205,306-313,445-485; plugins/tower/test/acceptance-queue.test.mjs:326-404 |
| `tower-loopback-csrf` | Tower loopback mutation APIs lack browser-origin and CSRF controls | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 20 | already-fixed | plugins/tower/app/server.mjs:207-220,298-304; plugins/tower/test/server.test.mjs:78-119 |
| `tower-owner-payload-forgery` | Tower trusts caller-supplied owner attribution for privileged mutations | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 18 | already-fixed | plugins/tower/app/server.mjs:429-430,487-521; plugins/tower/app/store.mjs:1525-1530,1607-1618; plugins/tower/test/server.test.mjs:108-118; plugins/tower/test/store.test.mjs:87-95 |
| `tower-docs-symlink-write` | Tower docs API writes through symlinked directories outside the repository | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 17 | already-fixed | plugins/tower/app/docs.mjs:79-133,313-349,367-390; plugins/tower/test/docs.test.mjs:84-101 |
| `tower-docs-symlink-delete` | Tower docs API deletes or moves files through symlinked directories outside the repository | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 14 | already-fixed | plugins/tower/app/docs.mjs:79-133,326-350; plugins/tower/test/docs.test.mjs:84-101 |
| `tower-docs-symlink-walk` | Tower docs inventory recursively traverses symlinked directories | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 4 | already-fixed | plugins/tower/app/docs.mjs:198-221; plugins/tower/test/docs.test.mjs:93-96 |
| `cd005-tower-token-dns-rebind` | Tower token authentication is bypassed for DNS-rebound loopback requests | plugins/tower/app/server.mjs | 2 | already-fixed | plugins/tower/app/server.mjs:162-181,235-249,539-548; plugins/tower/test/server.test.mjs:78-86 |
| `tower-tracked-state-priority-xss` | Tracked Tower card priority reaches innerHTML without validation or escaping | plugins/tower/app/store.mjs<br>plugins/tower/app/ui/tower.js | 2 | already-fixed | plugins/tower/app/store.mjs:303-329; plugins/tower/app/ui/tower.js:560,1020; plugins/tower/test/store.test.mjs:85-89 |
| `tower-ratified-decision-integrity-bypass` | Generic API callers can reopen or delete ratified owner decisions without an owner check | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 1 | already-fixed | plugins/tower/app/server.mjs:121-126,487-521; plugins/tower/app/store.mjs:1514-1530,1582-1593; plugins/tower/test/store.test.mjs:69-85 |

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
| `jit-jetarena-vec-layout-casts` | `already-fixed` | The live list mutation hosts use `JetArena::list_values_mut` at `crates/jet-jit/src/Collections.rs:1668-1677,3670-3719`, which converts carriers explicitly before shared `collection_semantics`; no `JetArena`-to-`Vec` cast remains. |
| `wasm-list-i64-untrusted-ownership` | `already-fixed` | Generated WASM records exact `(kind, ptr, byte_len)` ownership at `crates/jet-codegen/src/Codegen/Web.rs:3812-3847`; fixed-width list argument/free paths require the registered byte length before `Box` reconstruction at `crates/jet-codegen/src/Codegen/Web.rs:3916-3945`. Hostile forged-pointer and count proofs are `tests/web_build.rs:4263-4332`. |
| `wasm-list-string-untrusted-ownership` | `already-fixed` | Generated WASM requires an exact registered allocation before parsing the bounded list-string blob at `crates/jet-codegen/src/Codegen/Web.rs:4041-4067` and before freeing it at `crates/jet-codegen/src/Codegen/Web.rs:4070-4084`. The generated-WASM hostile ownership proof is `tests/web_build.rs:4336-4450`. |
| `wasm-map-untrusted-ownership` | `already-fixed` | Generated WASM requires an exact registered allocation before parsing bounded map bytes at `crates/jet-codegen/src/Codegen/Web.rs:4116-4151` and before freeing it at `crates/jet-codegen/src/Codegen/Web.rs:4154-4168`. The generated-WASM hostile ownership proof is `tests/web_build.rs:4453-4548`. |
| `wasm-string-untrusted-ownership` | `already-fixed` | Generated WASM requires exact registered string ownership before reconstructing or freeing bytes at `crates/jet-codegen/src/Codegen/Web.rs:3863-3890`; the live forged-token and length-mismatch proof is `tests/web_build.rs:4087-4178`. |
| `d0017-s1-aot-termios-layout` | `already-fixed` | AOT embeds the shared terminal Prelude at `crates/jet-codegen/src/Codegen/mod.rs:256-260`; target-specific `Termios` layout, offsets, and size guards reach `tcgetattr`/`tcsetattr` at `crates/jet-codegen/src/Prelude/Term.rs:450-591` through `crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:413-430`. Native-tier inclusion is checked by `tests/os_native.rs:68-97`. |
| `d0017-s1-jit-termios-layout` | `duplicate-of-d0017-s1-aot-termios-layout` | This is the same shared `Termios` FFI issue as `d0017-s1-aot-termios-layout`: JIT includes the canonical Prelude at `crates/jet-jit/src/IO.rs:21-23` and calls `jet_term_input_secret` at `crates/jet-jit/src/IO.rs:267-282`; the shared layout is `crates/jet-codegen/src/Prelude/Term.rs:450-591`. |
| `jit-http-worker-runtime-uaf` | `already-fixed` | HTTP handlers pin the published runtime pointer before loading it at `crates/jet-jit/src/Concurrency.rs:320-388` and invoke through that guard at `crates/jet-jit/src/net_http_hosts.rs:544-575,2264-2289`; both ordinary teardown and hot-swap now stop workers and clear handles under the same boundary at `crates/jet-jit/src/net_http_hosts.rs:57-75` and `crates/jet-jit/src/jit/resident.rs:628-649` before dropping/replacing resident allocations. The ordering proof is `tests/jit_run.rs:266-327`. |
| `jit-event-four-capture-abi` | `already-fixed` | JIT invocation has four native capture slots at `crates/jet-jit/src/Reactive.rs:30-92`, and event/async payload insertion is capped at four at `crates/jet-jit/src/Reactive.rs:360-404,722-772`; lowering now rejects capture-plus-payload arity above four at `crates/jet-jit/src/jit/lower_ctx.rs:24444-24481`, while callback signatures include captures and params at `crates/jet-jit/src/jit/functions_compile.rs:448-461` and payload params are added in `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs:3516-3566`. The AOT/interpreter parity and JIT preflight proof is `tests/jit_run.rs:199-263`. |

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
| `cd005-auth-predictable-session-id` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:109-118,133-146,285-295` — password and magic-link sessions use 32-byte CSPRNG-backed opaque IDs; the OAuth path mints no session; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:2835-2856` lowers the public session calls to the Prelude helpers. |
| `cd005-auth-predictable-magic-token` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:109-118,229-256` — magic-link issue uses a 32-byte CSPRNG-backed opaque token and consume checks it before minting a session; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:2844-2856` exposes the same helpers. |
| `cd005-auth-oauth-predictable-state` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:298-304,307-317` — the incomplete API issues no state and rejects completion until browser binding and provider proof exist; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:2858-2869` routes both calls to that Prelude seam. |
| `cd005-auth-oauth-unverified-subject` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:307-317` — a caller-supplied subject cannot mint a session because OAuth completion fails closed without verified provider proof; `crates/jet-comptime/src/Comptime/AuthLite.rs:185-191` includes the same Prelude implementation. |
| `notebook-zero-token-fallback` | `already-fixed` | `Source/CmdNotebook.rs:58-69,133-139` — token creation uses `read_exact` and entropy failure exits with an error; no all-zero fallback reaches `serve_loopback`. |
| `signing-key-permission-fail-open` | `already-fixed` | `Source/Publish/Sign.rs:111-117,401-425` — private seeds use `create_new(true).mode(0600)`, write and sync before publication, and permission errors abort; force replacement uses a separate temporary path. |
| `auth-session-show-token-leak` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs:149-155` — `session_show` emits `<redacted>` instead of the live bearer ID; `tests/taint.rs:553-567` asserts the hostile value is absent. |
| `archive-key-predictable-fallback` | `already-fixed` | `crates/jetpack/src/Store/Archive.rs:1388-1407,1442-1446` — automatic archive-key creation calls bounded OS CSPRNG entropy and fails on error; `crates/jetpack/src/TrustRoot.rs:47-65` has no predictable fallback. |
| `claude-config-credential-exfil` | `already-fixed` | `.claude/settings.local.json:2-8` — the tracked allow list no longer grants broad `~/.config` reads or unrestricted `gh api`; the repository config check is the regression proof, not a cargo test. |
| `comptime-aes-table-timing-side-channel` | `already-fixed` | `crates/jet-comptime/src/Comptime/CryptoLite/Aes256Gcm.rs:8-16,20-40,145-160` — AES S-box and GHASH use fixed-round arithmetic and masks, with no secret-indexed table or branch; `crates/jet-comptime/tests/crypto_lite.rs:24-40` checks the vector and source invariant. |
| `comptime-argon2id-nonstandard` | `already-fixed` | `crates/jet-comptime/src/Comptime/CryptoLite/Argon2id.rs:314-410` — standard address-block generation, reference-area mapping, and final-lane XOR are implemented; `crates/jet-comptime/tests/crypto_lite.rs:42-56` matches the canonical expert known answer and rejects the simplified generator. |
| `managed-secret-temp-permission-window` | `already-fixed` | `crates/jetpack/src/EnvFiles.rs:323-333,484-504,590-615` — sensitive temporary files receive restrictive mode at `create_new` before any write and are checked again before publication; `crates/jetpack/src/EnvFiles.rs:911-923` asserts the pre-write mode. |

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
| `s2-fenced-range-expansion-dos` | `already-fixed` | `crates/jet-parser/src/FencedNames.rs:12,313-331,486-497` rejects integer and named ranges above `MAX_FENCE_EXPANSION` before collection; `hostile_fence_ranges_are_rejected_before_expansion` at `:752-768` covers both forms. |
| `s2-runtime-json-depth-dos` | `already-fixed` | `crates/jet-foundation/src/EncodingJson.rs:3,87-98,250-292` applies `MAX_JSON_DEPTH` in the shared recursive parser; `crates/jet-codegen/src/Prelude/CoreLib/JetStd/JSONCodec.rs:1-5` uses that parser. |
| `s2-comptime-json-depth-dos` | `already-fixed` | `crates/jet-comptime/src/Comptime/JSONInterp.rs:147-160` delegates both comptime JSON paths to `EncodingJson`; `crates/jet-foundation/src/EncodingJson.rs:250-292` rejects excessive nesting. |
| `s2-comptime-toml-depth-dos` | `already-fixed` | `crates/jet-comptime/src/Comptime/EncodingLite.rs:628-636,830-845,877-894` checks `MAX_TOML_DEPTH` and passes depth through array and inline-table recursion. |
| `s2-comptime-yaml-depth-dos` | `already-fixed` | `crates/jet-comptime/src/Comptime/EncodingLite.rs:1104-1148,1276-1309,1327-1359` checks `MAX_YAML_DEPTH` in block and flow recursion. |
| `runtime-toml-depth-dos` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/JetStd/TOML.rs:10,374-382,578-592,621-638` checks `MAX_TOML_DEPTH` and carries depth through nested values. |
| `runtime-yaml-depth-dos` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/JetStd/YAML.rs:10,82-105,109-139,235-273,288-323` checks `MAX_YAML_DEPTH` in block and flow parsing. |
| `gzip-runtime-unbounded-output` | `already-fixed` | `crates/jet-pkg-model/src/Prelude/Compress.rs:31-44` limits gzip reads to 64 MiB plus one sentinel byte and rejects overflow. |
| `zstd-runtime-unbounded-output` | `already-fixed` | `crates/jet-pkg-model/src/Prelude/Compress.rs:53-68` limits zstd reads to 64 MiB plus one sentinel byte and rejects overflow. |
| `processspec-output-limit-late` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs:422-465,1981-2068,2133-2143` drains output under a shared budget, kills on overflow, and returns `ResourceLimit`; `crates/jet-jit/src/Process.rs:315-344,367-369` calls the Prelude. |
| `build-action-unbounded-output` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/ProcessSandbox.rs:16-18,343-371,391-464` caps captured stdout/stderr and kills timed-out children; `crates/jet-comptime/src/Comptime/Build/execution_runtime.rs:32-34,259-321,870-885` applies the 30-second deadline to local BuildActions; `:2633-2690` covers the shared timeout path. |
| `notebook-preauth-slowloris` | `already-fixed` | `Source/CmdNotebook.rs:186-204` sets a 10-second read timeout before request parsing; `Source/CmdNotebook.rs:444-459` also caps headers at 64 KiB. |
| `plugin-call-unbounded-resources` | `already-fixed` | `crates/jet-pkg-model/src/Prelude/Plugin.rs:33-74,95-105` configures fuel, epoch interruption, linear-memory, table, instance, and memory limits; `crates/jet-pkg-model/src/Prelude/Plugin.rs:147-170` arms the two-second call timer and argument budget. |
| `d0017-s1-tar-pax-allocation` | `already-fixed` | `crates/jetpack/src/Provider/fetch.rs:382-399` rejects raw `stored` payload size before `Vec::with_capacity`; `pax_logical_size_cannot_bypass_stored_payload_limit` at `:675-693` proves a small PAX logical size cannot bypass it. |
| `d0017-s3-studio-slowloris` | `already-fixed` | `crates/jetpack/src/CLI/studio_server.rs:25,111-115,130-133` configures ten-second read and write deadlines before the serial request reader; `:488-516` verifies the deadlines and hostile partial request timeout. |
| `compiler-extension-json-depth-dos` | `already-fixed` | `crates/jet-pkg-model/src/CompilerExtension.rs:484-488` uses `JSON::parse`; `crates/jet-foundation/src/JSON.rs:43-49,80-85,159-199` bounds input and recursive JSON depth. |
| `nix-json-depth-dos` | `already-fixed` | `crates/jet-nix-eval/src/JSON.rs:6,42-53,80-83,115-138` passes depth through JSON recursion and rejects values beyond the evaluator limit. |
| `process-pipeline-limits-ignored` | `already-fixed` | `crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs:1645-1777` starts per-stage output drains and enforces timeout/output limits; `crates/jet-jit/src/Process.rs:325-344` calls that Prelude pipeline. |
| `envhook-pretrust-symlink-recursion-dos` | `already-fixed` | `crates/jetpack/src/EnvHook.rs:279-317,363-392` rejects external links, records cycles with a visited set, and does not recurse through directory symlink entries; `crates/jetpack/src/CLI/run_enter_dev.rs:2814-2820` uses this bounded fingerprint before activation. |
| `nix-expression-parser-depth-dos` | `already-fixed` | `crates/jet-nix-eval/src/Evaluator.rs:627-670` tracks parser depth and returns `ResourceLimit` before nested expression parsing exceeds `MAX_EVAL_DEPTH`. |
| `package-treehash-symlink-recursion` | `already-fixed` | `crates/jet-foundation/src/SHA256.rs:219-226` uses `symlink_metadata` and skips symlink nodes before recursion; `tree_hash_skips_recursive_symlink_nodes` at `:285-303` proves a recursive link does not alter hashing or recurse indefinitely. |
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
| `comptime-fetch-ssrf` | Hash-pinned compile-time fetch permits arbitrary outbound requests before verification | crates/jet-comptime/src/Comptime/Methods/dispatch.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch/eval_method.rs | already-fixed | `crates/jet-net/src/lib.rs:190-305` — compile-time HTTP uses a redirect-free agent whose resolver rejects every non-public DNS answer, and local reads are root-scoped; `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:776-790` passes the compile-time root before hash verification; `crates/jet-net/src/lib.rs:507-538` rejects loopback and outside-root fixtures. | 11 |
| `cd005-comptime-fetch-local-disclosure` | Hermetic compile-time fetch can disclose arbitrary local text files | crates/jet-net/src/lib.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch.rs | already-fixed | `crates/jet-net/src/lib.rs:215-239` canonicalizes the source root and requested file and requires containment before opening it; `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:776-790` routes the caller through that check; `crates/jet-net/src/lib.rs:507-533` rejects absolute and symlink escapes. | 9 |
| `provider-registry-private-network-ssrf` | Project provider policy can authorize private-network HTTPS fetches | crates/jetpack/src/Provider/fetch.rs<br>crates/jetpack/src/Provider/script_registry.rs | already-fixed | `crates/jetpack/src/Provider/fetch.rs:145-217,530-587` rechecks the allowlist on every redirect, requires HTTPS, resolves every address as public, pins curl with `--resolve`, disables redirects and ambient proxy/configuration; `crates/jetpack/src/Provider/fetch.rs:686-705` covers private, credentialed, and denied destinations. | 5 |
| `jit-http-crlf-injection` | JIT generic HTTP request serialization permits CRLF request injection | crates/jet-jit/src/net_http_hosts.rs<br>crates/jet-pkg-model/src/Prelude/HTTP.rs | already-fixed | `crates/jet-pkg-model/src/Prelude/HTTP.rs:1577-1644,3064-3074` rejects control bytes in URL targets, methods, header names, and values before serialization; `crates/jet-jit/src/net_http_hosts.rs:2440-2533` marshals JIT requests to the shared Prelude sender; `tests/http_client_law.rs:1148-1219` and `tests/http_i9.rs:182-200` are hostile framing/tier witnesses. | 4 |
| `jit-websocket-handshake-crlf-injection` | WebSocket URL permits HTTP handshake CRLF injection | crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs | already-fixed | `crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs:247-291,581-603` rejects control and whitespace bytes before interpolating URL parts into the handshake; `tests/ws_law.rs:661-665` and `tests/http_i9.rs:182-200` reject hostile URLs before a socket write on the Prelude/dev tiers. | 1 |
| `git-dependency-transport-ssrf` | Git dependency fetch allows attacker-selected network destinations |  | already-fixed | `Source/Fetch.rs:841-851,2407-2416,2447-2503,2662-2805` restricts Git to approved schemes, rejects private DNS answers and embedded credentials, confines local `file://` repositories to real directories below the project root, and runs Git without system/global config, prompts, credential helpers, or SSH identities; `tests/pkg.rs:3738-3868` covers private, unscoped-local, option-shaped, and cache-path hostile inputs. | 1 |

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
| `package-store-install-symlink-escape` | already-fixed | `Source/Store.rs:44-71,235-312,339-385` validates real source/store roots, rejects symlinks and special files during recursive copy/link, and checks destination containment; `tests/pkg.rs:2889-2915` is the hostile source/destination regression. |
| `canvas-create-package-symlink-write` | already-fixed | `crates/jet-devserver/src/Canvas/project_transactions.rs:1118-1178` canonicalizes the project root and every existing component and rejects symlink traversal; `crates/jet-devserver/src/Canvas/source_model.rs:57-121` rejects symlink sources and publishes through exclusive temporary files; `tests/canvas.rs:5044-5074` covers package-path escape. |
| `dependency-name-path-traversal` | already-fixed | `crates/jet-pkg-model/src/Package/Blocks.rs:432-455` validates dependency names as one safe component before retaining them; `crates/jet-pkg-model/src/Package/Convert.rs:19-95` repeats the boundary at conversion; `tests/pkg.rs:1857-1880` rejects traversal names. |
| `devserver-static-symlink-escape` | already-fixed | `crates/jet-devserver/src/lib.rs:215-263` rejects rooted/traversal paths, walks existing components with `symlink_metadata`, and canonicalizes under the real root before serving; `tests/web_dev.rs:620-650` covers a symlink escape. |
| `d0002-s2-sparse-copy-symlink` | already-fixed | `crates/jetpack/src/Provider/remote.rs:286-296,648-675` — the sparse-copy fallback checks `symlink_metadata` and rejects symlink or non-file source entries before copying. |
| `s0-web-test-prefix-traversal` | already-fixed | `scripts/web-test/serve.mjs:17-73` resolves the real root, uses component-boundary `relative` checks, walks real ancestors, and rejects outside or final symlinks before `createReadStream`; `tests/web_dev.rs:620-650` covers the hostile server path. |
| `vendor-symlink-escape` | already-fixed | `Source/Publish/Vendor.rs:39-172,187-225` validates the source root, rejects source/destination symlinks and non-regular entries, and refuses symlink replacement; `tests/pkg.rs:5303-5339` covers source symlink and traversal-name inputs. |
| `cd005-comptime-embed-file-symlink` | already-fixed | `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:496-584` canonicalizes and contains embed paths before file reads; `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:1679-1702` rejects symlink escapes. |
| `cd005-comptime-embed-bytes-symlink` | already-fixed | `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:496-584` is the shared checked path for both text and bytes embeds; `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:1679-1702` exercises the symlink escape. |
| `cd005-build-embed-symlink` | already-fixed | `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:496-584` routes `b.embed` through the same canonical real-root check; `crates/jet-comptime/src/Comptime/Methods/dispatch.rs:1679-1702` rejects the hostile link. |
| `git-revision-cache-path-traversal` | already-fixed | `Source/Fetch.rs:847-851,2411-2412,2451-2463,2611-2659` requires a single safe revision component and validates every existing cache ancestor before creation, cleanup, or clone; `tests/pkg.rs:3833-3868` rejects traversal and option-shaped revisions. |
| `jetpack-dotenv-symlink-read` | already-fixed | `crates/jet-env-model/src/ModuleEval/Environment.rs:2450-2471` accepts only normal relative paths and canonicalizes them below a real project root; `crates/jet-env-model/src/ModuleEval/Environment.rs:3629-3650` covers a symlink escape. |
| `jetpack-image-files-read-traversal` | already-fixed | `crates/jetpack/src/CLI/add_remove_push_image.rs:1121-1188` — path components are normalized, symlinks are rejected, the target is canonicalized, and canonical containment is enforced before the bounded read. |
| `jetpack-image-layer-path-traversal` | already-fixed | `crates/jetpack/src/Image.rs:364-376,1362-1400` — every layer path is checked for absolute, parent, prefix, control-byte, length, duplicate, and collision violations before tar emission. |
| `canvas-action-temp-symlink-overwrite` | already-fixed | `crates/jet-devserver/src/Canvas/edit_actions.rs:1321-1341` creates the predictable check file exclusively with no-follow flags; `crates/jet-devserver/src/Canvas/edit_actions.rs:4231-4251` verifies a pre-existing symlink is not followed. |
| `jetpack-overlay-patch-path-traversal` | already-fixed | `crates/jetpack/src/Overlay.rs:21-44,67-124,268-309` — patch and target paths reject symlink components and require canonical containment before read or staged write. |
| `trust-prefix-sibling-overmatch` | already-fixed | `crates/jetpack/src/Trust.rs:616-658` requires an exact match or a `/`/`\` component boundary for both raw and canonical subjects; `crates/jetpack/src/Trust.rs:1428-1435` covers sibling-name regression. |
| `devserver-build-symlink-overwrite` | already-fixed | `Source/CmdCompile.rs:6215-6257`, `crates/jet-devserver/src/WebHost.rs:1077-1124`, and `crates/jet-codegen/src/Prelude/DevServer.rs:612-666` validate real output roots, reject symlink ancestors, and publish only contained paths; `tests/web_build.rs` and `tests/web_dev.rs` cover hostile outputs. |
| `lsp-predictable-log-symlink-write` | already-fixed | `Source/LSP/Server.rs:291-318` rejects a symlink and opens the fixed log with `O_NOFOLLOW`; `Source/LSP/Server.rs:4006-4024` covers the prepositioned link. |
| `canvas-source-symlink-read` | already-fixed | `crates/jet-devserver/src/Canvas/project_scan.rs:453-525`, `crates/jet-devserver/src/Canvas/schema_api.rs:287-310`, and `crates/jet-pkg-model/src/Authority.rs:776-800,1035-1063` route discovered files through descriptor-relative no-follow checks and reject symlink entries. |
| `jetpack-remote-symlink-fingerprint-escape` | already-fixed | `crates/jetpack/src/Provider/remote.rs:682-754` requires a real root, canonical containment, regular files, and no symlink entries for both fingerprint and copy; `crates/jetpack/src/Provider/remote.rs:647-680` is the hostile regression. |
| `repl-run-temp-symlink-overwrite` | already-fixed | `crates/jet-repl/src/lib.rs:1241-1263` creates the predictable temporary source exclusively with no-follow flags and removes only a file it created; `crates/jet-repl/src/lib.rs:4001-4020` covers the symlink collision. |

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
| `devserver-cross-origin-mutation` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:1445-1500` validates method, body framing, Host, Origin, and session; `1720-1750` applies that gate before Canvas routes. The public control paths at `2196-2274` only expose live-reload/status telemetry and do not dispatch Canvas mutations. |
| `s1-root-studio-remote-read` | `already-fixed` | `crates/jetpack/src/CLI/bridge_os_studio.rs:124-143` supports explicit or loopback binding; `crates/jetpack/src/CLI/studio_server.rs:147-155,166-195,259-334` requires the session/Host/Origin gate before projection or config reads. |
| `s1-root-studio-remote-write` | `already-fixed` | `crates/jetpack/src/CLI/studio_server.rs:147-164` gates the transaction route; `crates/jetpack/src/CLI/studio_transactions.rs:85-137,323-443` requires a server-issued transaction session and validates the staged write before applying it. |
| `s1-root-studio-remote-run` | `already-fixed` | `crates/jetpack/src/CLI/studio_server.rs:147-164` gates the run route; `crates/jetpack/src/CLI/studio_transactions.rs:631-695,1107-1147` performs the requested JetOS action only after that gate. |
| `s1-root-studio-loopback-csrf` | `already-fixed` | `crates/jetpack/src/CLI/bridge_os_studio.rs:133-143` defaults to loopback; `crates/jetpack/src/CLI/studio_server.rs:259-334` requires Host, same-origin/Origin, and a valid capability for every request, with POST-specific checks at `279-289`. |
| `s2-devserver-source-disclosure` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:1445-1500` requires Canvas Host, Origin, and session checks; `1720-1750` runs them before the graph/source routes at `1895-1935,2026-2043`. Public status at `2196-2211` now returns only `DevStatus::json()`; `tests/web_dev.rs:1642-1664` proves an authorized graph cannot make retained source appear in an unauthenticated status read. |
| `s2-devserver-debug-trigger` | `already-fixed` | `crates/jet-devserver/src/WebHost.rs:1445-1500` requires Canvas Host, Origin, and session checks; `1720-1750` runs them before the debug route at `2157-2182`. |
| `devserver-windows-absolute-static-path` | `already-fixed` | `crates/jet-devserver/src/lib.rs:215-263` rejects rooted, drive-prefixed, traversal, and symlink paths; `crates/jet-devserver/src/WebHost.rs:2416-2455` serves only the guarded result. |
| `canvas-project-revision-not-enforced` | `already-fixed` | `crates/jet-devserver/src/Canvas/schema_api.rs:405-423` requires `project_revision` and compares it with the current project revision before dispatch; project queries also pass the expected revision at `500-565`. |
| `embedded-devserver-windows-absolute-static-path` | `already-fixed` | `crates/jet-codegen/src/Prelude/DevServer.rs:568-610` serves only the guarded build path; `612-666` rejects rooted, drive-prefixed, traversal, and symlink paths. |

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
| `s0-bash-prompt-label-injection` | `already-fixed` | `crates/jetpack/src/Shell.rs:802-805,1000-1002` keeps the label in `JETPACK_PROMPT_LABEL` and renders it through quoted `printf`; `crates/jetpack/src/Shell.rs:1426-1495` proves hostile Bash labels do not execute. |
| `s0-zsh-prompt-label-injection` | `already-fixed` | `crates/jetpack/src/Shell.rs:802-805,1040-1042` keeps the label in the environment and renders it through `print -r --`; `crates/jetpack/src/Shell.rs:1460-1526` proves hostile Zsh labels do not execute. |
| `s0-fish-prompt-label-injection` | `already-fixed` | `crates/jetpack/src/Shell.rs:802-805,1073-1077` keeps the label in the environment and renders it as a quoted `printf` argument; `crates/jetpack/src/Shell.rs:1528-1553` proves hostile Fish labels do not execute. |
| `package-git-fetch-option-injection` | `already-fixed` | `crates/jetpack/src/Provider/remote.rs:418-487` preserves `#rev` but rejects control characters and leading `-`; `crates/jetpack/src/Provider/remote.rs:206-221` rechecks before `git fetch`. |
| `package-git-checkout-option-injection` | `already-fixed` | `crates/jetpack/src/Provider/remote.rs:418-487` validates the parsed revision; `crates/jetpack/src/Provider/remote.rs:543-554` rechecks it immediately before the `git checkout` argument. |
| `lldb-breakpoint-command-injection` | `already-fixed` | `crates/jet-debug/src/Inferior.rs:521-532,1214-1225` quotes breakpoint paths and rejects controls; `crates/jet-debug/src/Inferior.rs:1870-1875` proves the hostile path case. |
| `web-codegen-template-injection` | `already-fixed` | `crates/jet-codegen/src/Codegen/Web.rs:2668-2686` escapes backslashes, backticks, `${`, controls, and line separators; `crates/jet-codegen/src/Codegen/Web.rs:10219-10241` applies it to literal template parts, with hostile coverage at `11167-11172`. |
| `rustc-build-profile-env-injection` | `already-fixed` | `crates/jet-pkg-model/src/Package/Blocks.rs:778-807` rejects retired profile `env`; `Source/main.rs:271-326,358-408` derives only typed profile flags, and `Source/CmdCompile.rs:6494-6526` launches rustc without profile environment injection. |
| `jetpack-self-authorized-build-script` | `already-fixed` | `crates/jetpack/src/CLI/realize.rs:597-611` gates Core Cargo before Store realization; `crates/jetpack/src/Trust.rs:931-960` requires an exact build identity grant or explicit approval, while `crates/jetpack/src/Provider.rs:258-272` derives that identity only from declared dependencies and source authorities, not project metadata. |
| `package-git-kind-probe-option-injection` | `already-fixed` | `crates/jetpack/src/Provider.rs:2220-2237,2289-2304` rejects an unsafe probe revision before `git fetch`; `crates/jetpack/src/Provider/remote.rs:418-487` validates the parsed remote first. |
| `jetos-storage-disk-command-injection` | `already-fixed` | `crates/jetpack/src/JetOS/module_storage_workload.rs:113-117` shell-quotes the generated default and the runtime script at `128-137` allowlists disk syntax; root filesystem types are allowlisted at `64-68,140-142`, with generated-script coverage at `160-187`; `tests/jetpack_jetos.rs:1439-1450` proves a hostile disk cannot create a marker. |
| `jetos-storage-esp-command-injection` | `already-fixed` | `crates/jetpack/src/JetOS/module_storage_workload.rs:61-63,128-137` accepts only numeric storage sizes before script interpolation; `144-149` rejects shell-source payloads. |
| `envhook-profile-var-name-shell-injection` | `already-fixed` | `crates/jet-env-model/src/ModuleEval/Environment.rs:2442-2446,2610-2642` validates profile variable names; `crates/jetpack/src/EnvHook.rs:471-480,582-590` validates again at render. |
| `envhook-unset-name-shell-injection` | `already-fixed` | `crates/jet-env-model/src/ModuleEval/Environment.rs:2300-2302,2598-2607` validates unset names; `crates/jetpack/src/EnvHook.rs:582-590,783-788` enforces and tests the render boundary. |
| `claude-hook-relative-path-rce` | `already-fixed` | `.claude/settings.json:30,34,45` requires `CLAUDE_PROJECT_DIR` and resolves each script as `$CLAUDE_PROJECT_DIR/scripts/agent/...`; a hostile cwd no longer controls the executed path. |
| `vscode-workspace-lsp-rce` | `already-fixed` | `editors/vscode/extension.js:25-50,114-116,151-159` still selects the workspace/server command for trusted workspaces; `editors/vscode/package.json:21-24` disables the extension in untrusted workspaces before activation. |
| `zed-worktree-lsp-rce` | `already-fixed` | `editors/zed/wasm-src/src/lib.rs:12-31` resolves `jet` only with `worktree.which` and errors when PATH lacks it; `editors/zed/extension.toml:15-18` grants only the fixed `jet self lsp` command. |
| `perl-bind-compile-exec` | `already-fixed` | `Source/CmdDevTools.rs:3751-3817` invokes `PerlBind::bind`; `crates/jet-pkg-model/src/PerlBind.rs:57-68,131-160` parses source without `perl -c`, with hostile `BEGIN` coverage at `tests/cli_parts/bindings.rs:1318-1343`. |

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
| `locked-dependency-integrity-bypass` | `already-fixed` | `Source/Fetch.rs:1956-1990,1992-2029` matches lock identity and calls `Store::verify_entry` for both path and Git sources before returning; `tests/pkg.rs:3442-3510,3600-3695` reject tampered locked sources. |
| `package-store-incomplete-content-hash` | `already-fixed` | `crates/jet-foundation/src/SHA256.rs:186-238` hashes every copied regular non-hidden file, not only `.jet`; `tests/pkg.rs:3011-3038` proves tampering `runtime.data` returns E1204. |
| `git-revision-utf8-slice-panic` | `already-fixed` | `Source/Fetch.rs:2385-2403` builds the cache prefix with `char_indices`, not a byte-invalid slice; `tests/pkg.rs:3700-3734` proves a multibyte revision returns an error without panic. |
| `jetpack-typed-environment-trust-bypass` | `already-fixed` | `crates/jetpack/src/Trust.rs:551-564,1023-1032` requires an exact environment/build grant for typed environments; `tests/jetpack_engine.rs:4147-4181` proves a service-only environment gets E1255 and does not run its command. |
| `buildrecipe-exec-unsandboxed` | `already-fixed` | `crates/jetpack/src/Recipe.rs:968-992,1101-1124,1577-1595,1621-1670` routes recipe execution through the native sandbox and maps unavailable enforcement to an error; `crates/jet-comptime/src/Comptime/Build/execution_runtime.rs:315-317` has no unsandboxed fallback. |
| `buildrecipe-logged-unsandboxed` | `already-fixed` | `crates/jetpack/src/Provider/adapter.rs:57-72` reaches `Recipe::run_logged`; `crates/jetpack/src/Recipe.rs:1027-1064,1599-1670` sends logged execution through the same native sandbox. |
| `transitive-path-dependency-escape` | `already-fixed` | `Source/Fetch.rs:730-750` enforces lexical and canonical containment below the declaring dependency; `tests/pkg.rs:3198-3280,3415-3438` cover traversal, symlink, and compiler paths. |

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
| `secrets-key-permission-window` | `already-fixed` | `crates/jetpack/src/Secrets.rs:249-264` applies `OpenOptionsExt::mode(0600)` before creating the temporary identity; `crates/jetpack/src/Secrets.rs:724-742` and `tests/secrets.rs:80-96` assert the restricted mode. |
| `jetpack-project-trust-self-allow` | `already-fixed` | `crates/jetpack/src/Trust.rs:1034-1057` treats project `Allow` as insufficient external approval and continues to the terminal/prompt gate; `crates/jetpack/src/Trust.rs:1363-1399` proves both allow and deny are rejected non-interactively. |

## Source artifacts

The similarly named [full discovery artifact](security-deep-scan-2026-08-03-full.md) currently
belongs to the separate #1378 memory/ABI lane; the current-disposition tables above are the
source-backed evidence for #1384 and #1385.

That separate full artifact preserves source, control, sink, impact, evidence,
preconditions, uncertainty, CWE data, validation guidance, and source-ledger paths
for #1378.

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
