# #6497 held patch (FFIDesign-1)

| Item | File | Base | State | Next step |
|---|---|---|---|---|
| FFIDesign-1 | `FFIDesign-1.patch`, `FFIDesign-1.msg`, `FFIDesign-1-cards.tsv`, `REPORT-run1.md` (wave 22 run 1 on 69a987b5a6) | 45ab3b5ef5 | Pulled from wave 22. #CLI without commands gets a parser written for its fields. collatz goes from 294480 to 207672 B. Its help/version path calls `process.exit(0)`, which gives #CLI programs the Exec effect, and 5 goldens and 12 cases then fail with E1803 (see REPORT-run1.md). On 348c3649f1 a 3-way apply conflicts in CliEntry.jet, Prelude/Core.jet, Core/Args/Args.jet and the four generated tables | Rework is owned by IrTypes-3 (FFIDesign-2): help/version must not need Exec. Rebase the three hand-written files, then regenerate the tables (`node Tools/agent/gen-core-tables.mjs --write`) |
