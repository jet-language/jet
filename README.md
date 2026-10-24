# Jet

<p align="center"><img src="./Docs/assets/jetlang.png" width="120" alt="Jet logo" /></p>

Jet is a memory-safe, compiled programming language. Beginners get safe
defaults, little ceremony, and diagnostics that explain what went wrong, why,
and how to fix it. Experts get explicit control — down to audited unsafe
regions — without that machinery leaking into everyday code.

> **Pre-release.** Jet is at 0.0.1 and has no compatibility promise yet. The
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
- **One meaning on every execution tier.** `jet run` and `jet dev` compile a
  native executable through the normal build path and run it (`jet dev`
  rebuilds and restarts on every save), compile-time code runs in the
  in-process evaluator until native in-memory O0 replaces it, and web
  targets compile to WebAssembly. A program means the same thing on each.
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
error. Native builds go through Jet's own back end
([`Compiler/JetBackend`](Compiler/JetBackend/)); `jet run` and `jet dev` use
that same build path. Shared runtime meaning lives in the Prelude and the
Jet-authored core library, which every tier calls rather than
re-implementing.

**Jet is self-hosted.** The compiler is written in Jet, under `Compiler/`,
and builds itself: a candidate compiler must rebuild its own sources to a
byte-identical fixed point before it becomes the next seed.

The Jet parser's token entry points carry `TokenTables` alongside tokens:
spelling names and literal facts belong to the originating lexer tables, not
to an embedded token payload. Fence and named-marker expansion preserve that
association, and imported marker tokens are rebased into the receiving tables.

## Work on Jet

Read [AGENTS.md](AGENTS.md) first: it defines how decisions are made, the
invariants every change must keep, and how work is proven. Plans, decisions,
and status live in [Tower](Tools/tower/skills/tower/SKILL.md), the
project board, not in documentation.

Run repository commands through the pinned contributor environment:

```sh
export JET=/absolute/path/to/admitted/jet
Tools/agent/jet-env "$JET" run Examples/features/basics/hello.jet
Tools/agent/jet-env "$JET" check Examples/features/basics/functions.jet

# Check one example against its golden output
TESTKIT_ARGS="--filter Examples/features/basics/hello.jet" Tools/agent/jet-env "$JET" test Tools/testkit "suite golden"
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution workflow and
[SECURITY.md](SECURITY.md) to report a vulnerability.

## Repository map

| Path | Contents |
|---|---|
| [`Compiler/`](Compiler/) | The Jet compiler, written in Jet |
| [`Core/`](Core/) | The core library's Jet sources |
| [`Jetpack/`](Jetpack/) | Jetpack, the package manager, written in Jet |
| [`Examples/`](Examples/) | Executable examples and their golden output |
| [`tests/`](tests/) | Behavior, diagnostic-snapshot, and regression tests |
| [`Docs/`](Docs/README.md) | Specifications, guides, dated evidence, and the project website |
| [`Tools/`](Tools/) | Contributor tooling: the agent environment, CI and performance scripts, the performance gauntlet, editor integrations, and Tower, the project board |

`tests/` keeps its lowercase name; every other top-level folder is
capitalized.

## License

Jet is released under the [MIT License](LICENSE).
