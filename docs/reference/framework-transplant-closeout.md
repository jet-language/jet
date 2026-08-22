# Framework transplants E3 status

Shipped-law reconciliation for 11 framework-transplant decisions, the
D-VALIDATE-DECODE1 contract, and related
cards (#505/#506/#1157–#1161). `unshipped-law` names behavior required by a
ratified decision but absent from the current production path. It is not an
owner choice. `#1157`, `#1159`, and `#1160` are verified child
evidence, not proof that every broader law shipped.

`shipped` in the status column means the named behavior is reachable from Jet
source through the production path. It is NOT a tier claim. Which tiers were
OBSERVED to agree is a separate fact, recorded in the observed-tier table below
from `tests/jit_corpus_gate.txt` — the only ledger here whose rows are generated
from a run rather than typed by hand. `shipped-one-tier` marks a behavior whose
producer only one tier can reach; that is an I9 gap, not a completed row.

## Truth ledger

| Decision | status | production path | evidence path | successor/owner | boundary |
|----------|--------|-----------------|---------------|-----------------|----------|
| D-LIVEQUERY1 | shipped | `crates/jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs`; `crates/jet-comptime/src/Comptime/AppLite.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs`; `crates/jet-codegen/src/Prelude/CoreLib/Top/WsClient.rs` | `examples/features/tooling/app_live.jet`; `examples/features/expected/tooling/app_live.out`; `tests/ws_law.rs`; `tests/db_policy.rs`; `tests/ui/db_read_query_hidden_write.jet` | `#1158` closeout; remote reconnect/app graph owners | The bounded registry stores a typed rerunner and canonical signal sink. Matching invalidations rerun outside the lock, commit only the newest generation, update `JetSignal<T>` (including a sink rebound during rerun), and publish through the existing `core.net.ws` writer with serialized frames. The transport replays the latest bounded event per topic on reconnect. Connection-scoped authentication/routing, browser protocol binding, and general app-query graph discovery remain unshipped. |
| D-SCHEDULE1 | shipped | `crates/jet-parser/src/Parser/Items/markers_contracts.rs`; `crates/jet-foundation/src/AST/items.rs`; `crates/jet-sema/src/Sema/CheckerSchedule.rs` | `examples/features/devloop/schedule_every.jet`; `tests/dev.rs`; `tests/ui/schedule_*` | `#1157` verified child; `#1158` closeout; D-TYPE2-TIME1 substrate | The canonical Time family accepts `2h` and `1d`, and one resolver feeds sema/dev and the typed runtime consumers. Scheduled lifecycle units use `#Job`; `task` remains the separate concurrency construct. The service-runtime path is shipped: `Prelude/Job.rs` owns `JetJobSchedule`, `JetJobClock`, and `jet_job_schedule_due`; the generated dispatch calls `jet_job_service_tick` (`Codegen/Items.rs:1430`) and the interpreter adapter converts the AST carrier to the same Prelude carrier (`Source/Interpreter.rs:400`). |
| D-LINTPOLICY1 | shipped | `crates/jet-pkg-model/src/LintPolicy.rs`; `Source/CmdCompile.rs`; `crates/jet-semindex/src/Build.rs`; `crates/jet-semindex/src/Types.rs`; `crates/jet-semindex/src/JSON.rs` | `tests/pkg.rs`; `tests/semindex.rs`; `docs/spec/diagnostics.md` | `#1158` closeout | Warnings stay non-blocking by default. Explicit `package.jet` `policy.lints.deny` removes matching findings from the warning stream and emits one E1293 per site. Memory/type safety has no override. |
| D-BINPAT1 | shipped | `crates/jet-parser/src/Parser/Expressions/patterns.rs`; `crates/jet-sema/src/Sema/CheckerInfer/binary.rs`; `crates/jet-parser/src/Formatter/Expressions.rs` | `examples/features/parsing/binary_pattern.jet`; `examples/features/parsing/binary-reader.jet`; `tests/ui/binpat_*`; `tests/fmt.rs` | `#506` slice; no owner gate recorded | `[U8].{"…"}` is byte-mode in one pattern engine. Bit widths and endian rules are checked; retired `b"…"` spelling stays retired. |
| D-STM1 | shipped | `crates/jet-codegen/src/Prelude/Core/RuntimeControl.rs`; `crates/jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs`; `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs` | `examples/features/memory/shared_transact.jet`; `tests/dev.rs` | `#1161` | D-CONC-STM1 amends the old retry text. The body runs once; the commit takes touched locks in fixed order, applies buffered edits, and waits on contention. Rollback drops buffered edits before commit. |
| D-AUTH1 | shipped | `crates/jet-codegen/src/Prelude/CoreLib/Top/Auth.rs`; `crates/jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs`; `crates/jet-comptime/src/Comptime/AuthLite.rs` | `examples/features/crypto/auth_sessions.jet`; `examples/features/crypto/auth_tokens.jet`; `tests/taint.rs`; `tests/dev.rs` | `#1161` | Token verification and session helpers use a process-global typed store, checked expiry, single-use state, and OS cryptographic entropy. Magic links require a registered user with a syntactically valid delivery identity, and consume rechecks that identity. Durable DB-backed app routes, provider network, actual email delivery, and remote auth reconnect are unshipped. |
| D-SYNC1 | shipped | `crates/jet-codegen/src/Prelude/CoreLib/Top/Sync.rs`; `crates/jet-comptime/src/Comptime/SyncLite.rs`; `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs` | `examples/features/tooling/sync_crdt.jet`; `tests/sync_crdt.rs`; `docs/reference/core-library.md` | `#1159` verified child; `#1161` | Typed CRDT carriers/codecs carry an absorbing invalid state: merge overflow or malformed identity denies without truncation, and the ambient codec preserves that denial. `map_get` returns absence for invalid maps; `counter_value` still returns `0`, so changing its public return type requires an owner ballot. Merge keeps non-conflicting contributions: text is an atom-set union, list an add-only union, counter a per-replica max, and MAP CONFLICTS ARE LAST-WRITER-WINS — the losing value does not survive. The bounded session registry merges canonical list/counter DISPLAY STRINGS and equal-valued map entries, is idempotent for an exactly equal representation, denies conflicting map values and opaque text because display serialization carries no atom/LWW metadata, and publishes the latest receipt through the local live transport for reconnect replay. Generic `SyncMap<K,V>`/`SyncList<T>`, Jet-visible `#Codable` on the carriers, map/list/counter metadata, `JET_SYNC_SESSIONS` durability, authenticated routing, and network transport remain unshipped. |
| D-VALIDATE1 | shipped | `crates/jet-sema/src/Sema/CheckerValidate.rs`; `crates/jet-sema/src/Sema/CheckerInfer/calls/method_calls.rs`; `crates/jet-sema/src/Sema/Registration.rs`; `crates/jet-sema/src/Sema/Registration/Serde.rs`; `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs`; `crates/jet-codegen/src/Codegen/TIR/subset/methods.rs` | `examples/features/serde/validate.jet`; `examples/features/expected/serde/validate.out`; `tests/ui/validate_*`; `docs/spec/spec.md` | `#1161` | In-body `validate {}` and `Type.validate` ship. Derived struct decoders now run their synthesized validator automatically, and every typed Decode codec uses the canonical `[FieldError]` result contract. Hand-written codecs opt in explicitly. `Validate.over(s)` now starts the compiler-owned outside-context builder; chained `check(...)` rules use the same field vocabulary and accumulate into `[FieldError]`, and `finish()` returns `T [FieldError]!`. The common TIR path carries the same meaning across AOT, Cranelift, interpreter, and web lowering. |
| D-VALIDATE-DECODE1 | shipped | `crates/jet-sema/src/Sema/CheckerCoreLib/core_call.rs`; `crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingTraits.rs`; `crates/jet-codegen/src/Prelude/CoreLib/Top/DataFmt.rs`; `crates/jet-comptime/src/Comptime/TypedDecode.rs`; `crates/jet-jit/src/jit/lower_ctx.rs` | `tests/corelib_parts/derives.rs`; `examples/features/serde/decode_migration.jet`; `examples/features/serde/validate.jet` | `#1161`; D-MIGRATE3 retired by D-VALIDATE-DECODE1=B | Every typed codec and adapter returns `Result<T, [FieldError]>` (CSV returns `Result<[T], [FieldError]>`); published-schema migration is silent inside the one decode path. The retired migration-report decoder and result wrapper are absent. `AuthError::DecodeError` is a token-JSON error variant, not a codec result contract. |
| D-DBPOLICY1 | shipped | `crates/jet-codegen/src/Prelude/CoreLib/JetStd/RowPolicy.rs` (the one closed policy language); `crates/jet-codegen/src/Prelude/CoreLib/JetStd/DBPluginWire.rs` (SQL transformer); `crates/jet-codegen/src/Prelude/CoreLib/Top/Sync.rs` (`JetRowPolicy` carrier); `crates/jet-jit/src/DB.rs`; `crates/jet-jit/src/ambient_interp.rs` | `tests/db_policy.rs`; `examples/features/io/db_policy.jet`; `docs/reference/core-library.md` | `#1160` verified child; `#1161` | Bounded `DBScope` carries policy and user through query, mutation, transaction, and live paths. `RowPolicy.rs` is the single acceptance table every tier includes, and each tier parks the COMPILED policy rather than the caller's text; the accepted table shape and the expression set used to live in two places that disagreed about a leading-digit table name. The closed compiler now lowers its accepted forms to a bind-safe SQL predicate. Every scoped SQL adapter consumes a proof-bearing application and validates its user bind before execution; `db.policy_audit(scope)` exposes the active scope's compiled predicate and bound identity. The ratified general typed closure surface (`db.policy<T>((user, row) => ...)`) remains unshipped. |
| D-ENVHOOK1 | shipped | `crates/jetpack/src/EnvHook.rs`; `crates/jetpack/src/CLI/run_enter_dev.rs`; `crates/jetpack/src/CLI/trust_env_build.rs`; `crates/jet-foundation/src/Syntax/effects_surface.rs` | `tests/env_hook.rs`; `tests/env_dev_trust.rs`; `docs/reference/environment.md` | `#506` slice; env-model follow-up for declared shell variables | Opt-in bash/zsh/fish hook (`render_hook`), the D-JPK-GRANTCMD1 trust gate, activate/unload on crossing an env boundary, and the `JET_ENV_DISABLE` escape all ship; host tooling only, so no runtime tier applies. `env.jet` cannot declare arbitrary shell variables: activation variables come only from realized providers (`trust_env_build.rs:295-355` composes them from `provider_vars`), and the user's only control is the `unset` list. The ratified surface's own example activates with `PGHOST=localhost`, so this is owed work, not an unclaimed extra. |
| D-OBSERVE-LIVE1 | shipped | `crates/jet-codegen/src/Prelude/Observe.rs`; `crates/jet-codegen/src/Prelude/EnvInit.rs`; `crates/jet-jit/src/lib.rs`; `crates/jet-jit/src/jit/resident.rs`; `crates/jet-jit/src/jit/deopt.rs`; `crates/jet-devserver/src/LiveInspect.rs`; `Source/main.rs` (env setup + `inspect live` dispatch) | `tests/live_inspect.rs`; `tests/event_observations.rs`; `docs/reference/core-library.md` | `#1161`; GC/Canvas debt below | `JET_OBSERVE=1` publishes bounded PID-scoped facts. Tasks, channels, effects, and arena stats are visible; payloads, locals, environment, credentials, and arbitrary process memory are not. AOT `EnvInit`, resident Cranelift, and both interpreter entry paths call the same Prelude `jet_observe_runtime_start` entry, so `jet run --observe` reaches the shared producer on the default tier. `crates/jet-cli/src/CLI.rs` is the flag table only, not dispatch. GC stats and the Canvas task/channel/effect projection named by the ratified text are absent. |

## Observed tiers

Generated rows, not typed claims. `tests/jit_corpus_gate.txt` is regenerated
from a run; each example stem lands in exactly one class.

Read the classes exactly (`classify_corpus_stem`,
`tests/dev_parts/support.rs:4024-4120`):

- `resident_jit` proves the optimized AOT oracle ran clean, the default tiered
  run produced byte-identical normalized output, and resident Cranelift executed
  with no deopt. It does **not** prove the interpreter leg: the forced-interpreter
  comparison at `:4080-4092` only runs when the TIR evaluator accepts the
  program, and a refusal carrying E2201/E0956 is waived at `:4094-4099` without
  changing the class.
- `deopt_interp` means the default run never executed on Cranelift.
- `aot_broken` means the oracle itself failed, so **no** tier comparison ran.

| decision | example stem | class | what that licenses here |
|---|---|---|---|
| D-BINPAT1 | `parsing/binary_pattern`, `parsing/binary-reader` | resident_jit | Observed AOT, resident Cranelift, and forced interpreter runs agree. The TIR evaluator now marshals `BinMatchScan` through the shared matcher (`Codegen/TIR/eval/exprs.rs:7773-7811`). |
| D-STM1 | `memory/shared_transact` | deopt_interp | AOT and the interpreter agree; `#Transact` over `Shared` never executes on the resident Cranelift tier. `errors/transact` is resident_jit. |
| D-AUTH1 | `crypto/auth_sessions`, `crypto/auth_tokens` | resident_jit | AOT + resident Cranelift agree for the session and token examples. `verify_jwt`/`verify_paseto` have no comptime implementation at all (`Comptime/AuthLite.rs` includes `AuthSession.rs` only). |
| D-SYNC1 | `tooling/sync_crdt` | resident_jit | AOT + resident Cranelift agree. |
| D-DBPOLICY1 | `io/db_policy` | resident_jit | AOT + resident Cranelift agree; `tests/db_policy.rs` additionally asserts the AOT and default runs byte-for-byte, including the invalid-policy denials. |
| D-VALIDATE1 | `serde/validate` | resident_jit | AOT + resident Cranelift agree. |
| D-VALIDATE-DECODE1 | `serde/hand_codec` | aot_broken | Nothing. The oracle exits 1 against a golden expecting 0, so no tier ran a comparison. Held out in `AOT_BROKEN_HELD_OUT` (`tests/dev_parts/support.rs:3868-3873`) against card `#2016`, which is closed `done` — the row is orphaned. |
| D-OBSERVE-LIVE1, D-ENVHOOK1 | none | absent | No example stem exists, so the corpus gate says nothing about either. Both are host tooling paths proved by `tests/live_inspect.rs`, `tests/event_observations.rs`, `tests/env_hook.rs`, and `tests/env_dev_trust.rs`. |

## Shipped boundary notes

### D-LIVEQUERY1 scope boundary

The shipped D-LIVEQUERY1 boundary is the local rerun seam. `LiveQuery.rs` stores a typed rerunner and sink in the
shared Prelude registry. `DBScope.live` registers a policy-aware query rerun;
the AOT emitter binds its result to the canonical `JetSignal<T>`, and the
ambient/JIT adapter calls the same Prelude. Invalidation releases the registry
lock before rerunning, rejects stale generations, and sends successful values
through the existing `core.net.ws` writer. The transport replays the latest
bounded event for each topic when a connection registers. No local callback
queue, second signal, or browser-only protocol was added.

The retained extensions are general typed app/query graph discovery,
connection-scoped authentication/routing, and a client-side protocol that maps
wire events to the browser's `Signal<T>`. Those require the still-unbuilt app
graph and remote-transport substrate; they are outside this shipped boundary.

## Unshipped ratified behavior

### D-SCHEDULE1

The shipped `jet dev` path consumes `Interpreter::scheduled_jobs` and
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
- CRDT merge keeps every NON-CONFLICTING replica contribution and converges under
  each carrier's declared law: text is an atom-set union with tombstones, list an
  add-only union, counter a per-replica max. A conflicting map key is
  last-writer-wins on `(clock, writer, value)`, so the losing value does not
  survive — "preserves replica contributions" was too strong for `SyncMap`. The
  process-local session publishes a bounded latest-state event for local
  reconnect replay; its fixed String seam merges only list/counter displays and
  equal-valued map entries, and denies conflicting map values or opaque text
  because it carries no typed atom/LWW metadata or vector clocks. It is not an
  authenticated remote routing or network merge implementation.
- The live path is a bounded typed registry with explicit errors, generation
  checks, query rerun, canonical signal delivery, serialized native `core.net.ws`
  publication, and latest-topic replay on reconnect. It is not the browser
  protocol, connection-scoped authorization, general app-query graph, or
  remote authenticated reconnect implementation.

## Implementation checkpoint

Earlier passes replaced the live-query and auth thread-local registries with
process-global mutex state, bounded live-query state with oldest-entry eviction
and explicit closed-handle errors, required a registered delivery-capable
identity for magic links, made STM buffering an explicit typed guard, and
validated live-inspect JSON through the shared JSON parser. Child cards #1157 and
#1160 carry their focused evidence.

This pass (card #1161) made three facts single-homed rather than mirrored, each
because the copies had already drifted or could:

- The closed row-policy language moved to `Prelude/CoreLib/JetStd/RowPolicy.rs`
  and every tier includes it. `DBPluginWire.rs` and `Top/Sync.rs` held two copies
  that disagreed about a leading-digit table name, and the SQL transformer
  re-recognized the literal string `"true"` a third time; it now matches the
  compiled form. `tests/db_policy.rs` gained an AOT/default pair asserting the
  same verdict for a leading-digit table, a spaced table, an unsupported
  expression, and a padded-but-legal table.
- `SyncMap.get` now denies on an invalid carrier, and the comptime marshaller
  reads carrier validity through one fail-closed helper instead of four
  `unwrap_or(true)` copies.
- The session cookie's `HttpOnly`/`Secure`/`SameSite`/`Path` defaults are one
  helper in `AuthSession.rs` instead of three identical `format!` literals.

Two retired duplicate goldens were deleted:
`examples/features/crypto/auth_sessions.jet.expected_out` (which CONTRADICTED the
canonical `expected/crypto/auth_sessions.out` on the `magic_user` line) and
`examples/features/tooling/sync_crdt.jet.expected_out`. The live convention is
`examples/features/expected/<topic>/<name>.out`
(`tests/truthfulness.rs::every_feature_example_has_expected_output`).

This pass ran only scoped type-checks; focused tests and full-suite proof remain
orchestrator-owned. Every tier statement here is source reading or a
corpus-gate row, never a new observed three-leg run.
