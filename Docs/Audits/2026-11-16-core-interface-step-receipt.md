# Core interface step receipt (2026-11-16)

The before-and-after receipt for the precompiled Core interface step, measured on the batch compilers built before it (p201) and with it (p202) by `jet Tools/perf/core-interface/run.jet -- --before JET_P201 --after JET_P202 --cpus 8-15`. It is the evidence for the step's numbers; current behavior comes from rerunning the tool.

Findings: wall and peak improved on every program; the one WORSE row (built-in summarize run user+sys, +3%) has a better wall and an improved twin arm, so it reads as noise. The load gate is not met: a loaded body costs x12 to x20 a body checked from text, which is open speed work.

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| W1 summarize run user+sys +3% | no-action | its wall improved (x0.97) and the named-tree arm of the same program improved, so the row is run-to-run noise, not a regression |
| L1 loaded body costs x12 to x20 a body checked from text | card | #6759 criterion 24: the load path reaches a tenth of a text check |
<!-- /audit-dispositions -->


Before: identity `jet@a0.0.45#971cc5a059ed680bb74df04aaaa286a87e8eea0f00d39f0455abc0b68931f185`, `jet --version`: `jet a0.0.45-dev`.

After: identity `jet@a0.0.45#89ba7affc11a03c364426a129f98c44a4f648e13872d9da296e133056cbb907e`, `jet --version`: `jet a0.0.45-dev`.

Each row ran 11 warm and 5 cold runs per jet, the jets alternating run by run, pinned to CPUs 8-15, each with an empty JET_STORE_DIR and XDG_CACHE_HOME, no JET_SEED and NO_COLOR=1; each jet first ran once cold, unrecorded. Values are medians. User and sys are this program's RUSAGE_CHILDREN change across the run; peak and faults are GNU time's %M, %F and %R. A row is worse when its wall or user plus sys rises by more than 3% and 2 ms, or its peak by more than max(1 MB, 1%). Produced by `jet Tools/perf/core-interface/run.jet`.

## Headline

- hello `jet run`: peak 36.2 → 29.9 MB (warm), 35.9 → 29.9 MB (cold).
- rcc `jet check`: wall 534.8 → 475.0 ms, peak 309.9 → 302.6 MB (warm); cold wall 656.0 → 594.5 ms.
- rcc `jet run`: wall 657.7 → 636.2 ms, peak 290.5 → 278.4 MB (warm); cold wall 802.1 → 755.6 ms.

## A/B

