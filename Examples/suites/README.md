# Example suites

The files in this directory are small end-to-end suite fixtures. Each fixture
is a top-level `.jet` source file with a same-stem `.out` file under
[`expected/`](expected/) containing its intended stdout:

| Source | Output fixture |
| --- | --- |
| [`expense_report.jet`](expense_report.jet) | [`expense_report.out`](expected/expense_report.out) |
| [`support_queue.jet`](support_queue.jet) | [`support_queue.out`](expected/support_queue.out) |
| [`inventory_reorder.jet`](inventory_reorder.jet) | [`inventory_reorder.out`](expected/inventory_reorder.out) |
| [`dispatch.jet`](dispatch.jet) | [`dispatch.out`](expected/dispatch.out) |
| [`failure.jet`](failure.jet) | [`failure.out`](expected/failure.out) |
| [`finite_state.jet`](finite_state.jet) | [`finite_state.out`](expected/finite_state.out) |
| [`ownership.jet`](ownership.jet) | [`ownership.out`](expected/ownership.out) |
| [`wire_output.jet`](wire_output.jet) | [`wire_output.out`](expected/wire_output.out) |

## Run one fixture

From the repository root, exercise an individual source with the checked-out
compiler:

```sh
target/debug/jet run Examples/suites/expense_report.jet
```

The suite checker in [`tests/suite_goldens.rs`](../../tests/suite_goldens.rs)
defines the accepted layout: it enumerates the top-level `.jet` files (apart
from `package.jet`), requires the matching `.out`, runs each source with
`jet run`, and compares stdout byte for byte. The table above mirrors that
source-level contract; it does not replace the checker.

A fixture can be useful as a design example before it is accepted by the
current compiler. If a direct run reports a source or library diagnostic,
fix the source and its fixture together rather than treating this README or an
old output file as proof of compiler support. The feature examples in
[`../features/`](../features/) are the shorter current-syntax starting point.
