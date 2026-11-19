# Debugger latency: `jet debug` against LLDB on the same image

The debugging and profiling review (2026-11-14-debugging-profiling-review.md, M7 and gap G9) found that no Jet test measures the time to the first stop or the time per step on a large program. This audit measures both, for `jet debug` and for LLDB, on one O0 image.

## Method

`Tools/Perf/DebugLatency/Main.jet` does the whole measurement. It is run from the repository root:

```
jet run Tools/Perf/DebugLatency/Main.jet -- --runs 5
```

It writes a package to a temporary directory. The program has 2000 small functions of four lines each, and `run` calls each function in turn, one call per line. It builds the program's O0 image once with `jet debug`'s own build, `jet build -O0 --output-dir=out prog.jet`. That first build also fills the build cache. Both debuggers then drive that one image, `out/prog`, and the tool prints the image's SHA-256. Every command runs in the work directory with its output captured. Each figure is the median of 5 runs, in milliseconds:

- **build**: the same build again, warm.
- **jet_debug first_stop**: `jet debug --break prog.jet:L out/prog` with an empty stdin. L is the line of the first call in `run`. The time runs from the start to the exit. With no command on stdin, the console continues from the stop to the program's end.
- **jet_debug per_step**: the same with 20 `n` lines on stdin. The figure is the extra time over the first stop, divided by 20.
- **lldb first_stop**: `lldb -b -o 'breakpoint set --file prog.jet --line L' -o run -o continue out/prog`.
- **lldb per_step**: the same with 20 `-o next` before `continue`. The figure is the extra time over the first stop, divided by 20.

Both first-stop figures include the run from the stop to the program's end, and that run is the same for both debuggers. Per step subtracts it out. A debugger run counts only when its output reports the stop: `Breakpoint hit  prog.jet:L` from `jet debug`, `stop reason = breakpoint` from LLDB. Any other run fails the measurement.

The tool prints the image hash, both tools' versions and the exact commands as `#` lines, followed by the table below. LLDB is the comparator only. When no `lldb` is on PATH and `--lldb` is not given, the tool times `jet debug` alone and prints `-` in LLDB's column, so the measurement itself needs nothing but jet (#6841 c34).

## The first-stop bar (#6841 R11)

On a prebuilt image of a 100,000-line program, `jet debug` reaches its first stop in under 200 ms. `--bar MS` makes the tool fail after printing the rows when the median first stop took more than MS milliseconds, and it prints the verdict as `# bar: jet_debug first_stop at most MS ms: met` or `missed`. With 20000 functions the program has 120,004 lines: 100,000 in the functions and 20,004 in `run`. The bar is measured with:

```
jet run Tools/Perf/DebugLatency/Main.jet -- --functions 20000 --runs 5 --bar 200
```

The first-stop figure still includes the run from the stop to the program's end, which here is 20000 cheap calls, so the bar is conservative.

## Result

Measured 2026-11-18 on an AMD Ryzen 9 7950X3D (16 cores, 32 threads, Linux x86-64), with other agent jobs running beside it. `jet` is the a0.0.45 release candidate built at dev 3a012d3674 (stage-2 jet 7667b43e13b5a9f5); `lldb` is LLDB 21.1.8.

Run 1, the default size: `jet run Tools/Perf/DebugLatency/Main.jet -- --runs 5` (2000 functions, 12,004 lines, 20 steps, median of 5 runs).

| | |
|---|---|
| image `out/prog` sha256 | b914b020b4a1a8d733060bfab7800c9e06e0c2fa83a24a200a4a89bcb8978b6a |
| jet | a0.0.45-dev at 3a012d3674 |
| lldb | 21.1.8 |

| measure | jet_debug (ms) | lldb (ms) |
|---|---|---|
| build | 468 | - |
| first_stop | 7 | 150 |
| per_step | 76 | 0 |

Run 2, the R11 bar: `jet run Tools/Perf/DebugLatency/Main.jet -- --functions 20000 --runs 5 --bar 200` (20,000 functions, 120,004 lines, 20 steps, median of 5 runs). The tool printed `bar: jet_debug first_stop at most 200 ms: met`.

| | |
|---|---|
| image `out/prog` sha256 | 072a06c2c31e13d7e89edf0ada462006a995b597f0db2a9608bbe383af48b9c5 |

| measure | jet_debug (ms) | lldb (ms) |
|---|---|---|
| build | 19754 | - |
| first_stop | 55 | 206 |
| per_step | 1339 | 0 |

## Reading

`jet debug` reaches its first stop well ahead of LLDB on both images: 7 ms against 150 ms, and 55 ms against 206 ms on the 120,004-line image, a quarter of the 200 ms bar. LLDB pays for loading the image's DWARF up front.

Stepping is the opposite. LLDB's `next` costs under a millisecond on both images (the tool prints whole milliseconds, so 0). `jet debug`'s `n` costs 76 ms on the small image and 1,339 ms on the large one: its cost grows with the image, about 11 ms per 1,000 lines, so each step redoes work proportional to the whole program instead of to the step. That is finding F2.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1: no Jet test measures the time to the first stop or the time per step on a large program | no-action | measured by Tools/Perf/DebugLatency/Main.jet, the tool this report runs |
| F2: `jet debug` per-step time grows with the image (76 ms at 12,004 lines, 1,339 ms at 120,004) while LLDB stays under 1 ms | card | #6841 c59 |
<!-- /audit-dispositions -->
