# w05: csv

Parse `w05.csv` (1,000,000 rows) and total qty and qty * price_cents. The Jet program streams the file through `csv.reader`, one record at a time, as the Perl, PHP, Python, Ruby and Lua peers read it line by line; `csv.rows` would hold every row at once.

Origin: the 2026-11-01 card-close bench (the cells logged on its cards). Cards: #6083, #6087.

The card's sizes and expected totals are reproduced exactly.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
