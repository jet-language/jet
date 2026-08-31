# User-story suites

`examples/features/` remains the feature-by-feature language reference. This
directory adds complete small programs that start with a user story, model
data, perform a useful operation, and print a result.

Every program has a matching file in `expected/`. The golden test runs each
program through `jet run` and compares stdout byte-for-byte.

| Program | User story | End-to-end result |
| --- | --- | --- |
| `expense_report.jet` | A shop owner wants a daily total from line items. | Counts items and totals their cents. |
| `support_queue.jet` | A support lead wants the open tickets that need attention. | Filters open tickets and prints their priority. |
| `inventory_reorder.jet` | A stock clerk wants a quick reorder list. | Compares stock against reorder points. |
| `dispatch.jet` | A parser routes aliases and field keys. | Uses one ordered dispatch table. |
| `failure.jet` | A command reports typed and implicit failures. | Propagates and converts one failure rail. |
| `finite_state.jet` | A build job follows legal state transitions. | Uses enum variant groups, tags, and typestate. |
| `ownership.jet` | A report reuses views and copies at one boundary. | Makes ownership and materialization visible. |
| `wire_output.jet` | A command emits and reads a typed JSON record. | Writes canonical bytes and round-trips `#Codable`. |

Run any suite directly with `jet run examples/suites/<name>.jet`.
