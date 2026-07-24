# Your first hour with Jet

In this tour, you install Jet, run one program, fix one error, and build a
native command-line tool.

## 1. Install Jet

Jet uses Nix for its current build. Clone the repository and build the
compiler:

```bash
git clone https://github.com/jet-language/jet.git
cd jet
nix build
export PATH="$PWD/result/bin:$PATH"
jet version
```

The `result/bin/jet` binary is the compiler. The `PATH` change applies to your
current shell.

## 2. Run your first program

Make a work directory outside the repository. Save this as `hello.jet`:

```jet
fn run() {
    print("hello, world")
}
```

Run the file:

```bash
jet run hello.jet
```

Jet prints:

```text
hello, world
```

`fn run()` is the entry point. `print` is built in, so this program needs no
imports, manifest, or project setup. This snippet is also the
[golden-tested hello example](../examples/features/basics/hello.jet).

## 3. Read your first error

Change `print` to `pirnt`, then check the file:

```bash
jet check hello.jet
```

Jet reports the problem and its fix:

```text
Error [E0102]: nothing named `pirnt` exists here
 Why: only functions that have been defined (or built in, like `print` / `input`) can be called
 Fix: did you mean `print`?
```

Change `pirnt` back to `print`. Run `jet check hello.jet` again.

The error code stays stable. The full
[E0102 reference](reference/errors/E0102.md) links to its tested failing
program.

## 4. Build something you can ship

Save the following program as `ship.jet`:

```jet
use core.io as io

fn run() {
    args :: io.args()
    project :: if args.len() > 1 {
        args.get(1) ?? "first-hour"
    } else {
        "first-hour"
    }
    steps :: ["check source", "build binary", "run smoke test"]
    print("Shipping {project}")
    loop step; steps {
        print("[ok] {step}")
    }
}
```

Run it:

```bash
jet run ship.jet
```

The output is:

```text
Shipping first-hour
[ok] check source
[ok] build binary
[ok] run smoke test
```

Build the native executable and run it:

```bash
jet build --release ship.jet
./build/ship
```

Pass a project name after `--`:

```bash
jet run ship.jet -- dashboard
```

The delimiter tells Jet to forward later arguments to your program.
`io.args()` reads them. The complete program is the
[golden-tested first-hour example](../examples/features/basics/first_hour.jet).

## Expert checkpoint

The beginner path and expert tools use the same program:

```bash
jet fmt --check ship.jet
jet check ship.jet --json
jet build --release ship.jet
```

Use `--json` for machine-readable diagnostics. Use
[`core.io`](reference/core-library.md#coreio--terminal-and-arguments) for
terminal streams and command-line arguments. Read the
[language spec](spec/spec.md) for exact semantics and
[syntax decisions](spec/syntax-decisions.md) for ratified spellings.

You now have one source file that runs directly and builds as a native
executable. Continue with the
[Core library guide](reference/core-library.md) or the
[executable feature examples](../examples/features/).
