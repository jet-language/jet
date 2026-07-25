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

CI keeps a provisional warm no-op sanity gate (<100 ms process or in-process).
The peer-parity number is owner-gated as `D-SCRIPT-BUDGET1` and is enforced only
after that ruling.