| Arm | Program | Command | Wall ms | User+sys ms | Peak MB | Wall after/before | Cold wall ms | Cold user+sys ms | Cold peak MB | Cold major faults | Verdict |
|---|---|---|---|---|---|---:|---|---|---|---|---|
| built-in | hello | check | 15.6 → 14.4 | 15.5 → 14.3 | 30.6 → 22.7 | x0.92 | 61.9 → 45.5 | 23.5 → 20.7 | 29.7 → 22.6 | 176 → 110 | ok |
| built-in | hello | run | 17.7 → 16.8 | 17.6 → 16.9 | 36.2 → 29.9 | x0.95 | 76.9 → 54.9 | 28.9 → 24.3 | 35.9 → 29.9 | 226 → 141 | ok |
| built-in | abs | check | 36.7 → 36.2 | 42.7 → 42.3 | 49.4 → 41.5 | x0.99 | 100.8 → 88.1 | 56.1 → 52.3 | 49.6 → 40.6 | 245 → 185 | ok |
| built-in | abs | run | 44.1 → 43.5 | 52.6 → 51.7 | 63.3 → 47.9 | x0.99 | 121.1 → 100.4 | 79.0 → 67.0 | 63.1 → 47.1 | 311 → 217 | ok |
| built-in | fread | check | 102.0 → 99.4 | 186.9 → 184.1 | 95.2 → 88.6 | x0.98 | 178.0 → 172.8 | 206.5 → 204.8 | 94.2 → 88.6 | 276 → 236 | ok |
| built-in | fread | run | 132.2 → 127.1 | 236.2 → 234.6 | 106.4 → 100.1 | x0.96 | 254.4 → 232.5 | 308.8 → 303.8 | 106.5 → 100.2 | 364 → 286 | ok |
| built-in | summarize | check | 132.4 → 127.9 | 251.6 → 247.0 | 109.0 → 103.6 | x0.97 | 230.1 → 213.7 | 276.3 → 270.7 | 110.4 → 103.4 | 358 → 305 | ok |
| built-in | summarize | run | 197.6 → 191.2 | 385.6 → 398.0 | 127.2 → 119.1 | x0.97 | 320.6 → 304.2 | 462.0 → 457.9 | 126.1 → 118.5 | 463 → 384 | WORSE (user+sys) |
| built-in | rcc | check | 534.8 → 475.0 | 1181.6 → 1183.0 | 309.9 → 302.6 | x0.89 | 656.0 → 594.5 | 1208.7 → 1226.3 | 307.0 → 301.0 | 457 → 384 | ok |
| built-in | rcc | run | 657.7 → 636.2 | 1511.1 → 1515.4 | 290.5 → 278.4 | x0.97 | 802.1 → 755.6 | 1575.7 → 1571.7 | 287.8 → 277.5 | 524 → 433 | ok |
| named tree | hello | check | 21.0 → 20.5 | 20.9 → 20.4 | 46.8 → 40.5 | x0.98 | 69.3 → 58.0 | 31.0 → 28.3 | 46.2 → 39.7 | 172 → 119 | ok |
| named tree | hello | run | 26.4 → 24.8 | 26.4 → 24.5 | 52.6 → 47.0 | x0.94 | 85.6 → 69.0 | 36.5 → 33.4 | 52.2 → 46.5 | 225 → 150 | ok |
| named tree | abs | check | 41.9 → 42.6 | 47.9 → 48.8 | 65.1 → 57.2 | x1.02 | 109.8 → 98.6 | 64.9 → 60.4 | 64.3 → 56.3 | 245 → 189 | ok |
| named tree | abs | run | 51.3 → 51.4 | 60.4 → 59.8 | 77.4 → 63.3 | x1.00 | 130.0 → 113.2 | 88.2 → 76.9 | 77.7 → 62.4 | 310 → 223 | ok |
| named tree | fread | check | 110.4 → 110.3 | 198.4 → 197.2 | 107.3 → 103.0 | x1.00 | 187.4 → 184.0 | 220.2 → 215.3 | 106.3 → 101.9 | 264 → 232 | ok |
| named tree | fread | run | 141.0 → 136.9 | 247.2 → 245.1 | 119.7 → 113.5 | x0.97 | 233.2 → 228.0 | 292.4 → 302.9 | 119.0 → 111.7 | 355 → 283 | ok |
| named tree | summarize | check | 136.8 → 133.1 | 259.2 → 256.5 | 122.4 → 117.0 | x0.97 | 233.3 → 222.8 | 281.3 → 277.7 | 123.0 → 117.1 | 343 → 299 | ok |
| named tree | summarize | run | 196.2 → 192.4 | 371.5 → 368.9 | 139.3 → 131.1 | x0.98 | 316.4 → 294.7 | 445.6 → 451.0 | 137.7 → 131.0 | 471 → 377 | ok |
| named tree | rcc | check | 528.1 → 472.9 | 1161.3 → 1171.5 | 320.2 → 314.9 | x0.90 | 657.0 → 592.3 | 1192.5 → 1207.9 | 318.8 → 315.1 | 438 → 372 | ok |
| named tree | rcc | run | 660.1 → 645.1 | 1520.9 → 1546.2 | 298.6 → 290.7 | x0.98 | 799.3 → 769.0 | 1627.2 → 1614.8 | 298.3 → 289.1 | 516 → 420 | ok |

## hello: where its cold user CPU goes

