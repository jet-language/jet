# Jet

<p align="center"><img src="./Docs/assets/jetlang.png" width="120" alt="Jet logo" /></p>

Jet is a memory-safe, compiled programming language. Beginners get safe
defaults, little ceremony, and diagnostics that explain what went wrong, why,
and how to fix it. Experts get explicit control — down to audited unsafe
regions — without that machinery leaking into everyday code.

> **Pre-release.** Jet is at 0.1 and has no compatibility promise yet. The
> language, standard library, and tools change in place, without deprecation
> periods, until a 1.0 policy is declared. See the
> [release policy](Docs/spec/release-policy.md).

## A first look

```jet
#Error
enum NameError {
    Empty
}

struct Point {
    x: Float
    y: Float

    fn dist_sq(self) -> Float { self.x * self.x + self.y * self.y }
}

enum Light {
    Red
    Yellow
    Green
}

fn next(light: Light) -> Light {
    if light == {
        .Red -> Light.Green
        .Green -> Light.Yellow
        .Yellow -> Light.Red
    }
}

fn clean_name(raw: String) -> String !NameError {
    if raw == "" -> return Err(NameError.Empty)
    Ok(raw)
}

fn greet(name: String) -> String { "hello, {name}" }

#CLI
struct Args {
    #Doc("who to greet") name: String{"world"}
}

#Test("greet says hello") {
    assert_eq(greet("Jet"), "hello, Jet")
}

fn run(args: Args) {
    name :: clean_name(args.name) ?? "stranger"
    print(greet(name))
    p :: Point{x: 3.0, y: 4.0}
    print("distance squared: {p.dist_sq()}")
}
```

```text
$ jet run tour.jet -- --name Ada
hello, Ada
distance squared: 25.0
```

A few things to notice:

- `name :: value` binds an immutable name; `name := value` binds a mutable one.
  Types come from values or from signatures.
- `-> String !NameError` declares a result that is either a `String` or a
  `NameError`. Errors are ordinary values; `??` supplies a fallback.
- `if subject == { … }` matches a value against arms; each arm is an
  expression or a block.
- A `#CLI` struct *is* the command-line interface: `jet run tour.jet -- --help`
  lists `--name` with its documentation and default.
- `#Test` blocks live next to the code they test and run with `jet test`.

The [executable examples](Examples/README.md) cover the rest of the
language, each with golden-tested output.

## What Jet provides

- **Safety by default.** Programs are memory- and type-safe. Low-level
  control is available, but unsafe operations must sit inside an explicit
  `#Unsafe("reason") { … }` region that tools can audit.
- **Diagnostics as a product.** Every error has a stable code and explains
  what happened, why Jet enforces the rule, and how to fix it.
  `jet explain <CODE>` expands any code into a short lesson, and `jet fix`
  applies registered safe repairs.
- **One meaning on every execution tier.** By default, `jet run` and
  `jet dev` execute through a Cranelift JIT with an interpreter for
  deoptimization and compile-time evaluation; `jet build` (and `jet run`
  when you ask for a release profile, an artifact, or a target) compiles a
  native executable, and web targets compile to WebAssembly. A program
  means the same thing on each.
- **A batteries-included core library.** Files, HTTP, JSON and other
  encodings, time, text, collections, concurrency, terminal and UI, data,
  and more ship as `core.*` modules, most of them written in Jet. See the
  [core library reference](Docs/spec/reference/core-library.md).
- **One tool for the whole workflow.** The `jet` command formats, checks,
  tests, documents, debugs, profiles, packages, and manages dependencies.
  `jet help` lists every command.
- **Interoperability.** Jet binds to C and C++ and to a range of other
  language runtimes through checked foreign-function boundaries.

## Install

The supported install path uses [Nix](https://nixos.org/) with flakes on
x86_64 Linux and x86_64 macOS:

```sh
nix --extra-experimental-features "nix-command flakes" profile install github:jet-language/jet
jet version
```

Create and run your first project:

```sh
jet new hello
cd hello
jet run          # prints: hello, world
jet test         # runs the project's #Test blocks
```

## Learn Jet

| Start with | For |
|---|---|
| [First-hour guide](Docs/spec/guides/first-hour.md) | Install, then `new` → `run` → `check` → `test` → `fix` → `explain` |
| `jet learn` | Offline practice exercises in your terminal |
| [Examples](Examples/README.md) | Small, golden-tested programs for each feature |
| [Diagnostic recovery](Docs/spec/guides/diagnostic-recovery.md) | Reading and fixing compiler errors |
| [Core library reference](Docs/spec/reference/core-library.md) | The standard `core.*` modules |
| [Language spec](Docs/spec/spec.md) | The language contract, topic by topic |
| [Documentation index](Docs/README.md) | Everything else |

## How Jet is built

The compiler front end — lexer, parser, and semantic checker — owns every
language rule. Code generation only lowers facts the checker has already
established, so a backend failure is always a compiler bug, never a user
error. Native builds emit Rust and compile it with rustc and LLVM; by
default `jet run` and `jet dev` use a Cranelift JIT. Shared runtime meaning
lives in the Prelude and the Jet-authored core library, which every tier
calls rather than re-implementing.

**Self-hosting is in progress.** The production compiler is written in Rust
(`Source/` and `crates/`) and remains the reference implementation. New
compiler work happens in a staged port of the compiler to Jet itself, under
`Compiler/`: one pass at a time, each Jet-authored pass proven against the
Rust reference before it is trusted. The Rust-emission, rustc/LLVM, and
Cranelift backends stay. Jet counts as self-hosted only once the Jet
compiler reproducibly builds itself and becomes the default compiler.

## Work on Jet

Read [AGENTS.md](AGENTS.md) first: it defines how decisions are made, the
invariants every change must keep, and how work is proven. Plans, decisions,
and status live in [Tower](Tools/tower/skills/tower/SKILL.md), the
project board, not in documentation.

Run repository commands through the pinned contributor environment:

```sh
Tools/agent/jet-env cargo build
Tools/agent/jet-env jet run Examples/features/basics/hello.jet
Tools/agent/jet-env jet check Examples/features/basics/functions.jet

# Check one example against its golden output
Tools/agent/jet-env env JET_GOLDEN_FILTER=Examples/features/basics/hello.jet \
  cargo test --test golden examples_compile_and_run -- --nocapture
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution workflow and
[SECURITY.md](SECURITY.md) to report a vulnerability.

## Repository map

| Path | Contents |
|---|---|
| [`Source/`](Source/) | The `jet` command-line tool (Rust reference implementation) |
| [`crates/`](crates/) | Rust crates: front end, code generation, JIT, runtime, Jetpack, and a vendored Cranelift patch |
| [`Compiler/`](Compiler/) | The staged Jet-authored compiler port |
| [`Core/`](Core/) | The core library's Jet sources |
| [`Jetpack/`](Jetpack/) | The staged Jet-authored port of the Jetpack package manager |
| [`Examples/`](Examples/) | Executable examples and their golden output |
| [`tests/`](tests/) | Behavior, diagnostic-snapshot, regression, and compiler-proof tests |
| [`Docs/`](Docs/README.md) | Specifications, guides, dated evidence, and the project website |
| [`Tools/`](Tools/) | Contributor tooling: the agent environment, CI and performance scripts, the performance gauntlet, editor integrations, and Tower, the project board |

`crates/` and `tests/` keep Cargo's lowercase names; every other top-level
folder is capitalized.

## License

Jet is released under the [MIT License](LICENSE).
