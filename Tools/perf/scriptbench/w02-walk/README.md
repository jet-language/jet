# w02: walk

Walk `w02/` (10 x 10 x 10 directories, 20,000 files of 0..4,095 bytes), stat each file, print the file count and the byte total.

Origin: the task of the 2026-11-01 card-close bench (the cells logged on its cards), on the 20,000-file tree of the 2026-10-28 audit bench (w01-w15, the bench the scripting cards were filed from). Cards: #6086.

The card-close bench walked a 5,000-file tree, which made its w02 cells about a quarter of the audit's (11.6 ms against 113.6 ms for jet-bin). This bench keeps the audit's 20,000-file size. The audit program also grouped the bytes by top-level directory; that grouping is not part of the #6086 walk cost and is dropped. Lua has no directory listing in its standard library, so the Lua peers list the tree with `find`.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
