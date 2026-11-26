# Design Review: Errors with Two Audiences (`Describe` Trait with `public()` and `detail()`)

- **Date:** 2026-11-26
- **Scope:** Proposal item P12 — errors with two audiences (`Describe` trait with `public()` and `detail()`, chosen by each boundary) across Prelude, Core error types, HTTP, CLI, job reports, and MCP/tool-result boundaries.
- **Verdict:** **PASS WITH CUTOVER CONDITIONS** — Jet already separates user display (`Display`) from developer inspection (`Debug`) and stores structured causes and context metadata on the native `Err` handle (`Compiler/JetRuntime/Source/Err.jet`, `ErrReport.jet`). However, today every external boundary either collapses caller and operator text into one `"{err}"` string (leaking file paths, SQL fragments, host addresses, OS codes, and causes) or silently drops the failure on HTTP 500 without recording operator detail (`Core/HTTP/Server.jet:1186-1193`). Introducing one ambient `Describe` trait in `Compiler/Prelude/Traits.jet`, safe default `Describe` semantics on `Err`, `Describe` implementations on Core's error types, and cutting over every Core boundary so callers receive `public()` while operators receive `detail()` closes both failure modes at the root.

---

## 1. Current Inventory: Every Error Formatting and Rendering Path Today

### 1.1 Runtime & Prelude (`Compiler/Prelude/`, `Compiler/JetRuntime/Source/`, `Core/Prelude/`)

| Subsystem | File & Lines | Symbol(s) | Current Behavior | Gap / Hazard Today |
| :--- | :--- | :--- | :--- | :--- |
| Ambient traits | `Compiler/Prelude/Traits.jet:228-248` | `Display`, `Debug` | Single-audience `display(self) -> String` and `debug(self) -> String`. | No trait separates caller-safe text (`public()`) from operator diagnostic text (`detail()`). |
| Default `Err` declaration | `Compiler/Prelude/Errors.jet:85-95` | `Err` (`message`, `code`, `cause`, `__jet___memo_error_metadata`) | Stores `message`, optional `code`, optional `cause`, and hidden metadata. | No `Describe` impl; `Err.message` holds arbitrary internal text, including paths/SQL/host strings populated by `impl <CoreError> -> Err { return Err("{self}") }` (`Compiler/Prelude/Errors.jet:103-235`). |
| Native `Err` display/debug | `Compiler/JetRuntime/Source/Err.jet:217-229` | `jet_rt_handle_display_jet_err`, `jet_rt_handle_show_jet_err`, `jet_rt_handle_debug_jet_err` | All three return `ERR_MESSAGE` verbatim without separating audiences or walking `ERR_CAUSE`. | Interpolating `"{err}"` exposes raw internal `message` and omits the `cause` chain. |
| Native `Err` root report | `Compiler/JetRuntime/Source/ErrReport.jet:15-95, 176-260` | `err_root_width`, `jet_render_err`, `jet_error_report_json` | Renders full operator report (`Error: <code>: <message>`, `cause:`, context frames, file:line, conversions, details) for unhandled process exits. | Only invoked on process termination (`JourneyTLS.jet:147-186`); not available through a typed `Describe` contract for library boundaries. |
| Core Prelude errors | `Core/Prelude/Prelude.jet:204-235` | `SendError<T>`, `TaskFailure`, `ParseError` | `ParseError` implements `Display` echoing `self.message`; `SendError` and `TaskFailure` have no `Describe` impl. | `TaskFailure.Panicked(msg)` carries internal panic strings/locations; `ParseError.message` carries raw input tokens. |

### 1.2 Core Boundaries (`Core/HTTP/`, `Core/Args/`, `Core/Jobs/`, `Core/Service/`, `Core/App/`, `Core/Web/`, `Core/Plugin/`)

