# jetc versus the Rust reference
Owner rule: jetc/Rust **<= 1**; `SLOWER` means the rule is not met.
`python3 Tools/perf/jetc-vs-rust/bench.py` runs hello and 256 functions from the existing scaling generator.
`--all` adds 512 functions; `empty` measures the startup floor; `murmur3` is a separate Example compatibility probe.
Each compiler reads the same private source snapshot; projects retain their original package manifests.
Use `--jetc ~/.cache/jet-dev/loop/jetc0 --jetc ~/.cache/jet-dev/loop7/jetc0` to compare builds.
Without `--jetc`, executable loop7 wins, otherwise loop is used.
The reference defaults to `~/.cache/jet-dev/scratch/jet-release-night12/jet` (`--rust` overrides).
Rust legs are `jet check ENTRY` and **`jet emit --rust ENTRY`** (check + lower + optimize + Rust emission).
`jet build --emit-rust` is retired; no Cargo, linking, program execution, or self-compile is timed.
jetc uses stage1/lib.sh's runner/aot environment protocol, clean environment, private store, and 1 GiB stack.
Its laneB-style `jetwork.slice` scope caps memory at **14G**, swap at zero; Rust uses `laneB.sh 8`.
GNU time runs inside the scope, excluding lane acquisition; wall seconds and maximum process RSS are retained.
Every leg is a fresh process, including jetc's compiler-image startup; no batching or warmup hides that cost.
The primary ratio uses Rust **emit**; the check-only ratio is a separate, non-equivalent lower bound.
Runner receipts must be complete, have nonempty source, and contain no E-code report before ratios are valid.
Outputs: `results.json`, input SHA-256 provenance, command lines, stdout/stderr, receipts, and `time.txt`.
Default outputs live under `~/.cache/jet-dev/scratch/sol/SolJetcBench/results/`; use `--out DIR` to retain a run.
For coordinated slots, run `--phase rust --out DIR`, then `--phase jetc --out DIR`; `--phase report` prints saved data.
Existing legs are never overwritten; use a fresh output directory to remeasure. Invocations serialize on a lock.
Package investigation: runner accepts one `.jet` entry under one authorized root; JetLexer needs sibling Foundation.
Foundation's `Source/Collections.jet` probe: Rust check timed out at 300s; emit failed E0956 (prep/MIR) and E0101 (no run).
Example probes diverged: time-foundation (33 errors/19 warnings) and Murmur3 (`test` item unsupported); generated programs give valid ratios.
Observed loop/loop7 ratios versus Rust emit: hello **59.27x/54.55x**, N256 **164.75x/145.83x**, N512 **305.89x/291.95x**.
Empty floors were 8.02s/6.08s; negative hello-minus-empty differences show noise, not a separately measured image-restore duration.
