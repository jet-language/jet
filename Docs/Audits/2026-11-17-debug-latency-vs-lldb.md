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

The tool prints the image hash, both tools' versions and the exact commands as `#` lines, followed by the table below.

## Result

Machine: (to fill)

Command: `jet run Tools/Perf/DebugLatency/Main.jet -- --runs 5`

| | |
|---|---|
| image `out/prog` sha256 | (to fill) |
| jet | (to fill) |
| lldb | (to fill) |
| dev commit | (to fill) |

| measure | jet_debug (ms) | lldb (ms) |
|---|---|---|
| build | (to fill) | - |
| first_stop | (to fill) | (to fill) |
| per_step | (to fill) | (to fill) |

## Reading

(to fill once the rows are measured)