| Boundary | File & Lines | Symbol(s) | Current Error Rendering | Cutover Required |
| :--- | :--- | :--- | :--- | :--- |
| HTTP server dispatch | `Core/HTTP/Server.jet:1164-1193` | `dispatch_shielded`, `__dispatch`, `error_response`, `status_page` | `error_response(error: HTTPError)` maps most variants to `status_page(500)` (`"500 Internal Server Error"`) and **silently discards `error`**. No operator record/log receives `detail()`. | Record `HTTPErrorRecord` with `error.detail()` (request id, method, route, status) to the mux operator sink (`on_error` / `error_records()`), return `error.public()` to the HTTP caller, and provide `HTTPMux.respond_error<E: Describe>` for handlers failing with any `Describe` error. |
| HTTP static files & CORS | `Core/HTTP/Server.jet:969-981, 1041-1043` | `static_file`, `static_file_range`, `cors_policy` | Embeds local filesystem path ``static file `{path}` could not be opened...`` inside `HTTPError.IO{operation: ...}`. | `HTTPError.public()` must NEVER echo `IO.operation`, `Resolve.host`, `Connect.address`, `TLS.stage`, `Proxy.stage`, `ResourceUnavailable.resource`, or `Internal.incident_id` internals; `HTTPError.detail()` preserves them for operators. |
| App HTTP / RPC / uploads | `Core/App/App.jet:64-68, 358-368, 421-425, 8117-8121, 8232-8235` | `AppError`, `CallError`, `Notice`, `BlobError`, `file_done(500)` | `CallError.Unreachable(String)` and `BlobError.Storage(String)` embed raw transport/storage messages; `file_done(500)` returns status 500 without recording operator detail. | Implement `Describe` on `AppError`, `CallError`, `Notice`, `BlobError` so `public()` strips host/storage details while `detail()` preserves them. |
| CLI argument & command errors | `Core/Args/Args.jet:1345-1366` | `ArgsSpec.parse`, `ArgsSpec.parse_or_exit`, `core.process.fail` | `parse` returns `Err(refusal)` and `parse_or_exit` prints raw text to stderr and exits 2; no boundary separates caller-facing `public()` CLI output from operator `detail()`. | Add `CLIErrorReport` and `ArgsSpec` error reporting methods (`format_error<E: Describe>`, `report_error<E: Describe>`) that render `error.public()` for the CLI user while capturing/forwarding `error.detail()` with cause chain for operators. |
| Job queue failure reports | `Core/Jobs/Jobs.jet:57-81, 284-288, 1064-1068, 1647-1692` | `JobError`, `JobQueueReceipt`, `JobQueueEvent`, `JobQueue.fail`, `queue_fail_on_driver` | `JobError` already has `reason: String` (copied into `JobQueueReceipt.error_reason` and `JobQueueEvent.reason`) and `detail: String?` (stored in `error_detail`), but callers must construct `JobError` manually with no `Describe` enforcement. | Add `JobError.from_describe<E: Describe>` and `JobQueue.fail_described<E: Describe>` so `receipt.error_reason` and `JobQueueEvent.reason` strictly receive `error.public()`, while operator `JobQueueRecord.error.detail` receives `error.detail()` with cause chain. |
| MCP / tool-result & service boundary | `Core/Service/Service.jet:31-62` (and `Core/Plugin/Plugin.jet:59-85`) | `ServiceError`, `service_error_show`, `PluginError`, `PluginFault` | No dedicated `core.mcp` module exists in `Core/` yet (`Docs/Spec/Reference/jetpack-epoch5.md:575` references MCP integration). `ServiceError` exposes `service_error_show(error)` returning internal strings verbatim (e.g., authority/generation/worker names); `PluginError` holds guest trap frames (`PluginFault.frames`). | Add `ToolErrorReport` in `core.service` (`from_error<E: Describe>`, `caller_json()` emitting MCP `{"isError":true,"content":[{"type":"text","text":"..."}]}` strictly from `public()`, keeping `detail()` on `operator_detail`). Implement `Describe` on `ServiceError` and `PluginError` (stripping worker/authority/trap-frame internals from `public()`). |

### 1.3 Core Error Types (`Core/` `#Error` and Compiler-Owned Core Errors)

