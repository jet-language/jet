# Capstone proof gate

Current `examples/apps/*` programs are deterministic implementation slices. They
are valuable CI fixtures, but none is a product capstone until it satisfies this
whole gate.

## Product Checklist

| Gate | Required proof |
| --- | --- |
| Standalone run path | A fresh checkout can run the app from one documented command without hidden services. |
| Real workflow | The app completes a user workflow that matches the product claim, not a canned demo. |
| Headless tests | Deterministic tests cover core behavior and failure paths. |
| UI/browser tests | GUI or web surfaces have automated interaction tests; CLI-only apps state why this is not applicable. |
| Packaging/deploy story | Native/web/freestanding package or deploy output is produced and verified. |
| Perf budget | Runtime, memory, binary size, or frame budget is measured against a checked-in baseline. |
| LOC comparison | A maintained comparison names the equivalent Rust/Python/TS/Go/game implementation and counts meaningful source size. |
| No facade | External services, fake adapters, and scripted-only behavior are called out; core value works offline against fixtures. |

## Current Classification

| App | Classification | Evidence | Missing before capstone |
| --- | --- | --- | --- |
| `jetgrep` | slice | CLI modes, recursive fixture scan, golden output, error tests | packaging, perf/LOC comparison, larger real corpus |
| `jetpaste` | slice | loopback HTTP create/read routes, persistence fixture, health/stats probes | real server workflow, route state through `core.http`, deploy story, perf/LOC comparison |
| `jettasks` | slice | reducer transcript plus web build | browser interaction test, persistence workflow, packaging, perf/LOC comparison |
| `jetfighter` / `JetPlay` | capstone | standalone run path, source-backed editor loop, headless tests, web editor build, native build, perf/LOC proof, offline replay | none |
| `metal` | slice | host-runnable core plus freestanding build proof | board/QEMU proof, deploy image, perf/size budget, LOC comparison |

## Product Capstones

`JetLab` must still inherit this checklist. `JetPlay` has capstone status via
`examples/apps/jetfighter`: the game imports editable source (`level.jet`), the
editor rewrites that source in a copied app root, tests rerun the game from the
edited source, and package/UI/perf/LOC evidence is checked in.
