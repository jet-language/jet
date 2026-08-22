# Footprint receipt

This receipt records the footprint and readiness evidence for the two small
native programs in card #2142.

The named capture box is an AMD Ryzen 9 7950X3D with Linux
7.0.11-cachyos, `powersave`, and rustc 1.97.1
(`x86_64-unknown-linux-gnu`). The pinned target is
`x86_64-unknown-linux-gnu` in the `dev` profile. Statistical measurements use
twenty fresh child or service trials. The pinned baseline identity is
`card-2142/linux-x86-64-dev`; checks never advance that identity. The server
ServiceProbe row remains unaccepted until the fresh provider capture succeeds.

| Program / source | Hardware | OS / toolchain | Target / profile | Trials | BinarySize measured / budget | StartupTime p50 / p95 / p99 / budget | MemoryHighWater p50 / p95 / p99 / budget | ServiceReadiness p50 / p95 / p99 / budget | Baseline identity |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| hello / [source](../../examples/performance/receipts/hello/src/run.jet) | AMD Ryzen 9 7950X3D | Linux 7.0.11-cachyos; rustc 1.97.1 | `x86_64-unknown-linux-gnu` / `dev` | 1 binary; 20 statistical | 509,096 B / 1 MiB | 2.657 / 2.841 / 3.031 ms / 50 ms | 2,359,296 / 2,420,736 / 2,445,312 B / 8 MiB | not applicable | `card-2142/linux-x86-64-dev` |
| http-ready / [source](../../examples/performance/receipts/http_ready/src/run.jet) | AMD Ryzen 9 7950X3D | Linux 7.0.11-cachyos; rustc 1.97.1 | `x86_64-unknown-linux-gnu` / `dev` | 1 binary; 20 configured | 750,872 B / 1 MiB | not applicable | not captured | not captured / 50 ms | `card-2142/linux-x86-64-dev` |

`hello` uses the `BuildArtifact` provider for child-process startup and
memory. The server does not use a startup metric for its readiness claim.
`http_ready` declares `ServiceReadiness` through `ServiceProbe`; each fresh
trial must start the named service and pass its configured HTTP `curl` ready
event after the loopback listener binds. The endpoint returns
`HTTP/1.1 200 OK` with body `ok`.

The canonical check and baseline update are:

```text
cd examples/performance/receipts/hello
TMPDIR=$HOME/.cache/jet-test-scratch ../../../../scripts/agent/jet-env full jet budget check --json
TMPDIR=$HOME/.cache/jet-test-scratch ../../../../scripts/agent/jet-env full jet budget update --baseline card-2142/linux-x86-64-dev --bootstrap --reason "card 2142 initial same-box receipt" --yes --json

cd ../http_ready
TMPDIR=$HOME/.cache/jet-test-scratch ../../../../scripts/agent/jet-env full jet budget check --json
TMPDIR=$HOME/.cache/jet-test-scratch ../../../../scripts/agent/jet-env full jet budget update --baseline card-2142/linux-x86-64-dev --bootstrap --reason "card 2142 initial same-box receipt" --yes --json
```

Reports and baselines live under each program's `.jet/perf/` directory. The
baseline name, target, profile, toolchain, hardware, provider version,
workload, trial count, and confidence policy are part of compatibility. Do
not compare rows from different profiles or machines.
