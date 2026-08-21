# Footprint receipt

This receipt records the honest competitive axis for the two small native
programs in card #2142. It makes no throughput claim.

The initial same-box capture was made on 2026-08-21 on an AMD Ryzen 9 7950X3D
with Linux 7.0.11-cachyos, `powersave`, and rustc 1.97.1
(`x86_64-unknown-linux-gnu`). It used twenty fresh native child processes per
statistical metric. The table is the seed receipt for the pinned budgets; the
accepted baseline is the report/object pair written by the update command below.

| Program / source | Profile / target | BinarySize measured / budget | StartupTime p50 / p95 / p99 / budget | MemoryHighWater p50 / p95 / p99 / budget | Capture baseline |
| --- | --- | ---: | ---: | ---: | --- |
| hello / [source](../../examples/performance/receipts/hello/src/run.jet) | `dev` / `x86_64-unknown-linux-gnu` | 509,096 B / 1 MiB | 2.657 / 2.841 / 3.031 ms / 50 ms | 2,359,296 / 2,420,736 / 2,445,312 B / 8 MiB | `card-2142/linux-x86-64-dev` |
| http-ready / [source](../../examples/performance/receipts/http_ready/src/run.jet) | `dev` / `x86_64-unknown-linux-gnu` | 738,528 B / 1 MiB | 2.602 / 3.174 / 3.181 ms / 50 ms | 2,768,896 / 2,854,912 / 2,854,912 B / 8 MiB | `card-2142/linux-x86-64-dev` |

The provider measures startup from child-process spawn to the first stdout
line. The HTTP program prints `ready` only after binding its loopback TCP
listener; the line is therefore its declared readiness event. The provider
measures memory with Linux `wait4` `ru_maxrss` (converted from KiB to bytes),
and uses `/proc/<pid>/status` `VmHWM` while the child is alive. Binary size is
the selected artifact length.

The canonical check and baseline update are:

```text
cd examples/performance/receipts/hello
TMPDIR=$HOME/.cache/jet-test-scratch ../../../../scripts/agent/jet-env full jet budget check --json
TMPDIR=$HOME/.cache/jet-test-scratch ../../../../scripts/agent/jet-env full jet budget update --baseline card-2142/linux-x86-64-dev --bootstrap --reason "card 2142 initial same-box receipt" --yes --json
```

Run the same commands from `http_ready` for the second program. `jet budget` writes
the report and pinned baseline under that directory's `.jet/perf/`; checks never
advance the baseline. The baseline name, target, profile, toolchain, hardware,
provider version, workload, twenty samples, and confidence policy are all part
of compatibility. Never compare rows from different profiles or machines.