| Area | Error Type(s) | Sensitive Fields That Must Stay Out of `public()` |
| :--- | :--- | :--- |
| Files / OS | `IOError` (`IOContext`: `operation`, `resource`, `os_code`, `cause`), `PathLiteralError` | Local filesystem paths (`resource`), OS error numbers (`os_code`), raw kernel/libc cause strings. |
| Database | `DBError`, `SQLError`, `SQLLiteralError`, `SQLSyntaxError` | SQL query text, table/column names, constraint strings, database file paths. |
| Network / HTTP / TLS | `HTTPError`, `NetError` (`NetErrorDetail`, `NetDNSError`), `TLSError`, `DTLSError`, `URLError`, `MIMEError`, `WsError` | Internal hostnames, IP:port endpoints, DNS resolver details, proxy URLs, TLS handshake internals, file paths in `IO(operation)`. |
| Auth / Crypto | `AuthError`, `CryptoError`, `FileCryptoError`, `KeyWrapError`, `VaultError`, `UUIDError` | Key material, token claims, signature verification internals, vault paths. |
| Encoding / Data / Grammar | `EncodingError` (`path`, `byte_offset`, `reason`, `cause`), `DataError` (`operation`, `row`, `column`, `reason`, `cause`), `FormatError`, `HexError`, `Base32Error`, `Base64Error`, `BinaryError`, `CBORError`, `INIError`, `XMLError`, `RegexError`, `TimeError`, `TextError`, `UTF8Error`, `RangeError` | Internal file paths, raw unredacted values/rows, low-level byte offsets/OS causes where external callers only need a safe validation/format category. |
| Services / Plugins / Jobs / App / Sys | `ServiceError`, `PluginError` (`PluginFault.frames`), `AppError`, `CallError`, `Notice`, `BlobError`, `EnvError`, `AllocError`, `ModError`, `ArchiveError`, `GzipFileError`, `ZstdFileError`, `GitError`, `EventConfigError`, `MetadataError`, `BrowserError`, `UIError`, `UIHostError`, `WebFormError`, `WebRouterError`, `WebQueryError`, `WebStoreError`, `WebTableError`, `OpenAPIError` | Worker/tree/authority identifiers, WASM guest stack frames (`PluginFrame`), environment variable names/values, requested byte sizes, storage paths, subprocess arguments. |

---

## 2. Adversarial Design Attacks & Resolutions

