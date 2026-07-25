# Script-speed fixtures (#741)

Matched agent-shaped tasks for cold/warm Jet vs Bash, Python, and Node.

| Task | Jet | Peers |
|------|-----|-------|
| no-op | `noop.jet` | `bash -c true`, `python -c pass`, `node -e ""` |
| hello | `hello.jet` | print one line |
| file | `file_read.jet` + `data.txt` | `cat data.txt` |
| JSON text | `json_text.jet` | write then read a small JSON file |
| subprocess | `subprocess.jet` | spawn `true` / exit 0 |

## Method

1. Record machine facts (OS, CPU, `jet --version`, cache dir).
2. For each task: one cold `jet run`, then five warm runs. Use the median and
   the max (tail).
3. Run the peer commands the same way (five samples, median + max).
4. Separate spans when tracing: env entry, compiler start, load, parse, check,
   TIR lower, tier-1 compile, artifact load, program time (`JET_RUN_TRACE=1`).

Warm reuse stores a tier-1 Cranelift module under `JET_RUN_CACHE_DIR` (or
`~/.cache/jet/run`). A hit skips load, parse, check, lower, and codegen.
Programs that stay on whole-program interpreter deopt (some HostCall APIs)
still re-check today; stdout and exit stay equal to a cold run. Keying uses
WatchService path stamps, so source and dependency edits miss the cache.

## Budget

`D-SCRIPT-BUDGET1=B`: when `target/release/jet` (or `JET_BUDGET_BIN`) is present,
CI fails if the warm Jet no-op median is more than twice the fastest available
peer median (Bash / Python / Node) on that host. Timing uses microsecond
process-spawn medians (null stdio, one discard + seven samples). Peers still
log for file and subprocess fixtures. Without a release binary, the test keeps
a 100 ms debug sanity floor and logs that the hard B gate needs release jet.

