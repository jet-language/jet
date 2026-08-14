# Your first hour with Jet

In this tour, you install Jet, run one program, fix one error, and build a
native command-line tool.

## 1. Install Jet

On a supported host with Nix, install the current Jet package without a
repository checkout:

```bash
nix profile install github:jet-language/jet
jet version
```

Contributors can build from source instead:

```bash
git clone https://github.com/jet-language/jet.git
cd jet
nix build
export PATH="$PWD/result/bin:$PATH"
jet version
```

The installed `jet` binary is the compiler. For a source build,
`result/bin/jet` is the compiler and the `PATH` change applies to the current
shell.

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

## 4. Use the shared REPL and notebook

If `jet` is already installed on your host, this journey needs no repository
checkout. Verify the binary first:

```bash
jet version
```

Start the terminal REPL:

```bash
jet repl
```

Enter a binding and an expression. The session keeps both values:

```text
name :: "Jet"
name
```

For the first-party notebook, create a document and open the browser client:

```bash
jet notebook first-hour.jetnb
```

Open the printed local URL. Add a Jet cell, edit it, and run it. Add a
Markdown cell to explain the result. The same session provides inspect,
debug, profile, interrupt, completion, and queued `input()` controls.

Input and file access are explicit. Queue `Ada` in the browser's **Input**
control, then run this cell:

```jet
#Grant(caps: IO, FS) {
    name :: input("name: ") ?? "world"
    assert(name == "Ada")
    write_file("notes.txt", name) ?? panic("write failed")
    assert(file_exists("notes.txt"))
    assert_eq(file_exists("notes.txt"), true)
    print(read_file(Path.from("notes.txt")) ?? panic("read failed"))
}
```

The grant is the cell's explicit authority. `input`, `write_file`,
`file_exists`, and `read_file` are the same Prelude ambients used by the
terminal REPL; the relative file stays inside the notebook directory. The
`Path.from` call also shows the expert form used when a Core file operation
needs a `Path` value.

Save the document, stop the process, and run the same command again. **Reopen**
restores the cells and source. **Merge** combines another `.jetnb` by stable
cell ID. Export to `ipynb` or `.jet` shows an explicit loss report; imported
Jupyter output is quarantined until Jet runs it locally.

The first-party browser, Canvas lens, Jupyter adapter, and headless protocol
all call this one REPL session. They share stale-output rules, rich output,
source links, and trust decisions.

For a terminal-only or CI journey, use the same session as JSONL. Each input
line produces one JSON reply, so a consumer can stop after any observable
claim without waiting for end-of-file:

```bash
printf '%s\n' 'add-jet answer :: 42' 'exec first' 'state' |
  jet notebook --protocol first-hour.jetnb
```

If the browser or server stops, local drafts remain in the browser until the
server is reachable again. A stale cached result is shown as stale and must be
run again. An imported result remains plain text and quarantined. A merge with
the same cell ID edited on both sides is marked as a conflict; edit that cell
before running it.

## 5. Build something you can ship

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
