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

## tower-control-plane

### Tower authorization, CSRF, and document containment

11 candidates. Priority P0. Milestone `e12-security-boundaries`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `tower-default-network-auth-bypass` | Tower's default network authentication bypass exposes read and mutation APIs | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 116 |
| `tower-docs-symlink-read` | Tower document reads follow symlinks outside the docs root | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 31 |
| `tower-owner-authorization-bypass` | Tower grants owner-acceptance authority without establishing owner identity | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 29 |
| `tower-loopback-csrf` | Tower loopback mutation APIs lack browser-origin and CSRF controls | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 20 |
| `tower-owner-payload-forgery` | Tower trusts caller-supplied owner attribution for privileged mutations | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 18 |
| `tower-docs-symlink-write` | Tower docs API writes through symlinked directories outside the repository | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 17 |
| `tower-docs-symlink-delete` | Tower docs API deletes or moves files through symlinked directories outside the repository | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 14 |
| `tower-docs-symlink-walk` | Tower docs inventory recursively traverses symlinked directories | plugins/tower/app/docs.mjs<br>plugins/tower/app/server.mjs | 4 |
| `cd005-tower-token-dns-rebind` | Tower token authentication is bypassed for DNS-rebound loopback requests | plugins/tower/app/server.mjs | 2 |
| `tower-tracked-state-priority-xss` | Tracked Tower card priority reaches innerHTML without validation or escaping | plugins/tower/app/store.mjs<br>plugins/tower/app/ui/tower.js | 2 |
| `tower-ratified-decision-integrity-bypass` | Generic API callers can reopen or delete ratified owner decisions without an owner check | plugins/tower/app/server.mjs<br>plugins/tower/app/store.mjs | 1 |

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

## network-boundaries

### Network egress, SSRF, HTTP framing, and local disclosure

6 candidates. Priority P1. Milestone `e12-security-data`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `comptime-fetch-ssrf` | Hash-pinned compile-time fetch permits arbitrary outbound requests before verification | crates/jet-comptime/src/Comptime/Methods/dispatch.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch/eval_method.rs | 11 |
| `cd005-comptime-fetch-local-disclosure` | Hermetic compile-time fetch can disclose arbitrary local text files | crates/jet-net/src/lib.rs<br>crates/jet-comptime/src/Comptime/Methods/dispatch.rs | 9 |
| `provider-registry-private-network-ssrf` | Project provider policy can authorize private-network HTTPS fetches | crates/jetpack/src/Provider/fetch.rs<br>crates/jetpack/src/Provider/script_registry.rs | 5 |
| `jit-http-crlf-injection` | JIT generic HTTP request serialization permits CRLF request injection | crates/jet-jit/src/net_http_hosts.rs<br>crates/jet-pkg-model/src/Prelude/HTTP.rs | 4 |
| `jit-websocket-handshake-crlf-injection` | WebSocket URL permits HTTP handshake CRLF injection | crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs | 1 |
| `git-dependency-transport-ssrf` | Git dependency fetch allows attacker-selected network destinations |  | 1 |

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

## policy-integrity

### Trust policy, sandbox claims, concurrency, and remaining integrity gaps

2 candidates. Priority P1. Milestone `e12-security-validation`.

| Candidate ID | Discovery title | Primary locations | Source reports |
|---|---|---|---:|
| `secrets-key-permission-window` | Age identity is created with ambient permissions before best-effort chmod | crates/jetpack/src/Secrets.rs | 2 |
| `jetpack-project-trust-self-allow` | Untrusted projects can self-authorize the trust gate that precedes environment hooks | crates/jetpack/src/Trust.rs<br>crates/jetpack/src/CLI/run_enter_dev.rs | 1 |

## Source artifacts

The repository includes the [full discovery evidence](security-deep-scan-2026-08-03-full.md).

The full report preserves source, control, sink, impact, evidence, preconditions,
uncertainty, CWE data, validation guidance, and source-ledger paths.

This summary is the durable candidate inventory and Tower campaign map.
