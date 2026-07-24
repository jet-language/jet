# Binary size budgets

Jet checks ship-build size with `jet build --small`. This profile uses
`opt-level=z`, fat link-time optimization, `panic=abort`, and stripped symbols.
The release gate measures the final executable, not a Rust debug artifact.

## Current matrix

The measurements below use the default Linux x86-64 target. They were recorded
on 2026-07-24 after a fresh `cargo build`.

| Workload | Example | Measured bytes | CI cap |
|---|---|---:|---:|
| Hello | `examples/features/basics/hello.jet` | 382,120 | 512,000 |
| CLI | `examples/features/io/cli.jet` | 386,216 | 524,288 |
| Small HTTP service | `examples/features/net/http_server_tasks.jet` | 2,680,376 | 3,145,728 |

The hello cap preserves the existing release limit. The CLI cap is 512 KiB.
The service cap is 3 MiB. These caps allow limited toolchain variation but
stop large regressions. They are not targets to consume.

`tests/release_gates.rs` also keeps the existing 4 MiB limits for the library,
low-level, and freestanding release examples.

## Size levers

- Use `--small` so the linker strips symbols and uses fat link-time optimization.
- Keep dead-code elimination effective by emitting and linking only reachable
  Core code.
- Select the smallest allocator that meets the program's ownership and
  performance needs.
- Check the final artifact after each toolchain or runtime change.
