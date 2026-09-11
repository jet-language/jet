# Jet

<p align="center"><img src="./assets/jetlang.png" width="120" alt="Jet" /></p>

Jet is a memory-safe compiled language with safe beginner defaults and explicit
expert control. Read the [philosophy](docs/spec/philosophy.md) for design
priorities and [AGENTS.md](AGENTS.md) for contributor decision boundaries.

Code, registries, tests, and executable examples establish current behavior.
[Tower](plugins/tower/skills/tower/SKILL.md) owns plans and development status.
This page is a starting point, not a capability or readiness inventory.

## Install and run

With Nix flakes:

```sh
nix --extra-experimental-features "nix-command flakes" profile install github:jet-language/jet
jet version
jet new hello
cd hello
jet run
```

The example prints `hello, world`. Continue with the
[first-hour guide](docs/spec/guides/first-hour.md), or use `jet help` for the
commands provided by your installed compiler. Read the
[release policy](docs/spec/release-policy.md) for compatibility rules rather
than inferring production suitability from an example.

## Work on Jet

Run repository commands through `scripts/agent/jet-env` to use the pinned
contributor environment:

```sh
scripts/agent/jet-env cargo build
scripts/agent/jet-env jet run examples/features/basics/hello.jet
scripts/agent/jet-env jet check examples/features/basics/functions.jet
scripts/agent/jet-env env JET_GOLDEN_FILTER=examples/features/basics/hello.jet \
cargo test --test golden examples_compile_and_run -- --nocapture
```

[Executable examples](examples/README.md) include small feature programs and
end-to-end workflows. [`examples/canon.jet`](examples/canon.jet) is the syntax
showcase. Use the tests beside a mechanism to determine its behavior; prose is
not a substitute for an exercised result.

## Diagnostics and source reference

Try a diagnostic and ask the compiler to explain its registered code:

```sh
scripts/agent/jet-env jet check tests/ui/unknown_function.jet
scripts/agent/jet-env jet explain E0102
```

The [diagnostic recovery guide](docs/spec/guides/diagnostic-recovery.md) gives
exercises. Message text lives in the
[diagnostic registry](crates/jet-codegen/src/Prelude/Diagnostics.jet), with
rendering evidence in [UI snapshots](tests/ui/).

For a reference derived from the compiler's registries:

```sh
scripts/agent/jet-env jet inspect digest --list-topics
scripts/agent/jet-env jet inspect digest --topic diagnostics
```

## Repository map

| Path | Purpose |
|---|---|
| [Documentation](docs/README.md) | Navigation to explanations and dated evidence |
| [Examples](examples/) | Executable programs and expected results |
| [Tests](tests/) | Behavior, boundary, and regression checks |
| [Compiler crates](crates/) | Language implementation and execution engines |
| [Prelude](crates/jet-codegen/src/Prelude/) and [CoreLib](corelib/) | Shared semantic and library sources |
| [CLI implementation](crates/jet-cli/src/) | Commands, flags, and explanations |
| [Editors](editors/) | Editor integrations and grammar sources |

## License

See [LICENSE](LICENSE).
