# Jet security closure report

Report date: 2026-09-02
Card: #1387
Report status: reconciliation complete; fresh external scan pending

## Executive summary

This report reconciles the 134 candidate IDs from the canceled 2026-08-03 discovery report against its current-tree disposition tables.
The reconciliation has 133 `already-fixed` candidates and one `duplicate-of-d0017-s1-aot-termios-layout` candidate.
No candidate has a `confirmed` disposition, and no candidate remains unresolved in the historical campaign inventory.
The fresh full-repository Codex Security scan is not recorded as complete here. The installed plugin path and the exact external gate are recorded below.

## Criterion evidence

<!-- security-criteria:v1 -->
| criterion | status | evidence path |
| --- | --- | --- |
| 1 | met | `docs/audits/security-deep-scan-2026-08-03.md:107-621` |
| 2 | met | `docs/audits/security-deep-scan-2026-08-03.md:156-186` |
| 3 | pending-external-gate | `docs/agents/security-closure.md:197-265` |
| 4 | pending-external-gate | `docs/agents/security-closure.md:230-281` |
| 5 | met (recorded) | `docs/audits/security-deep-scan-2026-08-03.md:105-105` |
<!-- /security-criteria -->

Criterion 1 is measured by the machine validator and the single canonical reconciliation source.
Criterion 2 is the independent evidence recorded by the ten campaign cards; #1386 remains represented as the two-candidate scope absorbed by #1385.
Criterion 3 remains open until the external Codex Security completion and finalization gate produces a completed zero-finding bundle.
Criterion 4 remains pending because the external canonical scan bundle is not present in this checkout; this report is the dated reconciliation record.
Criterion 5 is recorded from the existing Tower closeout evidence and was not rerun by this report-generation pass.

## Candidate dispositions

Each evidence path points to the final-disposition row in the canonical reconciliation source. That row carries the source file:line trace and hostile-proof citation for the candidate.

