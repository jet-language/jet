# w12: strcat

Build one string from 2,000,000 appends of `abc,`.

Origin: the 2026-11-01 card-close bench (the cells logged on its cards). Cards: #6080.

Lua builds the string with `table.concat`, its idiomatic form, because `..` in a loop is quadratic.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
