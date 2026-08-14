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
| D-LIVEQUERY1 | unshipped-law | `crates/jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs`; `crates/jet-comptime/src/Comptime/AppLite.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs`; `crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs` | `examples/features/tooling/app_live.jet`; `examples/features/expected/tooling/app_live.out`; `tests/ws_law.rs`; `tests/db_policy.rs`; `tests/ui/db_read_query_hidden_write.jet` | `#1158` closeout; remote reconnect/app graph owners | The bounded registry stores a typed rerunner and canonical signal sink. Matching invalidations rerun outside the lock, commit only the newest generation, update `JetSignal<T>`, and publish through the existing `core.net.ws` writer with serialized frames. The transport replays the latest bounded event per topic on reconnect. Connection-scoped authentication/routing, browser protocol binding, and general app-query graph discovery remain unshipped. |
| D-SCHEDULE1 | unshipped-law | `crates/jet-parser/src/Parser/Items/markers_contracts.rs`; `crates/jet-foundation/src/AST/items.rs`; `crates/jet-sema/src/Sema/CheckerSchedule.rs` | `examples/features/devloop/schedule_every.jet`; `tests/dev.rs`; `tests/ui/schedule_*` | `#1157` verified child; `#1158` closeout; D-TYPE2-TIME1 substrate | The canonical Time family accepts `2h` and `1d`, and one resolver feeds sema/dev and the typed runtime consumers. Scheduled lifecycle units use `#Job`; `task` remains the separate concurrency construct. The remaining unshipped boundary is the service-runtime/jetos path carrying `(#Job, EverySchedule)` into one runtime. |
| D-LINTPOLICY1 | shipped | `crates/jet-pkg-model/src/LintPolicy.rs`; `Source/CmdCompile.rs`; `crates/jet-semindex/src/Build.rs`; `crates/jet-semindex/src/Types.rs`; `crates/jet-semindex/src/JSON.rs` | `tests/pkg.rs`; `tests/semindex.rs`; `docs/spec/diagnostics.md` | `#1158` closeout | Warnings stay non-blocking by default. Explicit `package.jet` `policy.lints.deny` removes matching findings from the warning stream and emits one E1293 per site. Memory/type safety has no override. |
| D-BINPAT1 | shipped | `crates/jet-parser/src/Parser/Expressions/patterns.rs`; `crates/jet-sema/src/Sema/CheckerInfer/binary.rs`; `crates/jet-parser/src/Formatter/Expressions.rs` | `examples/features/parsing/binary_pattern.jet`; `examples/features/parsing/binary-reader.jet`; `tests/ui/binpat_*`; `tests/fmt.rs` | `#506` slice; no owner gate recorded | `[U8].{"…"}` is byte-mode in one pattern engine. Bit widths and endian rules are checked; retired `b"…"` spelling stays retired. |
| D-STM1 | shipped | `crates/jet-codegen/src/Prelude/Core/RuntimeControl.rs`; `crates/jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs`; `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs` | `examples/features/memory/shared_transact.jet`; `tests/dev.rs` | `#1161` | D-CONC-STM1 amends the old retry text. The body runs once; the commit takes touched locks in fixed order, applies buffered edits, and waits on contention. Rollback drops buffered edits before commit. |
| D-AUTH1 | unshipped-law | `crates/jet-codegen/src/Prelude/CoreLib/Top/Auth.rs`; `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs`; `crates/jet-comptime/src/Comptime/AuthLite.rs` | `examples/features/crypto/auth_sessions.jet`; `examples/features/crypto/auth_tokens.jet`; `tests/taint.rs`; `tests/dev.rs` | `#1161` | Token verification and session helpers use a process-global typed store, checked expiry, single-use state, and OS cryptographic entropy. Magic links require a registered user with a syntactically valid delivery identity, and consume rechecks that identity. Durable DB-backed app routes, provider network, actual email delivery, and remote auth reconnect are unshipped. |
| D-SYNC1 | unshipped-law | `crates/jet-codegen/src/Prelude/CoreLib/Top/Sync.rs`; `crates/jet-comptime/src/Comptime/SyncLite.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs` | `examples/features/tooling/sync_crdt.jet`; `tests/sync_crdt.rs`; `docs/reference/core-library.md` | `#1159` verified child; `#1161` | Typed CRDT carriers/codecs now carry an absorbing invalid state: merge overflow or malformed identity denies without truncation, and the ambient codec preserves that denial. The bounded session registry merges canonical map/list/counter displays deterministically, keeps duplicate delivery idempotent, and publishes the latest receipt through the local live transport for reconnect replay. `JET_SYNC_SESSIONS` is still process-local; authenticated routing and network transport remain unshipped. |
| D-VALIDATE1 | unshipped-law | `crates/jet-sema/src/Sema/CheckerValidate.rs`; `crates/jet-sema/src/Sema/Registration/Serde.rs`; `crates/jet-codegen/src/Codegen/Context.rs`; `crates/jet-parser/src/Parser/Items/states_protocols.rs` | `examples/features/serde/validate.jet`; `tests/ui/validate_*`; `docs/spec/spec.md` | `#1161` | In-body `validate {}` and `Type.validate` ship. Derived struct decoders now run their synthesized validator automatically, and every typed Decode codec uses the canonical `[FieldError]` result contract. Hand-written codecs opt in explicitly. `Validate.over` is unshipped. Rule vocabulary is `check(...)` only. |
| D-VALIDATE-DECODE1 | shipped | `crates/jet-sema/src/Sema/Registration/Serde.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/functions.rs`; `crates/jet-codegen/src/Prelude/CoreLib/Top/DataFmt.rs` | `tests/corelib.rs`; `examples/features/serde/hand_codec.jet`; `docs/spec/syntax-decisions.md` | `#1161` | Every generated, hand-written, and format adapter Decode path returns `Result<T, [FieldError]>`. The retired codec `DecodeError` envelope has no alias or second decoder API. `AuthError::DecodeError` is a token-JSON error variant, not a codec result contract. |
| D-DBPOLICY1 | unshipped-law | `crates/jet-codegen/src/Prelude/CoreLib/Top/Sync.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs`; `crates/jet-jit/src/DB.rs` | `tests/db_policy.rs`; `examples/features/io/db_policy.jet`; `docs/reference/core-library.md` | `#1160` verified child; `#1161` | Bounded `DBScope` carries policy and user through query, mutation, transaction, and live paths. Policy expressions are only `true` and `owner == user`; the general typed closure compiler and generated per-path proof/filter are unshipped. |
| D-ENVHOOK1 | shipped | `crates/jetpack/src/EnvHook.rs`; `crates/jetpack/src/CLI/run_enter_dev.rs`; `crates/jet-foundation/src/Syntax/effects_surface.rs` | `tests/env_hook.rs`; `tests/env_dev_trust.rs`; `docs/reference/environment.md` | `#506` slice; env-model follow-up for arbitrary shell variables | Opt-in bash/zsh/fish hook, trust gate, activation/deactivation, and `JET_ENV_DISABLE` ship. `env.jet` does not declare arbitrary shell variables such as `PGHOST`. |
| D-OBSERVE-LIVE1 | shipped | `crates/jet-codegen/src/Prelude/Observe.rs`; `crates/jet-devserver/src/LiveInspect.rs`; `crates/jet-cli/src/CLI.rs` | `tests/live_inspect.rs`; `tests/event_observations.rs`; `docs/reference/core-library.md` | `#506` slice; no successor | `JET_OBSERVE=1` publishes bounded PID-scoped facts. Tasks, channels, effects, and arena stats are visible; payloads, locals, environment, credentials, and arbitrary process memory are not. |

