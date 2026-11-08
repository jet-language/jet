# w03: grep

Scan `data/log100m.txt` (about 100 MB of log lines) for `status=5xx ms=N`, count the matches and total N.

Origin: the 2026-10-28 audit bench (w01-w15, the bench the scripting cards were filed from). Cards: #5072 c7.

The inputs come from Gen.jet's own generator, so the match counts differ from the audit's, but the size and line shape are the same. The bash peer is the grep and awk pipeline the audit used.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
