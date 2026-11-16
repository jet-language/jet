# Head-to-head `jet dev` benchmark

This note records a rerunnable comparison of the Jet web starter, Bun + Vite,
and npm + Vite. The harness is [`scripts/benchmarks/dev-dx.mjs`](../../scripts/benchmarks/dev-dx.mjs).

## Result

Measured 2026-08-27 at 11:25:46 UTC on Linux x64 with an AMD Ryzen 9 7950X3D
16-Core Processor, Node v22.23.2, and Chromium 151.0.7922.137. Tool versions
were Jet 1.0.0, Bun 1.4.0, npm 10.9.8, and Vite 8.2.2.

| Tool | Steps to first app | Cold page | Warm page | Warm reload median | Project install bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Jet | 3 | 519 ms | 502.2 ms | 561.5 ms | 0 B |
| Bun + Vite | 4 | 316.9 ms | 286.6 ms | 55.9 ms | 60.3 MiB |
| npm + Vite | 4 | 401 ms | 332.1 ms | 54.9 ms | 32.2 MiB |

Warm reload samples, in run order:

| Tool | Ten samples in milliseconds |
| --- | --- |
| Jet | 486.0, 561.4, 561.9, 561.5, 563.1, 564.2, 563.7, 482.1, 559.2, 560.1 |
| Bun + Vite | 51.8, 56.9, 55.6, 56.1, 56.6, 56.9, 58.3, 54.6, 53.6, 54.0 |
| npm + Vite | 50.8, 53.7, 54.5, 56.6, 57.2, 55.2, 57.6, 55.7, 52.9, 52.9 |

Jet meets the steps criterion: three typed commands versus four for each Vite
flow. Jet loses all three latency comparisons. Cold page time is 63.8% slower
than Bun + Vite and 29.4% slower than npm + Vite. Warm reload median is about
10.0 times Bun + Vite and 10.2 times npm + Vite.

Named follow-up: `Jet web dev reload latency parity`. The result does not meet
the epic latency gate, so epic #2234 must stay open until this loss is fixed or
owner policy changes.

## Qualitative checklist

| Tool | Zero-config check | Error overlay | Project install weight |
| --- | --- | --- | ---: |
| Jet | Yes for this flow. `jet new --target=web` writes the package and entry; no install or Vite config is needed. | Registered diagnostic code and error text include the What, Why, and Fix fields. | 0 B |
| Bun + Vite | The starter has no `vite.config` file, but `bun install` is required. | Vite overlay shows JavaScript/source error text and a stack. It has no registered What/Why/Fix structure. | 60.3 MiB |
| npm + Vite | The starter has no `vite.config` file, but `npm install` is required. | Vite overlay shows JavaScript/source error text and a stack. It has no registered What/Why/Fix structure. | 32.2 MiB |

Project install weight is the recursive file size of `node_modules` plus the
project lockfile. It excludes global package caches, the language toolchain,
and Chromium. The first run provisioned Bun into the local benchmark cache; that
cache was 154.1 MiB and is separate from project install weight.

## Method

Each tool starts in a new empty directory. The benchmark uses the stock Jet web
starter and the Vite `vanilla` starter. The exact typed command sequences are:

```text
jet new app --target=web
cd app
jet dev
```

```text
bun create vite app --template vanilla --no-interactive
cd app
bun install
bun run dev
```

```text
npm create vite@latest app -- --template vanilla --no-interactive
cd app
npm install
npm run dev
```

The harness counts each command, including `cd`. It adds localhost, port, and
strict-port flags to the dev processes so runs do not collide; those harness
flags are not extra user steps. The Vite command shape follows the [Vite getting-started guide](https://vite.dev/guide/) and the [Bun Vite guide](https://bun.sh/guides/ecosystem/vite).

The harness uses the existing [Chromium CDP driver](../../scripts/canvas-test/driver.mjs)
and starts Chromium before each dev command. It starts the timer just before the
dev process starts. A first page succeeds only when the expected counter text
appears in the browser and a button click changes the counter.

The cold page measure follows the fresh scaffold and install. The warm page
measure stops and restarts the dev process in the same installed project. The
reload measure edits the visible counter source ten times and waits for each
new label in the browser DOM. Jet edits `run.jet`; both Vite flows edit
`src/counter.js`. The median is the mean of sorted samples five and six.

Cold means a clean project and a new dev process. It does not clear network or
package-manager caches. The harness deletes its temporary project directories
after each run.

## Reproduce

Run from the Jet repository in the full shell:

```sh
scripts/agent/jet-env full node scripts/benchmarks/dev-dx.mjs
```

The harness provisions Bun with npm when `bun` is not on `PATH`. Set `BUN_BIN`
to use an existing Bun executable. Use `--json` for machine-readable output.