## Unshipped ratified behavior

### D-LIVEQUERY1

The remaining D-LIVEQUERY1 gap is the app/query graph and remote lifecycle, not
the local rerun seam. `LiveQuery.rs` stores a typed rerunner and sink in the
shared Prelude registry. `DBScope.live` registers a policy-aware query rerun;
the AOT emitter binds its result to the canonical `JetSignal<T>`, and the
ambient/JIT adapter calls the same Prelude. Invalidation releases the registry
lock before rerunning, rejects stale generations, and sends successful values
through the existing `core.net.ws` writer. The transport replays the latest
bounded event for each topic when a connection registers. No local callback
queue, second signal, or browser-only protocol was added.

The remaining items are general typed app/query graph discovery, connection-
scoped authentication/routing, and a client-side protocol that maps wire events
to the browser's `Signal<T>`. Those require the still-unbuilt app graph and
remote-transport substrate; the local production paths now close their
truthful seam and replay the latest bounded topic event on reconnect.


### D-SCHEDULE1

The shipped `jet dev` path consumes `Interpreter::scheduled_tasks` and
`EverySchedule`, which sema writes from the registered Time-family facts. The
authorized service/jetos path has no equivalent typed input: `DevServicePlan`
has service-process metadata, `ServicePlan` captures only `enable` plus untyped
`extra` fields, and the jetos systemd projection reads raw service
`timer`/`schedule` extras. None carries a project `#Job` identity and its
resolved `EverySchedule`.

The shared literal plane now resolves `2h` and `1d` with the same nanosecond
arithmetic as other duration surfaces. The unshipped D-CONC-SCHED1 item is the
service/jetos path that carries `(#Job, EverySchedule)` into one runtime. The
current path must not reinterpret a raw service extra as `#Every` or add a
second scheduler.

## Non-goals / honesty

- Full Convex-class multi-tenant authorization matrices beyond the shipped
  footprint/session APIs are not part of the shipped boundary.
- The DB policy expression is a closed v1 language (`true` and `owner == user`),
  with exact SQL target validation. It is not a general user-supplied policy
  closure compiler.
- CRDT values preserve replica contributions and converge under their declared
  laws, and the current process-local session publishes a bounded latest-state
  event for local reconnect replay. The fixed String session seam does not carry
  typed atom/LWW metadata or vector clocks. It is not an authenticated remote
  routing or network merge implementation.
- The live path is a bounded typed registry with explicit errors, generation
  checks, query rerun, canonical signal delivery, serialized native `core.net.ws`
  publication, and latest-topic replay on reconnect. It is not the browser
  protocol, connection-scoped authorization, general app-query graph, or
  remote authenticated reconnect implementation.

## Implementation checkpoint

The current production patch replaces the live-query and auth thread-local
registries with process-global mutex state, bounds live-query state with
oldest-entry eviction and explicit closed-handle errors, requires a registered
delivery-capable identity for magic links, makes STM buffering an explicit
typed guard, validates live-inspect JSON through the shared JSON parser, and
rejects malformed row-policy tables and non-canonical sync documents. Child
cards #1157 and #1160 carry their focused evidence. This implementation pass
ran no test, build, formatter, linter, or devtool command; the orchestrator
owns that verification.
