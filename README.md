# Jet

<p align="center"><img src="./Docs/assets/jetlang.png" width="120" alt="Jet logo" /></p>

Jet is a memory-safe, compiled programming language that aims to be the
easiest language to start with and the hardest to outgrow. Beginners get safe
defaults, little ceremony and diagnostics that say what went wrong, why, and
how to fix it. Experts get explicit control, down to audited unsafe regions,
without that machinery leaking into everyday code.

> **Pre-release.** Jet ships as numbered `a0_NNN` releases and makes no
> compatibility promise yet. The language, library and tools change in place,
> with no deprecation period, until a 1.0 policy is declared. See the
> [release policy](Docs/spec/release-policy.md).

## A first look

```jet
#Error
NameError: enum {
    Empty
}

Point: struct {
    x: Float
    y: Float

    dist_sq: fn(self) -> Float { self.x * self.x + self.y * self.y }
}

Light: enum {
    Red
    Yellow
    Green
}

next: fn(light: Light) -> Light {
    if light == {
        .Red -> Light.Green
        .Green -> Light.Yellow
        .Yellow -> Light.Red
    }
}

clean_name: fn(raw: String) -> String NameError! {
    if raw == "" -> return Err(NameError.Empty)
    Ok(raw)
}

greet: fn(name: String) -> String { "hello, {name}" }

#CLI
Args: struct {
    #Doc("who to greet") name: String{"world"}
}

#Test("greet says hello") {
    assert_eq(greet("Jet"), "hello, Jet")
}

run: fn(args: Args) {
    name: clean_name(args.name) ?? "stranger"
    print(greet(name))
    p: Point{x: 3.0, y: 4.0}
    print("distance squared: {p.dist_sq()}")
    print("after red: {next(Light.Red)}")
}
```

```text
$ jet run tour.jet -- --name Ada
hello, Ada
distance squared: 25.0
after red: Green

$ jet test tour.jet
greet says hello: pass
1 passed, 0 failed, 0 skipped
```

- Every declaration is written `name: value`. A type, a function and a
  constant all use the same form; `name := value` makes a mutable binding.
- `-> String NameError!` returns a `String` or fails with a `NameError`.
  Errors are ordinary values, and `??` supplies a fallback.
- `if subject == { … }` matches a value against arms.
- A `#CLI` struct *is* the command line: `jet run tour.jet -- --help` lists
  `--name` with its documentation and default.
- `#Test` blocks sit next to the code they test and run with `jet test`.

## Install