<!-- security-dispositions:v1 -->
| id | candidate | disposition | evidence path |
| --- | --- | --- | --- |
| `tower-default-network-auth-bypass` | Tower's default network authentication bypass exposes read and mutation APIs | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:109` |
| `tower-docs-symlink-read` | Tower document reads follow symlinks outside the docs root | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:110` |
| `tower-owner-authorization-bypass` | Tower grants owner-acceptance authority without establishing owner identity | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:111` |
| `tower-loopback-csrf` | Tower loopback mutation APIs lack browser-origin and CSRF controls | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:112` |
| `tower-owner-payload-forgery` | Tower trusts caller-supplied owner attribution for privileged mutations | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:113` |
| `tower-docs-symlink-write` | Tower docs API writes through symlinked directories outside the repository | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:114` |
| `tower-docs-symlink-delete` | Tower docs API deletes or moves files through symlinked directories outside the repository | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:115` |
| `tower-docs-symlink-walk` | Tower docs inventory recursively traverses symlinked directories | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:116` |
| `cd005-tower-token-dns-rebind` | Tower token authentication is bypassed for DNS-rebound loopback requests | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:117` |
| `tower-tracked-state-priority-xss` | Tracked Tower card priority reaches innerHTML without validation or escaping | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:118` |
| `tower-ratified-decision-integrity-bypass` | Generic API callers can reopen or delete ratified owner decisions without an owner check | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:119` |
| `jit-jetarena-vec-layout-casts` | JIT list mutations cast `JetArena` to `Vec` without a guaranteed layout | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:146` |
| `d0017-s1-aot-termios-layout` | AOT terminal control uses a non-portable termios FFI layout | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:151` |
| `d0017-s1-jit-termios-layout` | JIT secret-input path uses a non-portable termios FFI layout | `duplicate-of-d0017-s1-aot-termios-layout` | `docs/audits/security-deep-scan-2026-08-03.md:152` |
| `wasm-list-i64-untrusted-ownership` | Generated WebAssembly integer-list ABI reconstructs and frees unchecked host pointers | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:147` |
| `wasm-list-string-untrusted-ownership` | Generated WebAssembly string-list ABI trusts host pointer ownership and embedded counts | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:148` |
| `wasm-map-untrusted-ownership` | Generated WebAssembly string-map ABI reconstructs and frees unchecked host pointers | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:149` |
| `wasm-string-untrusted-ownership` | Generated WebAssembly string ABI reconstructs and frees unchecked host pointers | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:150` |
| `jit-http-worker-runtime-uaf` | Resident JIT teardown can leave HTTP workers with stale runtime and code pointers | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:153` |
| `jit-event-four-capture-abi` | Four-capture JIT event callbacks are invoked with the wrong ABI | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:154` |
| `cd005-auth-predictable-session-id` | Auth runtime issues predictable session identifiers | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:215` |
| `cd005-auth-predictable-magic-token` | Auth runtime issues predictable magic-login tokens | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:216` |
| `cd005-auth-oauth-predictable-state` | OAuth state values are predictable and not bound to a browser session | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:217` |
| `cd005-auth-oauth-unverified-subject` | OAuth completion trusts a caller-supplied subject without provider proof | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:218` |
| `notebook-zero-token-fallback` | Notebook bearer-token generation silently falls back to an all-zero token | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:219` |
| `signing-key-permission-fail-open` | Signing-key generation writes the seed before fail-open permission tightening | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:220` |
| `auth-session-show-token-leak` | Session.show includes the live bearer session identifier | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:221` |
| `archive-key-predictable-fallback` | Archive signing-key fallback is predictable | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:222` |
| `claude-config-credential-exfil` | Tracked Claude permissions combine broad credential reads with unrestricted GitHub API calls | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:223` |
| `comptime-aes-table-timing-side-channel` | Interpreter AES-256-GCM uses secret-indexed lookup tables and secret-dependent GHASH branches | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:224` |
| `comptime-argon2id-nonstandard` | Comptime expert.argon2id uses a simplified, likely incompatible KDF | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:225` |
| `managed-secret-temp-permission-window` | Sensitive managed-file bytes are written before restrictive permissions | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:226` |
| `tar-unbounded-materialization` | TAR APIs materialize every entry without a size limit | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:287` |
| `zip-runtime-unbounded-output` | Runtime `core.archive` ZIP decompression reads expanded output without a bound | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:286` |
| `s1-root-net-http-unbounded` | Compile-time HTTP fetch buffers an unbounded response before digest verification (root) | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:292` |
| `zip-resident-unbounded-output` | Resident ZIP inflater accepts attacker-declared output without the codec limit | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:284` |
| `s0-devserver-unbounded-header-line` | Devserver accepts unbounded HTTP request and header lines | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:288` |
| `s0-jit-http-simple-unbounded-response` | Simple JIT HTTP client buffers responses without a total cap | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:289` |
| `s0-jit-http-request-unbounded-response` | Configurable JIT HTTP request host buffers responses without a cap | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:290` |
| `s2-runtime-json-depth-dos` | Runtime JSON parser has unbounded recursive nesting | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:294` |
| `gzip-runtime-unbounded-output` | Gzip decompression has no output budget | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:300` |
| `zstd-runtime-unbounded-output` | Zstandard decompression has no output budget | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:301` |
| `s2-comptime-json-depth-dos` | Comptime JSON parsing can overflow the compiler stack | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:295` |
| `runtime-yaml-depth-dos` | Runtime YAML parser has unbounded recursive nesting | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:299` |
| `s2-comptime-toml-depth-dos` | Comptime TOML parsing has unbounded recursive nesting | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:296` |
| `s2-comptime-yaml-depth-dos` | Comptime YAML parsing has unbounded recursive nesting | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:297` |
| `processspec-output-limit-late` | ProcessSpec output_limit is enforced only after unbounded capture | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:302` |
| `compiler-extension-json-depth-dos` | Shared package-model JSON parser permits stack-exhausting nesting | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:308` |
| `worktree-tar-entry-count-dos` | Differing worktree TAR parser has unbounded entry count and quadratic accounting | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:317` |
| `s1-root-net-file-unbounded` | Compile-time file fetch can exhaust memory on an endless special file (root) | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:291` |
| `notebook-preauth-slowloris` | Notebook server authenticates only after a blocking read on its sole connection loop | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:304` |
| `d0017-s1-tar-pax-allocation` | PAX logical size can bypass stored-size allocation limit | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:306` |
| `d0017-s3-studio-slowloris` | One incomplete Studio HTTP request blocks the single-threaded service indefinitely | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:307` |
| `nix-json-depth-dos` | Nix evaluator JSON parser has no nesting limit | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:309` |
| `devserver-slowloris-thread-exhaustion` | Devserver thread-per-connection design permits slowloris thread exhaustion | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:314` |
| `embedded-devserver-unbounded-http-resource-use` | Generated embedded devserver permits unbounded request headers and blocking connection threads | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:316` |
| `d0002-s1-http1-slow-body` | HTTP/1 request bodies have only per-read idle timeouts, allowing prolonged worker occupation | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:285` |
| `s2-fenced-range-expansion-dos` | Numbered fence ranges can exhaust compiler memory | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:293` |
| `runtime-toml-depth-dos` | Runtime TOML parser has unbounded recursive nesting | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:298` |
| `build-action-unbounded-output` | Local build actions capture unbounded output without a timeout | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:303` |
| `plugin-call-unbounded-resources` | Sandboxed WASM plugin execution lacks time and memory budgets | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:305` |
| `process-pipeline-limits-ignored` | JIT process pipelines ignore configured timeout and output limits | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:310` |
| `envhook-pretrust-symlink-recursion-dos` | Environment fingerprint follows recursive symlinks before trust gating | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:311` |
| `nix-expression-parser-depth-dos` | Foreign-flake parser recurses on nesting before evaluator depth limits apply | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:312` |
| `package-treehash-symlink-recursion` | Package hashing follows directory symlinks without cycle or root containment checks | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:313` |
| `archive-urandom-read-to-eof` | Archive key generation reads /dev/urandom to EOF | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:315` |
| `module-discovery-symlink-recursion-dos` | Recursive module discovery follows directory symlink cycles without visited-set or depth limit | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:318` |
| `comptime-fetch-ssrf` | Hash-pinned compile-time fetch permits arbitrary outbound requests before verification | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:393` |
| `cd005-comptime-fetch-local-disclosure` | Hermetic compile-time fetch can disclose arbitrary local text files | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:394` |
| `provider-registry-private-network-ssrf` | Project provider policy can authorize private-network HTTPS fetches | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:395` |
| `jit-http-crlf-injection` | JIT generic HTTP request serialization permits CRLF request injection | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:396` |
| `jit-websocket-handshake-crlf-injection` | WebSocket URL permits HTTP handshake CRLF injection | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:397` |
| `git-dependency-transport-ssrf` | Git dependency fetch allows attacker-selected network destinations | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:398` |
| `package-store-install-symlink-escape` | Package-store installation follows dependency symlinks outside the source tree | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:458` |
| `canvas-create-package-symlink-write` | Canvas `create_package` writes outside the project through symlink ancestors | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:459` |
| `dependency-name-path-traversal` | Unvalidated package identifiers escape store, project, and registry roots | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:460` |
| `devserver-static-symlink-escape` | Devserver static reads follow symlinks outside the build directory | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:461` |
| `d0002-s2-sparse-copy-symlink` | Sparse remote fetch fallback follows dependency-controlled symlinks while copying | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:462` |
| `s0-web-test-prefix-traversal` | Web-test file server uses prefix-only path containment | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:463` |
| `vendor-symlink-escape` | Vendoring follows dependency symlinks outside the source tree | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:464` |
| `cd005-comptime-embed-file-symlink` | Compile-time embed_file follows project symlinks outside the source root | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:465` |
| `cd005-comptime-embed-bytes-symlink` | Compile-time embed_bytes follows project symlinks outside the source root | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:466` |
| `cd005-build-embed-symlink` | BuildContext embed follows project symlinks outside the source root | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:467` |
| `git-revision-cache-path-traversal` | Manifest Git revisions escape the cache root and shape recursive deletion | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:468` |
| `jetpack-dotenv-symlink-read` | Project-relative dotenv validation follows symlinks outside the project | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:469` |
| `jetpack-image-files-read-traversal` | Image files entries can read arbitrary host paths outside the project | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:470` |
| `jetpack-image-layer-path-traversal` | OCI tar builder emits unvalidated and silently truncated attacker-controlled paths | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:471` |
| `canvas-action-temp-symlink-overwrite` | Predictable Canvas check file follows workspace symlinks | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:472` |
| `jetpack-overlay-patch-path-traversal` | A malicious overlay patch can overwrite files outside the source root | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:473` |
| `trust-prefix-sibling-overmatch` | Trust path prefix matching authorizes sibling project names | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:474` |
| `devserver-build-symlink-overwrite` | Project-controlled build symlink redirects finalized web outputs to host paths | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:475` |
| `lsp-predictable-log-symlink-write` | LSP panic logging follows a predictable shared temporary symlink | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:476` |
| `canvas-source-symlink-read` | Canvas projected-source scan follows directory symlinks and can disclose an external Jet file | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:477` |
| `jetpack-remote-symlink-fingerprint-escape` | Remote package fingerprint traversal follows symlinks outside the checkout | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:478` |
| `repl-run-temp-symlink-overwrite` | REPL :run writes predictable files in the shared temporary directory | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:479` |
| `devserver-cross-origin-mutation` | Loopback dev server accepts cross-origin state-changing requests without origin, host, or capability checks | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:506` |
| `s1-root-studio-remote-run` | Unauthenticated non-loopback Studio clients can invoke JetOS build, proof, and switch actions (root) | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:509` |
| `s2-devserver-source-disclosure` | Loopback devserver exposes project source without Host or session authentication | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:511` |
| `s1-root-studio-remote-write` | Unauthenticated non-loopback Studio clients can mint sessions and overwrite config.jet (root) | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:508` |
| `s2-devserver-debug-trigger` | Unauthenticated devserver endpoint can start project debug execution | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:512` |
| `s1-root-studio-remote-read` | Non-loopback jetos Studio exposes system projection and config.jet without authentication (root) | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:507` |
| `devserver-windows-absolute-static-path` | Windows absolute paths can escape the devserver static build root | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:513` |
| `s1-root-studio-loopback-csrf` | A malicious website can trigger loopback Studio JetOS subprocess actions via CSRF (root) | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:510` |
| `canvas-project-revision-not-enforced` | Canvas project transactions parse but do not enforce project_revision | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:514` |
| `embedded-devserver-windows-absolute-static-path` | Generated embedded devserver can read outside build root on Windows absolute paths | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:515` |
| `git-ls-remote-option-injection` | Manifest-controlled Git URL can inject `git ls-remote` options | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:552` |
| `git-clone-option-injection` | Manifest-controlled Git URL can inject `git clone` options | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:553` |
| `s0-bash-prompt-label-injection` | Project prompt label is embedded unescaped in generated Bash startup code | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:554` |
| `package-git-fetch-option-injection` | Package Git revision fragments can inject fetch options | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:557` |
| `s0-zsh-prompt-label-injection` | Project prompt label is embedded unescaped in generated Zsh startup code | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:555` |
| `s0-fish-prompt-label-injection` | Project prompt label is embedded unescaped in Fish startup commands | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:556` |
| `lldb-breakpoint-command-injection` | Generated filenames are interpolated into LLDB command text | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:559` |
| `vscode-workspace-lsp-rce` | VS Code extension auto-executes a workspace-controlled language server binary | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:569` |
| `zed-worktree-lsp-rce` | Zed extension selects a worktree-controlled language server binary | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:570` |
| `package-git-checkout-option-injection` | Package Git revision fragments can inject checkout options | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:558` |
| `web-codegen-template-injection` | Jet string literals are emitted raw into generated JavaScript templates | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:560` |
| `rustc-build-profile-env-injection` | Project build-profile environment reaches the rustc process | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:561` |
| `jetpack-self-authorized-build-script` | Project policy can self-authorize unsandboxed dependency build script | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:562` |
| `package-git-kind-probe-option-injection` | Jetpack provider-kind probe parses an untrusted revision as a Git option | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:563` |
| `jetos-storage-disk-command-injection` | JetOS storage disk value becomes executable second-stage shell syntax | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:564` |
| `jetos-storage-esp-command-injection` | JetOS ESP size is embedded unescaped in an executed apply script | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:565` |
| `envhook-profile-var-name-shell-injection` | Unvalidated profile variable names inject commands into auto-activation scripts | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:566` |
| `envhook-unset-name-shell-injection` | Unvalidated lifecycle unset names inject shell commands into auto-activation | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:567` |
| `claude-hook-relative-path-rce` | Claude hooks prefer cwd-relative scripts, enabling path-hijack command execution | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:568` |
| `perl-bind-compile-exec` | Perl binding inspection executes project compile-time blocks with host authority | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:571` |
| `package-store-incomplete-content-hash` | Package store integrity hashes omit copied non-.jet files | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:596` |
| `jetpack-typed-environment-trust-bypass` | Typed Jetpack environments bypass trust for lifecycle hooks and services when packages and secrets are empty | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:598` |
| `locked-dependency-integrity-bypass` | Locked builds use mutable dependency sources without content verification | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:595` |
| `git-revision-utf8-slice-panic` | Multibyte Git revisions panic dependency resolution at a byte boundary | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:597` |
| `buildrecipe-exec-unsandboxed` | BuildRecipe run executes tools with ambient host authority | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:599` |
| `buildrecipe-logged-unsandboxed` | Logged BuildRecipe execution bypasses the promised sandbox | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:600` |
| `transitive-path-dependency-escape` | Transitive path dependencies can escape their fetched dependency root | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:601` |
| `secrets-key-permission-window` | Age identity is created with ambient permissions before best-effort chmod | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:620` |
| `jetpack-project-trust-self-allow` | Untrusted projects can self-authorize the trust gate that precedes environment hooks | `already-fixed` | `docs/audits/security-deep-scan-2026-08-03.md:621` |
<!-- /security-dispositions -->

## Independent review

The review table preserves the card-required independent-review record. Each campaign card log is the owning review record; the linked source range is the corresponding final-disposition and recitation evidence.

<!-- security-independent-review:v1 -->
| campaign | owning card | candidate count | independent review evidence |
| --- | --- | ---: | --- |
| Tower authorization, CSRF, and document containment | #1377 | 11 | Tower card #1377 log; `docs/audits/security-deep-scan-2026-08-03.md:94-119` |
| JIT, WebAssembly, FFI, and ABI memory safety | #1378 | 9 | Tower card #1378 log; `docs/audits/security-deep-scan-2026-08-03.md:172-186` |
| Identity, tokens, secrets, and cryptography | #1379 | 12 | Tower card #1379 log; `docs/audits/security-deep-scan-2026-08-03.md:209-232` |
| Resource bounds, parser depth, and service availability | #1380 | 35 | Tower card #1380 log; `docs/audits/security-deep-scan-2026-08-03.md:320-383` |
| Network egress, SSRF, HTTP framing, and local disclosure | #1381 | 6 | Tower card #1381 log; `docs/audits/security-deep-scan-2026-08-03.md:400-412` |
| Filesystem roots, symlinks, temporary files, and path containment | #1382 | 22 | Tower card #1382 log; `docs/audits/security-deep-scan-2026-08-03.md:445-479` |
| Devserver, Canvas, and Studio control planes | #1383 | 10 | Tower card #1383 log; `docs/audits/security-deep-scan-2026-08-03.md:500-515` |
| Command, shell, editor, and generated-code injection | #1384 | 20 | Tower card #1384 log; `docs/audits/security-deep-scan-2026-08-03.md:546-571` |
| Package, Git, provider, store, and dependency integrity | #1385 | 7 | Tower card #1385 log; `docs/audits/security-deep-scan-2026-08-03.md:589-601` |
| Trust policy, sandbox claims, concurrency, and remaining integrity gaps | #1386 | 2 | Tower card #1386 log; `docs/audits/security-deep-scan-2026-08-03.md:603-621` |
<!-- /security-independent-review -->

All ten campaign scopes are represented. The current source reconciliation contains no confirmed candidate that would require a new root-cause fix or hostile regression before this gate.

## Canonical evidence hashes

SHA-256 covers every repository artifact referenced by the criterion, disposition, and independent-review evidence paths. The report is excluded from its own hash table to avoid a self-referential digest.

<!-- security-evidence-hashes:v1 -->
| artifact | sha256 |
| --- | --- |
| `docs/audits/security-deep-scan-2026-08-03.md` | ea31f76760fd1714662936436ac11528f1bcaad1e55cf25770a5f0d5fb2b7780 |
| `docs/audits/security-deep-scan-2026-08-03-full.md` | 87c7ff0e7cdce8938a23b1c2c8c8909daf05194efcc98a29e589f8a9acb439e7 |
| `docs/audits/security-deep-scan-2026-08-03-full-tower-control-plane.md` | f0eabf9f49bb511a1c8cdd7cab99889af1301a5514a5a8c835f33805f4379a0c |
| `docs/agents/security-closure.md` | 1f5b0eb93489e55c9b3b9476dc93fcfc5fc59342f81b1e4add71b4da2491e2ee |
| `scripts/agent/security-scan.mjs` | 455edd4a429a7452fd0124788f1e92fd7e01a3608b95b8aa8f50992cb9b6d769 |
<!-- /security-evidence-hashes -->

## Fresh external scan gate

Status: `pending-external-gate`. A Codex Security plugin is installed at `/home/nate/.codex/plugins/cache/openai-curated-remote/codex-security/0.1.22`; its `scripts/finalize_scan_contract.py` and `scripts/validate_scan_contract.py` files are present.

This pass did not execute the external scan or fabricate its canonical manifest, findings, coverage, or receipt. Run the following exact gate once a new preparation request and the returned scan directory are available:

~~~sh
CODEX_SECURITY_PLUGIN_DIR=/home/nate/.codex/plugins/cache/openai-curated-remote/codex-security/0.1.22
publish=/home/nate/Projects/Github/jet/docs/audits/security-final-2026-09-02
scripts/agent/jet-env full node scripts/agent/security-scan.mjs \
  finalize \
  --repo /home/nate/Projects/Github/jet \
  --request <REQUEST_DIR>/request.json \
  --scan-dir <SCAN_DIR> \
  --plugin-dir "$CODEX_SECURITY_PLUGIN_DIR" \
  --publish-dir "$publish"
~~~

The gate must run after the clean-revision preparation and the Codex Security completion operation. It must pass the official plugin validator, report zero reportable findings, report zero deferred work and open questions, and publish the canonical evidence directory.

The attempted run observed HEAD `08243a6aa42f0e960c9df54b5cc22f55934c93d7`.
The shared checkout was dirty because sibling implementation lanes were editing it.
Preparation stopped before scan execution with `security-scan: repository must be clean before a security scan`.
The requested finalization then stopped because no request existed:
`security-scan: scan request does not exist: /home/nate/.cache/jet-luna/security/request-2026-09-02/request.json`.
No scan artifacts or publish directory were created.

## Receipt

<!-- security-receipt:v1 -->
~~~json
{
  "documentType": "jet.security-closure.receipt",
  "schemaVersion": "1.0",
  "card": "#1387",
  "report": "docs/audits/security-closure-2026-09-02.md",
  "candidateCount": 134,
  "uniqueDispositionCount": 134,
  "conflictingDispositionCount": 0,
  "unresolvedCandidates": 0,
  "dispositions": {
    "already-fixed": 133,
    "duplicate-of-d0017-s1-aot-termios-layout": 1
  },
  "evidenceHashAlgorithm": "sha256",
  "evidenceHashTable": "security-evidence-hashes:v1",
  "observedHead": "08243a6aa42f0e960c9df54b5cc22f55934c93d7",
  "worktree": {
    "status": "dirty",
    "note": "Sibling implementation lanes edited the shared checkout; preparation refused before scan execution.",
    "prepareLastLine": "security-scan: repository must be clean before a security scan",
    "finalizeLastLine": "security-scan: scan request does not exist: /home/nate/.cache/jet-luna/security/request-2026-09-02/request.json"
  },
  "freshRepositoryScan": {
    "status": "pending-external-gate",
    "pluginDirectory": "/home/nate/.codex/plugins/cache/openai-curated-remote/codex-security/0.1.22",
    "command": "scripts/agent/jet-env full node scripts/agent/security-scan.mjs finalize --repo /home/nate/Projects/Github/jet --request <REQUEST_DIR>/request.json --scan-dir <SCAN_DIR> --plugin-dir /home/nate/.codex/plugins/cache/openai-curated-remote/codex-security/0.1.22 --publish-dir /home/nate/Projects/Github/jet/docs/audits/security-final-2026-09-02"
  }
}
~~~
<!-- /security-receipt -->

## Closure decision

The candidate reconciliation is complete and machine-validatable. Card #1387 remains open for criterion 3 and the external portion of criterion 4 until the exact fresh-scan gate above passes at the integration revision.
