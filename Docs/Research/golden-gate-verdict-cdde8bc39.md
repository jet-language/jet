# Golden gate verdict at cdde8bc39 (2026-10)

Scope: pass/fail verdict of the golden gate at dev commit cdde8bc39.

- 970 stems: 778 pass; 192 fail.
- 0 new failures against the master list.
- 361 failures are present on the master baseline.
- Golden unit: 17 ok, 1 failed.

Isolated stems (13 stems that need isolated runs, including io/db_*, concurrency/select_generic, lowlevel/inline_c, safety/checked_text_sql): 10 former timeouts now close in 23–77 s when run alone (recorded in HANDOFF.md#199A).

Sources: Gate2Pre3/golden-verdict.txt; Gate2Pre3/isolated-stems.txt.
