# w14: numeric

Sum i * i % 7919 for i in 1..=30,000,000.

Origin: the 2026-10-28 audit bench (w01-w15, the bench the scripting cards were filed from). Cards: #6087.

The card's cells (jet-bin 74.1 ms against 135.7 ms) are this numeric loop. It is not a calendar workload. The bash peer runs the loop in bash arithmetic and overruns the timeout, which the comparator reports as a lower bound.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