- Before: untraced cold user CPU 9.24 ms; the user code's traced CPU 0.35 ms, 3.8% of it (not more than half).
- After: untraced cold user CPU 11.31 ms; the user code's traced CPU 0.34 ms, 3.0% of it (not more than half).
- Target: the user code's CPU is about 0.55 ms, so more than half of hello's user CPU is the user's when the rest falls under that. Each phase's median traced CPU (the user code's parse and sema.module spans included in theirs):

| Phase | Before ms | After ms | Change ms | After over the target ms |
|---|---:|---:|---:|---:|
| sources | 0.84 | 0.84 | 0.01 | 0.29 |
| load | 1.06 | 1.11 | 0.05 | 0.56 |
| register | 2.94 | 2.96 | 0.03 | 2.41 |
| sema.check | 1.06 | 1.07 | 0.02 | 0.52 |
| comptime | 0.37 | 0.28 | -0.08 | 0 |
| sema.finalize | 2.95 | 2.83 | -0.13 | 2.28 |
| comptime.bodies | 0.06 | 0.06 | -0.00 | 0 |
| comptime.validate | 0.03 | 0.04 | 0.00 | 0 |
| driver.prelower.merge | 0.03 | 0.04 | 0.01 | 0 |
| driver.prelower.owners | 0.07 | 0.08 | 0.01 | 0 |
| driver.prelower.registration | 0.05 | 0.06 | 0.02 | 0 |
| lower | 0.05 | 0.07 | 0.02 | 0 |
| driver.preassemble.entry | 0.02 | 0.02 | 0.00 | 0 |
| driver.result | 0.05 | 0.06 | 0.00 | 0 |
| driver.release | 0.11 | 0.10 | -0.01 | 0 |
| cli.diagnostics | 0.02 | 0.02 | 0.00 | 0 |
| cli.authority | 0.02 | 0.02 | -0.00 | 0 |
| native.sources | 0.03 | 0.03 | 0.00 | 0 |
| native.rust_foreign | 0.03 | 0.03 | -0.00 | 0 |
| ir.build | 0.65 | 0.64 | -0.01 | 0.09 |
| ir.opt | 0.08 | 0.08 | 0.00 | 0 |
| ir.lint | 0.26 | 0.25 | -0.01 | 0 |
| ir.x64 | 0.12 | 0.13 | 0.00 | 0 |
| ir.object | 0.13 | 0.11 | -0.02 | 0 |
| native.foreign | 0.02 | 0.02 | 0.00 | 0 |
| native.link | 0.16 | 0.16 | 0.00 | 0 |
| all phases | 11.20 | 11.11 | -0.10 | 10.56 |

## Per-body load against check

From one run per jet with `--trace-detail=functions` (the core.timed counters, task CPU). A load gate holds when a loaded body costs at most a tenth of a body checked from text. `-` is a counter the jet does not trace.

| Program | Command | Jet | Loaded (timed) | µs per load | From text (timed) | µs per check | Load/check | Bodies loaded | Bodies file read MiB | From text | Kept bodies |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| hello | check | before | - | - | - | - | - | 2 | 0.0 | 0 | - |
| hello | check | after | 0 | - | 0 | - | - | 2 | 0.0 | 0 | - |
| hello | run | before | - | - | - | - | - | 2 | 0.0 | 0 | - |
| hello | run | after | 0 | - | 0 | - | - | 2 | 0.0 | 0 | - |
| abs | check | before | - | - | - | - | - | 65 | 0.2 | 0 | - |
| abs | check | after | 35 | 28 | 0 | - | - | 65 | 0.2 | 0 | - |
| abs | run | before | - | - | - | - | - | 76 | 0.2 | 0 | 3 |
| abs | run | after | 3 | 35 | 0 | - | - | 76 | 0.2 | 0 | 3 |
| fread | check | before | - | - | - | - | - | 316 | 1.0 | 0 | - |
| fread | check | after | 172 | 36 | 0 | - | - | 316 | 1.0 | 0 | - |
| fread | run | before | - | - | - | - | - | 324 | 1.3 | 0 | 79 |
| fread | run | after | 61 | 81 | 0 | - | - | 324 | 1.3 | 0 | 79 |
| summarize | check | before | - | - | - | - | - | 413 | 1.5 | 0 | - |
| summarize | check | after | 255 | 40 | 0 | - | - | 413 | 1.5 | 0 | - |
| summarize | run | before | - | - | - | - | - | 343 | 1.4 | 2 | 85 |
| summarize | run | after | 65 | 82 | 2 | 4 | x20.50 | 343 | 1.4 | 2 | 85 |
| rcc | check | before | - | - | - | - | - | 2355 | 12.2 | 8 | - |
| rcc | check | after | 2082 | 57 | 8 | 4 | x14.25 | 2355 | 12.2 | 8 | - |
| rcc | run | before | - | - | - | - | - | 988 | 6.6 | 83 | 561 |
| rcc | run | after | 607 | 85 | 80 | 7 | x12.14 | 988 | 6.6 | 83 | 561 |
