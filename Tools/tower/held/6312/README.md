# #6312 held tests (D-CACHE-CONTROL1=A)

MemDesign's `cache-control1-wip.patch` was lost in the 2026-11-04 scratch
cleanup (see `../README.md`). Its compiler half is gone. Three of its test
files survived, in a lane checkout. `cache-control1-tests.patch` adds them
at their repository paths:

- `Examples/features/memory/cache_line_padding.jet` (22 lines);
- `Examples/features/memory/stream_store.jet` (41 lines);
- `tests/conformance/corpus/core/mem/stream_store.jet` (11 lines).

| Item | File | Base | Applies to a96f0867f1 | State | Next step |
|---|---|---|---|---|---|
| Tests | `cache-control1-tests.patch` | a96f0867f1 (new files only) | Yes | The examples and the conformance test have no golden output. They do not compile on dev: `mem.stream_store` and `$build.cpu.cache_line` do not exist | Rebuild the compiler half that MemDesign's HELD.md row describes: `mem.stream_store(&dst, value)` (x64 `movnti` encoder, IR StreamStore, registry and Core rows, Core/Mem wrapper) and `$build.cpu.cache_line` (64, also accepted in `#Layout(c, align(…))`). Then add golden output on o0/o1/o2 and land with these tests (#6312 c5) |
