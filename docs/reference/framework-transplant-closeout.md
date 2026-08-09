# Framework transplants E3 status

Shipped-law reconciliation for 11 framework-transplant decisions, the
D-VALIDATE-DECODE1 contract, and related
cards (#505/#506/#1157–#1161). `unshipped-law` names behavior required by a
ratified decision but absent from the current production path. It is not an
owner choice. `#1157`, `#1159`, and `#1160` are verified child
evidence, not proof that every broader law shipped.

## Truth ledger

| Decision | status | production path | evidence path | successor/owner | boundary |
|----------|--------|-----------------|---------------|-----------------|----------|
| D-LIVEQUERY1 | unshipped-law | `crates/jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs`; `crates/jet-comptime/src/Comptime/AppLite.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs` | `examples/features/tooling/app_live.jet`; `examples/features/expected/tooling/app_live.out`; `tests/ui/db_read_query_hidden_write.jet` | `#1158` closeout; remote transport/app graph owners | Process-global mutex registry stores typed normalized footprints, returns explicit errors, and evicts the oldest entry at its 1024-query bound. Write-set invalidation marks matching queries dirty; evicted or unavailable handles report errors. `signal_push` commits a local receipt. The typed query rerun, `Signal<T>` binding, remote reconnect, and actual `core.ws` transport are unshipped. |
| D-SCHEDULE1 | unshipped-law | `crates/jet-parser/src/Parser/Items/markers_contracts.rs`; `crates/jet-sema/src/Sema/CheckerSchedule.rs` | `examples/features/devloop/schedule_every.jet`; `tests/dev.rs`; `tests/ui/schedule_*` | `#1157` verified child; `#1158` closeout | Parser/sema checks and the shared `EveryArg::resolve` law ship. The D-CONC-SCHED1 typed `Duration`/wall-clock value, `2h` and `1d`, service-runtime and jetos timer consumers, and one Job/task vocabulary are unshipped. |
| D-LINTPOLICY1 | shipped | `crates/jet-pkg-model/src/LintPolicy.rs`; `Source/CmdCompile.rs`; `crates/jet-semindex/src/Build.rs`; `crates/jet-semindex/src/Types.rs`; `crates/jet-semindex/src/JSON.rs` | `tests/pkg.rs`; `tests/semindex.rs`; `docs/spec/diagnostics.md` | `#1158` closeout | Warnings stay non-blocking by default. Explicit `package.jet` `policy.lints.deny` removes matching findings from the warning stream and emits one E1293 per site. Memory/type safety has no override. |
| D-BINPAT1 | shipped | `crates/jet-parser/src/Parser/Expressions/patterns.rs`; `crates/jet-sema/src/Sema/CheckerInfer/binary.rs`; `crates/jet-parser/src/Formatter/Expressions.rs` | `examples/features/parsing/binary_pattern.jet`; `examples/features/parsing/binary-reader.jet`; `tests/ui/binpat_*`; `tests/fmt.rs` | `#506` slice; no owner gate recorded | `[U8].{"…"}` is byte-mode in one pattern engine. Bit widths and endian rules are checked; retired `b"…"` spelling stays retired. |
| D-STM1 | shipped | `crates/jet-codegen/src/Prelude/Core/RuntimeControl.rs`; `crates/jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs`; `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs` | `examples/features/memory/shared_transact.jet`; `tests/dev.rs` | `#1161` | D-CONC-STM1 amends the old retry text. The body runs once; the commit takes touched locks in fixed order, applies buffered edits, and waits on contention. Rollback drops buffered edits before commit. |
| D-AUTH1 | unshipped-law | `crates/jet-codegen/src/Prelude/CoreLib/Top/Auth.rs`; `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs`; `crates/jet-comptime/src/Comptime/AuthLite.rs` | `examples/features/crypto/auth_sessions.jet`; `examples/features/crypto/auth_tokens.jet`; `tests/taint.rs`; `tests/dev.rs` | `#1161` | Token verification and session helpers use a process-global typed store, checked expiry, single-use state, and OS cryptographic entropy. Magic links require a registered user with a syntactically valid delivery identity, and consume rechecks that identity. Durable DB-backed app routes, provider network, actual email delivery, and remote auth reconnect are unshipped. |
| D-SYNC1 | unshipped-law | `crates/jet-codegen/src/Prelude/CoreLib/Top/Sync.rs`; `crates/jet-comptime/src/Comptime/SyncLite.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs` | `examples/features/tooling/sync_crdt.jet`; `tests/sync_crdt.rs`; `docs/reference/core-library.md` | `#1159` verified child; `#1161` | Typed CRDT carriers/codecs and typed bounded session receipts exist. `JET_SYNC_SESSIONS` is process-local and duplicate publishes are idempotent. Remote authenticated reconnect, transport, and merge protocol are unshipped. |
| D-VALIDATE1 | unshipped-law | `crates/jet-sema/src/Sema/CheckerValidate.rs`; `crates/jet-sema/src/Sema/Registration/Serde.rs`; `crates/jet-codegen/src/Codegen/Context.rs`; `crates/jet-parser/src/Parser/Items/states_protocols.rs` | `examples/features/serde/validate.jet`; `tests/ui/validate_*`; `docs/spec/spec.md` | `#1161` | In-body `validate {}` and `Type.validate` ship. Derived struct decoders now run their synthesized validator automatically, and every typed Decode codec uses the canonical `[FieldError]` result contract. Hand-written codecs opt in explicitly. `Validate.over` is unshipped. Rule vocabulary is `check(...)` only. |
| D-VALIDATE-DECODE1 | shipped | `crates/jet-sema/src/Sema/Registration/Serde.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/functions.rs`; `crates/jet-codegen/src/Prelude/CoreLib/Top/DataFmt.rs` | `tests/corelib.rs`; `examples/features/serde/hand_codec.jet`; `docs/spec/syntax-decisions.md` | `#1161` | Every generated, hand-written, and format adapter Decode path returns `Result<T, [FieldError]>`. The retired codec `DecodeError` envelope has no alias or second decoder API. `AuthError::DecodeError` is a token-JSON error variant, not a codec result contract. |
| D-DBPOLICY1 | unshipped-law | `crates/jet-codegen/src/Prelude/CoreLib/Top/Sync.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs`; `crates/jet-jit/src/DB.rs` | `tests/db_policy.rs`; `examples/features/io/db_policy.jet`; `docs/reference/core-library.md` | `#1160` verified child; `#1161` | Bounded `DBScope` carries policy and user through query, mutation, transaction, and live paths. Policy expressions are only `true` and `owner == user`; the general typed closure compiler and generated per-path proof/filter are unshipped. |
| D-ENVHOOK1 | shipped | `crates/jetpack/src/EnvHook.rs`; `crates/jetpack/src/CLI/run_enter_dev.rs`; `crates/jet-foundation/src/Syntax/effects_surface.rs` | `tests/env_hook.rs`; `tests/env_dev_trust.rs`; `docs/reference/environment.md` | `#506` slice; env-model follow-up for arbitrary shell variables | Opt-in bash/zsh/fish hook, trust gate, activation/deactivation, and `JET_ENV_DISABLE` ship. `env.jet` does not declare arbitrary shell variables such as `PGHOST`. |
| D-OBSERVE-LIVE1 | shipped | `crates/jet-codegen/src/Prelude/Observe.rs`; `crates/jet-devserver/src/LiveInspect.rs`; `crates/jet-cli/src/CLI.rs` | `tests/live_inspect.rs`; `tests/event_observations.rs`; `docs/reference/core-library.md` | `#506` slice; no successor | `JET_OBSERVE=1` publishes bounded PID-scoped facts. Tasks, channels, effects, and arena stats are visible; payloads, locals, environment, credentials, and arbitrary process memory are not. |

## Unshipped ratified behavior

### D-LIVEQUERY1

The unshipped D-LIVEQUERY1 behavior is an integration seam, not another
registry feature. `LiveQuery.rs` has no typed query runner or `JetSignal<T>` binding:
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

The precise unshipped items are typed query identity and rerun callback,
result-to-`JetSignal<T>` binding, `core.ws` inclusion and connection lifecycle,
and the matching `AppLite` adapter. The bounded dirty registry is the shipped
boundary. Rerun, transport, or remote Signal delivery is not implemented.

### D-SCHEDULE1

The shipped `jet dev` path consumes `Interpreter::scheduled_tasks` and
`EverySchedule`, which comes from the checked `EveryArg::resolve` law. The
authorized service/jetos path has no equivalent typed input: `DevServicePlan`
has service-process metadata, `ServicePlan` captures only `enable` plus untyped
`extra` fields, and the jetos systemd projection reads raw service
`timer`/`schedule` extras. None carries a project `#Job` identity and its
resolved `EverySchedule`.

The unshipped D-CONC-SCHED1 items are typed `Duration` and wall-clock values on
the D-TYPE2-TIME1 rail, including `2h` and `1d`, plus the service/jetos path
that carries `(task, EverySchedule)` into one runtime. The current path must
not reinterpret a raw service extra as `#Every` or add a second scheduler.

## Non-goals / honesty

- Full Convex-class multi-tenant authorization matrices beyond the shipped
  footprint/session APIs are not part of the shipped boundary.
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
rejects malformed row-policy tables and non-canonical sync documents. Child
cards #1157 and #1160 carry their focused evidence. This docs-only
reconciliation ran no test, build, or formatter command.
