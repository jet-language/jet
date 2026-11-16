# w04: wordfreq

Count the lower-cased `[a-z]+` words of `data/words.txt` (about 25 MB of prose over a Zipf vocabulary of 5,000 words); print the total, the distinct count and the ten most frequent words (ties broken by word).

Origin: the 2026-10-28 audit bench (w01-w15, the bench the scripting cards were filed from). Cards: #5072 c7.

Gen.jet uses integer Zipf weights in place of the audit's floating-point ones; the size and vocabulary are the same.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
