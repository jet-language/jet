# Compiler speed levers mapped to Jet (2026-10)

Scope: frontend speed levers ranked in the speed plan, with the measurements recorded there. Figures come from the source plan and were not re-measured here.

Ranked levers (source order):

1. Native backend: emit is 39% of frontend instructions.
2. Interned IDs: IDs were decimal text used as map keys.
3. Arenas: allocator cost about 16%.
4. Stop copying: memmove 6.4%.
5. Lazy loading.
6. No-serial-tail parallelism.
7. Cache early cutoff.
8. FNV replaced by word hashing.
9. SIMD in the lexer.
10. Container audit.
11. Reachability pruning, items 12a–f: module loading is file-granular (see reachability-census-2026-10.md).

Recorded results: cand3 −20.3% instructions; sema.check regressions at 2.13×; allocator change −12.6%; soundness bug #4911 (multithreading) noted as open.

Source: research/JET-SPEED-PLAN.md (mining shard MineTop). Status fields in the source were stripped.
