# #6414 held pass (OptAccumulate)

Codegen-1 wrote `OptAccumulate.jet`, an O2 pass. It turns a recursive
`g(..) + x` tail into a loop with an accumulator. The file was recovered on
2026-11-04 from the lane's rescued directory. `OptAccumulate.patch` adds it
as `Compiler/JetIR/Source/OptAccumulate.jet` (220 lines).

| Item | File | Base | Applies to a96f0867f1 | State | Next step |
|---|---|---|---|---|---|
| OptAccumulate | `OptAccumulate.patch` | a96f0867f1 (new file only) | Yes | Not wired in: nothing calls `ir_opt_accumulate`. It has no tests and no measurements. It recognizes only Int `Add` in one tail shape | Rework it into the general associative-operation tail-accumulation pass, wire it into O1/O2, add a `#Test` per shape, and measure it against the plain recursion in the same run (#6414 c37) |
