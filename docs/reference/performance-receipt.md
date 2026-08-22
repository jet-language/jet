# Footprint receipt

This receipt records the footprint and readiness evidence for the two small
native programs in card #2142.

The capture uses one named box: AMD Ryzen 9 7950X3D, 32 logical CPUs,
66,502,565,888 bytes RAM, Linux 7.0.11-cachyos, `powersave`, rustc 1.97.1,
Jet 1.0.0, and compiler build
`b4e2fcf9c3a601d05030223bc8793ffd817373c89a6ee37ceb17a95c79685d89`.
Both receipts pin `x86_64-unknown-linux-gnu` in the `dev` profile and use the
baseline identity `card-2142/linux-x86-64-dev`. Statistical measurements use
twenty fresh trials. Checks do not advance the baseline.

| Program / source | Hardware | OS / toolchain | Target / profile | Trials | BinarySize measured / budget | StartupTime p50 / p95 / p99 / budget | MemoryHighWater p50 / p95 / p99 / budget | ServiceReadiness p50 / p95 / p99 / budget | Baseline identity |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| hello / [source](../../examples/performance/receipts/hello/src/run.jet) | AMD Ryzen 9 7950X3D; 32 CPUs; 66,502,565,888 B RAM | Linux 7.0.11-cachyos; rustc 1.97.1; Jet 1.0.0; compiler `b4e2fcf9…` | `x86_64-unknown-linux-gnu` / `dev` | 1 binary; 20 BuildArtifact | 509,096 B / 1 MiB | 1.304145 / 1.359411 / 1.473619 ms / 50 ms | 22,417,408 / 22,679,552 / 22,679,552 B / 24 MiB | not applicable | `card-2142/linux-x86-64-dev`<br>capture `060717800caeec4b0c058842076b4986cffc8fb55030574c024d84c00ceef87e` |
| http-ready / [source](../../examples/performance/receipts/http_ready/src/run.jet) | AMD Ryzen 9 7950X3D; 32 CPUs; 66,502,565,888 B RAM | Linux 7.0.11-cachyos; rustc 1.97.1; Jet 1.0.0; compiler `b4e2fcf9…` | `x86_64-unknown-linux-gnu` / `dev` | 1 binary; 20 BuildArtifact; 20 ServiceProbe | 750,872 B / 1 MiB | not applicable | 23,117,824 / 23,117,824 / 23,117,824 B / 24 MiB | 364.892284 / 367.797690 / 368.171524 ms / 500 ms | `card-2142/linux-x86-64-dev`<br>capture `ef7b97cbe46c54459533696743657714cb9b3dd24127f4934d42c95fb300ba14` |

`hello` uses the `BuildArtifact` provider for child-process startup and
memory. The server does not use a startup metric for its readiness claim.
`http_ready` declares `ServiceReadiness` through `ServiceProbe`; each fresh
trial starts the named service, then records the time until its configured
loopback HTTP `curl` ready event succeeds with `HTTP/1.1 200 OK`. The endpoint
returns body `ok`. The provider versions are
`jet-artifact-footprint-v1-first-line-vmhwm-trials-20` and
`jet-service-readiness-v1-trials-20`; the service provider uses
`service-process-group-down-up-per-trial` isolation.

The bootstrap capture reports are the IDs in the table. Subsequent checks
passed with reports
`061e4ddc5bcdefc492c9a11adc78d91e4ca75799ec9225d2de2c7ad4d116204a` and
`173e4b30f3ffdcaa77d692604166b1cff8550cf9bd2a0a0532941983a55171ca`,
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
