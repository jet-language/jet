# Framework transplants E3 status

Shipped-law reconciliation for 11 framework-transplant decisions and related
cards (#505/#506/#1157–#1161). `open` means a bounded slice exists or the full
ratified scope remains incomplete. `#1157`, `#1159`, and `#1160` are verified
child evidence, not proof that every broader proposal claim shipped.

## Truth ledger

| Decision | status | production path | evidence path | successor/owner | boundary |
|----------|--------|-----------------|---------------|-----------------|----------|
| D-LIVEQUERY1 | open | `crates/jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs`; `crates/jet-comptime/src/Comptime/AppLite.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs` | `examples/features/tooling/app_live.jet`; `examples/features/expected/tooling/app_live.out`; `tests/ui/db_read_query_hidden_write.jet` | `#1158` closeout; remote transport/app graph owners | Process-global mutex registry stores typed normalized footprints, returns explicit errors, and evicts the oldest entry at its 1024-query bound. Write-set invalidation marks matching queries dirty; evicted or unavailable handles report errors. `signal_push` commits a local receipt. No app query rerun, remote reconnect, or actual `core.ws` transport. |
| D-SCHEDULE1 | open | `crates/jet-parser/src/Parser/Items/markers_contracts.rs`; `crates/jet-sema/src/Sema/CheckerSchedule.rs` | `examples/features/devloop/schedule_every.jet`; `tests/dev.rs`; `tests/ui/schedule_*` | `#1157` verified child; `#1158` closeout | Parser/sema checks and the shared `EveryArg::resolve` law ship. Service-runtime and jetos timer consumers are not wired to the checked schedule metadata. |
| D-LINTPOLICY1 | shipped | `crates/jet-pkg-model/src/LintPolicy.rs`; `Source/CmdCompile.rs`; `crates/jet-semindex/src/Build.rs`; `crates/jet-semindex/src/Types.rs`; `crates/jet-semindex/src/JSON.rs` | `tests/pkg.rs`; `tests/semindex.rs`; `docs/spec/diagnostics.md` | `#1158` closeout | Warnings stay non-blocking by default. Explicit `package.jet` `policy.lints.deny` removes matching findings from the warning stream and emits one E1293 per site. Memory/type safety has no override. |
| D-BINPAT1 | shipped | `crates/jet-parser/src/Parser/Expressions/patterns.rs`; `crates/jet-sema/src/Sema/CheckerInfer/binary.rs`; `crates/jet-parser/src/Formatter/Expressions.rs` | `examples/features/parsing/binary_pattern.jet`; `examples/features/parsing/binary-reader.jet`; `tests/ui/binpat_*`; `tests/fmt.rs` | `#506` slice; no open owner gate recorded | `[U8].{"…"}` is byte-mode in one pattern engine. Bit widths and endian rules are checked; retired `b"…"` spelling stays retired. |
| D-STM1 | open | `crates/jet-codegen/src/Prelude/Core/RuntimeControl.rs`; `crates/jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs`; `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs` | `examples/features/memory/shared_transact.jet`; `tests/dev.rs`; `tests/jit_gaps.txt` | `#1161`; owner follow-up if retry-on-conflict is required | Shared edits use an explicit typed transaction guard and canonical lock-fold atomic commit. `shared_transact` is covered by the interpreter/JIT gap ledger; no ratified optimistic retry-on-conflict semantics are claimed. |
| D-AUTH1 | open | `crates/jet-codegen/src/Prelude/CoreLib/Top/Auth.rs`; `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs`; `crates/jet-comptime/src/Comptime/AuthLite.rs` | `examples/features/crypto/auth_sessions.jet`; `examples/features/crypto/auth_tokens.jet`; `tests/taint.rs`; `tests/dev.rs` | `#1161`; app graph, provider, and email owners | Token verification and session helpers use a process-global typed store, checked expiry, single-use state, and OS cryptographic entropy. Magic links require a registered user with a syntactically valid delivery identity, and consume rechecks that identity. No durable DB-backed app routes, provider network, actual email delivery, or remote auth reconnect. |
| D-SYNC1 | open | `crates/jet-codegen/src/Prelude/CoreLib/Top/Sync.rs`; `crates/jet-comptime/src/Comptime/SyncLite.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs` | `examples/features/tooling/sync_crdt.jet`; `tests/sync_crdt.rs`; `docs/reference/core-library.md` | `#1159` verified child; `#1161` for remaining transport scope | Typed CRDT carriers/codecs and typed bounded session receipts exist. `JET_SYNC_SESSIONS` is process-local and duplicate publishes are idempotent; no remote authenticated reconnect, transport, or merge protocol. |
| D-VALIDATE1 | open | `crates/jet-sema/src/Sema/CheckerValidate.rs`; `crates/jet-sema/src/Sema/Registration/Serde.rs`; `crates/jet-codegen/src/Codegen/Context.rs`; `crates/jet-parser/src/Parser/Items/states_protocols.rs` | `examples/features/serde/validate.jet`; `tests/ui/validate_*`; `docs/spec/spec.md` | `#1161`; ratified `D-VALIDATE-DECODE1=B` | In-body `validate {}` and `Type.validate` ship. Derived struct decoders now run their synthesized validator automatically, and every typed Decode codec uses the canonical `[FieldError]` result contract. Hand-written codecs opt in explicitly; `Validate.over` remains open integration work. Rule vocabulary is `check(...)` only. |
| D-DBPOLICY1 | open | `crates/jet-codegen/src/Prelude/CoreLib/Top/Sync.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs`; `crates/jet-jit/src/DB.rs` | `tests/db_policy.rs`; `examples/features/io/db_policy.jet`; `docs/reference/core-library.md` | `#1160` verified child; `#1161` for full policy scope | Bounded `DBScope` carries policy and user through query, mutation, transaction, and live paths. Policy expressions are only `true` and `owner == user`; no general closure or multi-tenant policy compiler. |
| D-ENVHOOK1 | shipped | `crates/jetpack/src/EnvHook.rs`; `crates/jetpack/src/CLI/run_enter_dev.rs`; `crates/jet-foundation/src/Syntax/effects_surface.rs` | `tests/env_hook.rs`; `tests/env_dev_trust.rs`; `docs/reference/environment.md` | `#506` slice; env-model follow-up for arbitrary shell variables | Opt-in bash/zsh/fish hook, trust gate, activation/deactivation, and `JET_ENV_DISABLE` ship. `env.jet` does not declare arbitrary shell variables such as `PGHOST`. |
| D-OBSERVE-LIVE1 | shipped | `crates/jet-codegen/src/Prelude/Observe.rs`; `crates/jet-devserver/src/LiveInspect.rs`; `crates/jet-cli/src/CLI.rs` | `tests/live_inspect.rs`; `tests/event_observations.rs`; `docs/reference/core-library.md` | `#506` slice; no open successor | `JET_OBSERVE=1` publishes bounded PID-scoped facts. Tasks, channels, effects, and arena stats are visible; payloads, locals, environment, credentials, and arbitrary process memory are not. |

## Open implementation handoff

### D-LIVEQUERY1

The remaining live-query work is an integration seam, not another registry
feature. `LiveQuery.rs` has no typed query runner or `JetSignal<T>` binding:
`jet_app_invalidate` can mark a record dirty, but it cannot rerun the query or
deliver its typed result. The existing `JetSignal<T>` in
`crates/jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs` is the
canonical signal runtime and must remain the delivery target.

The emitter owner must connect the typed app/query graph to that runtime at
`crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs`, then make the
transaction write set call the registered rerunner and publish the new result
through the same signal. `Codegen/mod.rs` currently includes the live registry
for `app.live`, but does not include the existing `core.ws` fragments for that
path; the transport adapter must use those existing WebSocket mechanisms or an
already-owned equivalent. `AppLite.rs` must marshal the same contract for
interpreter/comptime parity. Do not add a local callback queue, a second
signal implementation, or a local-only browser protocol.

This batch leaves that seam untouched because `core_calls.rs` is owned by the
Service worker. Required handoff inputs are: typed query identity and rerun
callback, result-to-`JetSignal<T>` binding, `core.ws` inclusion/connection
lifecycle, and the matching `AppLite` adapter. Until those inputs exist, the
bounded dirty registry is the honest shipped boundary; claiming rerun,
transport, or remote Signal delivery would be a stub.

### D-SCHEDULE1

`jet dev` already consumes `Interpreter::scheduled_tasks` and
`EverySchedule`, which comes from the checked `EveryArg::resolve` law. The
authorized service/jetos path has no equivalent typed input: `DevServicePlan`
has service-process metadata, `ServicePlan` captures only `enable` plus open
`extra` fields, and the jetos systemd projection reads raw service
`timer`/`schedule` extras. None carries a project `#Job` identity and its
resolved `EverySchedule`.

Wiring that path requires the still-missing D-SERVICE1 typed
builder/worker/group boundary to carry `(task, EverySchedule)` into the
service runtime and jetos projection. Do not reinterpret a raw service extra
as `#Every`, add a second scheduler, or modify the service worker's runtime
slice in this batch. The exact handoff is: expose the checked task schedule in
the service plan, let the one runtime consume it, and let jetos project that
same value with operator-override provenance.

## Non-goals / honesty

- Full Convex-class multi-tenant authorization matrices beyond the shipped
  footprint/session APIs remain product follow-ups, not silent stubs.
- The DB policy expression is a closed v1 language (`true` and `owner == user`),
  with exact SQL target validation. It is not a general user-supplied policy
  closure compiler.
- CRDT values preserve replica contributions and converge under their declared
  laws, but the current session registry is process-local. It is not a remote
  authenticated reconnect implementation.
- The live path is a bounded typed registry with explicit errors and dirty
  marking. It counts `ws_pushes` as a local signal receipt after an explicit
  payload commit. It is not a browser `core.ws` wire protocol, query rerun
  graph, or remote Signal transport.

## Implementation checkpoint

The current production patch replaces the live-query and auth thread-local
registries with process-global mutex state, bounds live-query state with
oldest-entry eviction and explicit closed-handle errors, requires a registered
delivery-capable identity for magic links, makes STM buffering an explicit
typed guard, validates live-inspect JSON through the shared JSON parser, and
rejects malformed row-policy tables and non-canonical sync documents. Tests,
builds, and formatter checks are intentionally pending for this checkpoint.
