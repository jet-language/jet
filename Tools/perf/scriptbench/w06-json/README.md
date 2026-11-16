# w06: json

Decode `w06.json` (400,000 records in one 44 MB array) into typed records and total the salaries of the active ones.

Origin: the 2026-11-01 card-close bench (the cells logged on its cards). Cards: #6082, #6087.

The audit bench used a 50 MB file with nested records and printed a projection; the cards cite the 44 MB card-close file, which this bench reproduces exactly. Plain Lua has no JSON decoder, so Lua is not a peer here.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