Jet runs on x86_64 Linux. Each release on
[GitHub](https://github.com/jet-language/jet/releases) is one archive with
the `jet` binary, its runtime and checksums:

```sh
gh release download --repo jet-language/jet --pattern 'jet-*-linux-x64.tar.gz*'
sha256sum -c jet-*-linux-x64.tar.gz.sha256
mkdir -p ~/.local/jet && tar -xzf jet-*-linux-x64.tar.gz -C ~/.local/jet
export PATH="$HOME/.local/jet:$PATH"
jet version
```

> Until the release binary carries its own library sources, `jet run` needs a
> checkout of the same release. Clone it once and point Jet at it:
> `git clone --depth 1 --branch "$(cat ~/.local/jet/RELEASE)" https://github.com/jet-language/jet ~/.local/jet-src`
> and `export JET_TOOLCHAIN_ROOT=~/.local/jet-src`.

Create and run a project:

```sh
jet new hello && cd hello
jet run     # prints: hello, world
jet test    # runs the project's #Test blocks
```

## What you get

- **Safety by default.** Programs are memory-safe, type-safe and leak-free.
  Low-level work goes in an explicit `#Unsafe("reason") { … }` region that
  tools can audit.
- **Diagnostics as a product.** Every error has a stable code and says what
  happened, why the rule exists and how to fix it. `jet explain <CODE>`
  expands a code into a short lesson; `jet fix` applies the safe repairs.
- **One meaning everywhere.** `jet run` and `jet dev` compile natively and run
  in memory, `jet build` and `--release` produce optimized executables, and
  compile-time code runs natively too. One runtime serves every tier, so a
  program means the same thing on each.
- **A large core library**, written in Jet: files, processes, HTTP, JSON and
  other encodings, time, text, collections, concurrency, terminal and UI, and
  more, as `core.*` modules. See the
  [core library reference](Docs/spec/reference/core-library.md).
- **One tool for the workflow.** `jet` formats, checks, tests, documents,
  debugs, profiles, packages and manages dependencies; `jet help` lists every
  command. Jetpack, the package manager, is built in.
- **Interop.** Jet binds to C and C++ through checked foreign-function
  boundaries.

## Learn Jet

| Start with | For |
|---|---|
| [First-hour guide](Docs/spec/guides/first-hour.md) | `new`, `run`, `check`, `test`, `fix` and `explain`, step by step |
| [Examples](Examples/README.md) | Small programs for every feature, grouped by topic, each with checked output |
| [Diagnostic recovery](Docs/spec/guides/diagnostic-recovery.md) | Reading and fixing compiler errors |
| [Core library reference](Docs/spec/reference/core-library.md) | The `core.*` modules |
| [Language spec](Docs/spec/spec.md) | The language contract, topic by topic |
| [Philosophy](Docs/spec/philosophy.md) | What Jet is for and how it ranks trade-offs |
| [Documentation index](Docs/README.md) | Everything else |

## Where Jet is going

Plans, priorities and status live on [Tower](Tools/tower/skills/tower/SKILL.md),
the project board, not in documents. The active goals are:

- **Hand-off and durability:** a fresh contributor can clone, fetch the latest
  release and build, test and ship Jet with no private setup.
- **Finish Rust removal and self-hosting:** every tool and test runs on Jet
  alone.
- **Declarations as types and metaprogramming:** the core library declares its
  own runtime functions, with no compiler-side copy of its API.
- **Core library completeness**, **effects on types**, and **memory safety and
  soundness**.
- **Developer experience:** the CLI, error messages, the editor, and a
  first-class testing toolkit.
- **Packages and configuration,** replacing Nix entirely.
- **Best scripting language**, and **web, end to end**, measured against the
  strongest competitors.
- **Speed and size:** beat every peer language and match hand-written machine
  code on each measured workload.

## Work on Jet

The compiler, the package manager and the tools are written in Jet. The
compiler builds itself: a candidate must rebuild its own sources to a
byte-identical fixed point before it becomes the next release.

Read [AGENTS.md](AGENTS.md) first. It defines how decisions are made, the
invariants every change keeps and how work is proven. Then see
[CONTRIBUTING.md](CONTRIBUTING.md); report vulnerabilities through
[SECURITY.md](SECURITY.md).

To build Jet from a fresh clone, take the latest release as the seed and let
it build the compiler:

```sh
TAG=$(gh release view --repo jet-language/jet --json tagName --jq .tagName)
git clone --branch "$TAG" https://github.com/jet-language/jet && cd jet
gh release download "$TAG" --repo jet-language/jet --pattern "jet-$TAG-linux-x64.tar.gz*" --dir ../release
(cd ../release && sha256sum -c "jet-$TAG-linux-x64.tar.gz.sha256")
mkdir ../seed && tar -xzf "../release/jet-$TAG-linux-x64.tar.gz" -C ../seed
../seed/jet build Compiler     # writes Compiler/JetCli/.jet/build/jet
```

[Building Jet](Docs/spec/contributing/bootstrap.md) covers the rest: giving the
new compiler its runtime, the full self-host chain, walking forward across
releases, and the frozen `rust` branch as the last-resort seed.

| Path | Holds |
|---|---|
| [`Compiler/`](Compiler/) | The compiler, its runtime and the embedded Prelude |
| [`Core/`](Core/) | The core library |
| [`Jetpack/`](Jetpack/) | The package manager |
| [`Examples/`](Examples/README.md) | Executable examples with checked output |
| [`tests/`](tests/) | Behavior, diagnostic-snapshot and regression tests |
| [`Docs/`](Docs/README.md) | Specifications, guides, dated evidence and the website |
| [`Tools/`](Tools/) | Contributor tooling, CI, performance gates and Tower |

## License

Jet is released under the [MIT License](LICENSE).
