# #3078 c6 evidence map: shared operations keep each domain's rules (2026-11-13)

Criterion c6 on card #3078 reads: "Shared task/deadline/channel/failure/
marshalling operations do not erase domain admission, close, cancellation,
durability, retry or authority." D-ONE-LIFECYCLE1=A shares the operations,
not the policies. A task, a `#Deadline`, a channel or a TaskFailure crossing a
join must reach a domain's code as a value. The domain then applies its own
rule. This map names, for each rule and domain, the test that proves the rule
survives the shared operation.

Every entry is a `jet test Compiler --profile cases --filter <path>` case,
except the two `tests/run` programs, which run under `--profile run`.

## HTTP server

| Rule | Shared operation | Proof |
|---|---|---|
| Admission | a worker's `#Deadline` and a worker's panic inside a handler | `http_server_lifecycle/a_handler_task_failure_is_its_answer_and_admission_goes_on`: the handler answers 504 and 500, the next request answers 200, and shutdown reports 4 completed and 0 cancelled. |
| Admission | a delivered signal cancels every task | `process_signal_drain/sigterm_answers_the_request_in_flight`: the accept loop stops, and the request in flight is still answered. |
| Close | task-per-connection drain under a grace period | `http_server_lifecycle/shutdown_grace_cancels_straggler_socket_and_returns_bounded_report`: a straggler is counted cancelled within the grace. |
| Cancellation | a handler blocked past the grace | golden `Network/AppLifecycle` (#3078 c8): the admission line ends `cancelled 1`, and the line `the cancelled handler's response was sent: false` shows its response is not sent. |

## Database

| Rule | Shared operation | Proof |
|---|---|---|
| Admission | a task `#Deadline` shorter than the pool's own acquire deadline | `db_policy/task_deadline_and_cancel_keep_pool_admission_and_transaction_outcome`, lines 1-3: the waiter fails with the task's deadline, the pool counts no timeout, and the next acquire is admitted. |
| Failure | a pool refusal crossing `join` | the same case, line 4: the joined value is the pool's own text, `database pool acquisition deadline exceeded`. |
| Durability | a task cancelled inside an open `#SingleUse` transaction | the same case, lines 6-8: nothing is written, and the connection begins a new transaction. This held only once `DBTransaction` gained a `Close` (`Core/Db/Db.jet`): the cancel unwinds through the transaction, and its `close` rolls back a transaction still open. Before that, the cancelled writer's row stayed and the next `begin` failed with a transaction already open. |
| Cancellation | a waiter task cancelled in the pool's queue | `tests/run/CoreIO/DBContracts.jet`: the cancelled waiter leaves the queue, and the next acquire is admitted. |
| Close | drain while a lease is out | golden `Network/AppLifecycle`: `pool drain within 20ms refused: 1 lease still out` while the handler holds its lease, then the `db.pool lifecycle=drained` receipt with `leased=0` once it is returned. |

## Jobs and services

| Rule | Shared operation | Proof |
|---|---|---|
| Retry | a worker's task `#Deadline` | `services_runtime/task_deadline_and_cancel_keep_job_lease_and_retry`, lines 1-4: the job retries after its 1s backoff, as attempt 2. |
| Authority | a stale claim after the retry | the same case, line 5: the first claim's lease is refused. |
| Cancellation | a worker task cancelled mid-job | the same case, lines 6-8: the job stays Running under its lease, `recover_expired` leaves the live lease alone, and the lease holder completes the job. |
| Authority | the queue's endpoint file | `services_runtime/job_queue_refuses_a_shared_authority_file`. |
| Durability | redelivery | `services_runtime/durable_retry_is_the_only_explicit_redelivery_default_run`, plus its `_aot` and `_interpreter` twins. |
| Cancellation | a cancelled delivery | `services_runtime/service_authority_replays_cancelled_delivery_without_requeue_or_pin`. |
| Admission | a send during drain | `services_runtime/tree_durable_send_during_drain_is_rejected_before_admission_default_run`, plus its `_aot` and `_interpreter` twins. |
| Marshalling | a typed payload through the durable queue | `services_runtime/durable_lifecycle_matrix_covers_all_delivery_scenarios_on_all_tiers`. |

## Channels

A channel's close and capacity are their own rules. Both pool cases above hand
their start signal through a bounded channel. No case in this map shows a
channel erasing a domain rule, and none was needed: a channel carries values
and never a domain's policy.
