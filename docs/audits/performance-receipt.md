# Footprint receipt

This receipt records the footprint and readiness evidence for the two small
native programs in card #2142.

The capture uses one named box: AMD Ryzen 9 7950X3D, 32 logical CPUs,
66,502,565,888 bytes RAM, Linux 7.0.11-cachyos, `powersave`, rustc 1.97.1,
and Jet 1.0.0. The hello compiler build is
`f38a265bf7595770e3136efabe2dd08596362bd7c1c9967d68f588d649bcee1a`; the
http-ready compiler build is
`4f655fe5c5c2a92d6fff865bb3d6af782b08dd392c9d6bfecb49c6efae8a74fc`.
Both receipts pin `x86_64-unknown-linux-gnu` in the `dev` profile and use the
baseline identity `card-2142/linux-x86-64-dev`. Statistical measurements use
twenty fresh trials. BinarySize uses one artifact sample. Checks do not advance
the baseline.

| Program / source | Hardware | OS / toolchain | Target / profile | Trials | BinarySize measured / budget | StartupTime p50 / p95 / p99 / budget | MemoryHighWater p50 / p95 / p99 / budget | ServiceReadiness p50 / p95 / p99 / budget | Baseline identity |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| hello / [source](../../examples/performance/receipts/hello/src/run.jet) | AMD Ryzen 9 7950X3D; 32 CPUs; 66,502,565,888 B RAM | Linux 7.0.11-cachyos; rustc 1.97.1; Jet 1.0.0; compiler `f38a265b…` | `x86_64-unknown-linux-gnu` / `dev` | 1 binary; 20 BuildArtifact | 509,096 B / 1 MiB | 1.290819 / 1.316548 / 1.529333 ms / 50 ms | 21,884,928 / 21,884,928 / 21,884,928 B / 24 MiB | not applicable | `card-2142/linux-x86-64-dev`<br>capture `7be5801ecee3f06298df06fa8a83e10e04b52e6bf452e15721791c0095456e41` |
| http-ready / [source](../../examples/performance/receipts/http_ready/src/run.jet) | AMD Ryzen 9 7950X3D; 32 CPUs; 66,502,565,888 B RAM | Linux 7.0.11-cachyos; rustc 1.97.1; Jet 1.0.0; compiler `4f655fe5…` | `x86_64-unknown-linux-gnu` / `dev` | 1 binary; 20 BuildArtifact; 20 ServiceProbe | 750,872 B / 1 MiB | not applicable | 21,782,528 / 21,782,528 / 21,782,528 B / 24 MiB | 364.314649 / 364.933948 / 365.001768 ms / 500 ms | `card-2142/linux-x86-64-dev`<br>capture `12404763d201458d0356dc11e3bce0ba211aca467d64bdad32c9263e14db88a0` |

`hello` uses the `BuildArtifact` provider for child-process startup and
memory. The server does not use a startup metric for its readiness claim.
`http_ready` declares `ServiceReadiness` through `ServiceProbe`; each fresh
trial starts the named service after binding, then records the time until its
configured loopback HTTP `curl` ready event succeeds with `HTTP/1.1 200 OK`.
The endpoint returns body `ok`. The report records 20 fresh samples for this
gate. It records the configured readiness event, not process-start time. The
provider versions are
`jet-artifact-footprint-v1-first-line-vmhwm-trials-20` and
`jet-service-readiness-v1-trials-20`; the service provider uses
`service-process-group-down-up-per-trial` isolation.

The bootstrap capture reports are the IDs in the table. Subsequent checks
passed with reports
`42bdcb3781f0fe58b780b943c35be1ff4cf0786bdd3034b16f03da8cc193ff0b` and
`89431d03b96ce4665481f9d548e8b674a42e112e8485f27f87b0ea78e281f263`,
respectively. The table records only this
one hardware, OS, toolchain, target, and profile identity; it makes no
cross-machine or cross-profile comparison.

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
