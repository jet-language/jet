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

Run one directly with `jet run examples/suites/expense_report.jet`.