### Attack 1: Untyped `Err` Leakage Trap
- **Attack:** Every Core error conversion in `Compiler/Prelude/Errors.jet:103-235` writes `impl <CoreError> -> Err { return Err("{self}") }`. As a result, `Err.message` routinely holds file paths (`open /etc/shadow: permission denied`), SQL text, internal IP:port addresses, or secret-adjacent strings. If `Err.public()` returned `self.message`, any handler returning `Err` would leak internal details straight into HTTP 500 bodies, CLI output, MCP tool responses, and job receipts.
- **Resolution:** `Err.public()` **NEVER** returns `self.message`, `self.cause`, file paths, stack frames, or internal values.
  - When `self.code == .Val(code)` and `code` matches a strict safe opaque token allowlist (`[A-Za-z0-9_-]`, length 1..64 — rejecting `/`, `\`, `:`, `.`, whitespace, quotes, and control characters), `Err.public()` returns `"Request failed ({code})."`.
  - Otherwise (no code, empty code, overly long code, or any unsafe character stuffed into `code:`), `Err.public()` returns `"An internal error occurred."`.
- **Operator path:** `Err.detail()` returns `[{code}] {self.message}` (when `code` is non-empty) or `{self.message}`, followed by every link in `self.cause` rendered as `\ncaused by: ...`.

### Attack 2: Cause-Chain Recursion & Ownership on `Err`
- **Attack:** `Err` is backed by the native `Err96` handle (`Compiler/JetIR/Source/BuildExpr.jet:5154-5162`), checked strictly by `ir_default_err_drift`. Adding fields to `Err` would break `IR_DEFAULT_ERR_ABI` and every runtime consumer. Walking `self.cause` recursively could also stack-overflow on deep chains or trigger unnecessary `.copy()` churn in `Compiler/Prelude/Errors.jet`.
- **Resolution:** Keep `Err`'s exact 4-field layout intact (`message`, `code`, `cause`, `__jet___memo_error_metadata`). Implement `impl Describe` inside `Err: struct { ... }` in `Compiler/Prelude/Errors.jet` (keeping `SEMA_PRELUDE_ERROR_PROGRAM.items.len() == 7`), walking `^next_cause: self.cause` iteratively in a flat `loop` with zero `.copy()` calls.

### Attack 3: HTTP 500 Silent Drop & Header/Body Leakage
- **Attack:** Today `Core/HTTP/Server.jet:1186-1193` returns `status_page(500)` (`"500 Internal Server Error"`) and silently discards `error: HTTPError`, so operators never see why a 500 happened. Conversely, naive handlers returning `response(500, "{err}")` leak paths, SQL, and causes directly to clients.
- **Resolution:**
  - `HTTPMux` owns an operator error sink (`Shared<[HTTPErrorRecord]>` plus an optional `on_error` callback) with zero effect-row breakage on existing `-[Net, FS.Read, Time]>` servers.
  - `__dispatch` routes `HTTPError` through `error_response(mux, tagged, error)` which records `HTTPErrorRecord{request_id, method, path, status, public_message: error.public(), detail: error.detail()}` to the operator sink and returns ONLY `error.public()` to the HTTP caller (with status 413/400/415/500).
  - `HTTPMux.respond_error<E: Describe>(self, request: HTTPRequest, status: Int, error: E) -> HTTPResponse Never!` gives every handler one canonical way to answer with `error.public()` while recording `error.detail()` (with full cause chain) on the operator side.
  - HTTP 500 responses never echo `detail()`, file paths, SQL text, secret strings, or cause chains in body or headers.

### Attack 4: CLI Boundary Audience Conflation
- **Attack:** `Core/Args/Args.jet` writes raw refusal strings to stderr via `core.process.fail(2, refusal)`. Applications that catch a `Describe` error at CLI exit have no standard way to print `error.public()` to the user on stderr while recording `error.detail()` (and its cause chain) for operator diagnostics/logs.
- **Resolution:** Add `CLIErrorReport` (`exit_code`, `public_message`, `detail`) in `core.args` with `CLIErrorReport.from_error<E: Describe>` and `ArgsSpec.report_error<E: Describe>` so CLI boundaries present `public()` to callers and preserve `detail()` for operators.

### Attack 5: Job Queue Receipt vs Operator Record Leakage
- **Attack:** `JobQueueReceipt.error_reason` and `JobQueueEvent.reason` are caller-facing polling fields, while `JobQueueRecord.error.detail` is the operator-facing diagnostic column. Constructing `JobError{reason: "{err}", detail: None}` leaks internal error text directly into caller receipts and event streams.
- **Resolution:** Add `JobError.from_describe<E: Describe>(type_id: String, error: E)` and `JobQueue.fail_described<E: Describe>(self, claim: JobQueueClaim, type_id: String, error: E)` which populate `reason: error.public()` (written to `receipt.error_reason` and `JobQueueEvent.reason`) and `detail: Val(error.detail())` (written strictly to the operator-only `error_detail` column).

### Attack 6: MCP / Tool-Result Boundary
- **Attack:** Tool calls returned to LLM clients or external callers over MCP (`{"isError": true, "content": [{"type": "text", "text": "..."}]}`) are an external untrusted boundary. Passing `"{err}"` into tool output leaks host paths, SQL queries, stack frames, and credentials into model context windows.
- **Resolution:** Add `ToolErrorReport` in `core.service` with `ToolErrorReport.from_error<E: Describe>(tool: String, error: E)` and `caller_json(self) -> String Never!`. `caller_json()` serializes ONLY `public_content` (`error.public()`), while `operator_detail` holds `error.detail()` with its full cause chain for operator logs and telemetry.

### Attack 7: Duplicate Error-Formatting Paths Deleted / Consolidated
- `Core/HTTP/Server.jet`: Replace the silent-drop `error_response(error: HTTPError)` body with the single `Describe`-backed `record_and_respond_error` path shared by `__dispatch` and `HTTPMux.respond_error<E: Describe>`.
- `Core/Service/Service.jet`: Consolidate ad-hoc `service_error_show` formatting into `ServiceError`'s `Describe` (`detail()` for full message, `public()` for safe caller category text), keeping `Display` delegating through the single source of truth.
- `Core/Jobs/Jobs.jet`: Consolidate manual `JobError` error-string splitting behind `JobError.from_describe<E: Describe>` / `JobQueue.fail_described<E: Describe>`.
- `Core/Args/Args.jet`: Route structured CLI failure reporting through `CLIErrorReport` rather than ad-hoc stderr formatting.

---

## 3. Exact Cutover List (2-Stage `seed: s1` Rollout)

### Stage 1 (`seed: s1` — Compiler Prelude Trait, `Err.Describe`, and `Core.rows` Declarations)
1. `Docs/Audits/2026-11-26-errors-two-audiences-review.md` — this design review (`n1`).
2. `Compiler/Prelude/Traits.jet` — declare ambient `Describe: trait` (`public: fn(self) -> String Never!`, `detail: fn(self) -> String Never!`, `local: true`, `bare_bound: true`, `raw_return: true`) (`n2`).
3. `Compiler/JetFoundation/Source/Registry/OwnedTraitRows.jet` — regenerated via `Tools/Agent/OwnedTraitRows.jet --write` (`n2`).
4. `Compiler/Prelude/Errors.jet` — implement safe `impl Describe` inside `Err: struct` (`n2`).
5. `Compiler/JetSema/Source/Sema/Registration/PreludeDerives.jet` — synced via `Compiler/Toolchain/PreludeSources.jet --write` (`n2`).
6. `Compiler/JetSema/Source/Sema/PreludeRows.jet` — regenerated via `Compiler/Toolchain/PreludeRows/Main.jet --write` (`n2`).
7. `Compiler/Prelude/Core.rows` + boundary report data structs:
   - `Core/HTTP/Server.jet` (`HTTPErrorRecord`)
   - `Core/Args/Args.jet` (`CLIErrorReport`)
   - `Core/Service/Service.jet` (`ToolErrorReport`)
   - `Core.rows`: export `HTTPErrorRecord`, `CLIErrorReport`, `ToolErrorReport`; add `impls { IOError }` on `core.files`, `impls { TextError, UTF8Error }` on `core.text`, and `impls { RangeError }` on `core.prelude`; add `core.log` to `depends_on` of `core.http.server`, `core.args`, `core.jobs`, `core.service`.

### Stage 2 (Verified Live on Rebuilt S1 Compiler — Boundaries, Core Errors, Proof Tests, Goldens, Docs)
1. **Boundaries (`n3`)**:
   - `Core/HTTP/Server.jet` — cut over `__dispatch` / `error_response`, add `HTTPMux.on_error`, `HTTPMux.error_records`, `HTTPMux.respond_error<E: Describe>`, `HTTPErrorRecord.log`.
   - `Core/Args/Args.jet` — add `CLIErrorReport.from_error<E: Describe>`, `ArgsSpec.report_error<E: Describe>`, `ArgsSpec.fail_described<E: Describe>`.
   - `Core/Jobs/Jobs.jet` — add `JobError.from_describe<E: Describe>`, `JobQueue.fail_described<E: Describe>`.
   - `Core/Service/Service.jet` — add `ToolErrorReport.ok`, `ToolErrorReport.from_error<E: Describe>`, `ToolErrorReport.caller_json`, `ToolErrorReport.log`.
2. **Core error `Describe` implementations & leak-proof tests (`n4`)**:
   - Implement `Describe` on Core error types across `Core/Prelude/Prelude.jet`, `Core/Files/Files.jet`, `Core/HTTP/HTTP.jet`, `Core/DB/DB.jet`, `Core/Net/Net.jet`, `Core/Net/URL.jet`, `Core/Net/MIME.jet`, `Core/Net/Ws.jet`, `Core/Auth/Auth.jet`, `Core/Service/Service.jet`, `Core/Plugin/Plugin.jet`, `Core/App/App.jet`, `Core/Encoding/Encoding.jet`, `Core/Data/Data.jet`, `Core/Crypto/Crypto.jet`, `Core/Regex/Regex.jet`, `Core/Time/Time.jet`, `Core/Text/Text.jet`, `Core/Sys/Sys.jet`, `Core/Mem/Mem.jet`, `Core/Archive/Archive.jet`, `Core/Mod/Mod.jet`, `Core/Git/Git.jet`.
   - Add unit tests and end-to-end case tests proving:
     - `Err.public()` never leaks message, cause chain, file paths, stack frames, SQL, or unsafe codes, while `Err.detail()` preserves the full cause chain.
     - Core error types' `public()` strips internal paths/SQL/host:port/secrets/trap-frames while `detail()` retains full diagnostics.
     - Live HTTP 500 responses over `HTTPServer` carry zero internal detail in body/headers while the operator-side `HTTPErrorRecord` receives `detail()` with the cause chain.
     - CLI, job queue receipt vs operator record, and MCP `ToolErrorReport.caller_json()` enforce strict `public()` vs `detail()` separation.
3. **Executable example, goldens, docs, grammars (`n5`)**:
   - Executable example under `Examples/` with `.out` golden verified on `O0`/`O1`/`O2`.
   - Update `Docs/Spec/spec.md` and `Docs/Spec/Reference/core-library.md` (and `Docs/Spec/Reference/` error/prelude pages).
   - Check `Tools/Editors/` grammars (`jet.tmGrammar`, `TreeSitter`, `Zed`) for prelude trait highlighting and add `Describe` where traits like `Display`/`Debug` are listed.
