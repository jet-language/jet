# Binary size budgets

Jet checks ship-build size with `jet build --small`. This profile uses
`opt-level=z`, fat link-time optimization, `panic=abort`, and stripped symbols.
The release gate measures the final executable, not a Rust debug artifact.

## Release caps

The release gate uses the default Linux x86-64 target and measures the final
executable produced by `jet build --small`. Caps are limits, not targets:

| Workload | Example | CI cap |
|---|---|---:|
| Hello | `examples/features/basics/hello.jet` | 512,000 |
| CLI | `examples/features/io/cli.jet` | 524,288 |
| Small HTTP service | `examples/features/net/http_server_tasks.jet` | 3,145,728 |

`tests/release_gates.rs` also keeps the existing 4 MiB limits for the library,
low-level, and freestanding release examples.

## Size levers

- Use `--small` so the linker strips symbols and uses fat link-time optimization.
- Keep dead-code elimination effective by emitting and linking only reachable
  Core code.
- Select the smallest allocator that meets the program's ownership and
  performance needs.
- Check the final artifact after each toolchain or runtime change.
