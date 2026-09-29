# Install Jet and learn your first workflow

This tutorial is for a new Jet user who can open a terminal and edit a file.
It walks through `new`, `run`, `check`, `test`, a manual correction, and
`explain`. The executable teaching source is
[`Examples/features/basics/onboarding/run.jet`](../../../Examples/features/basics/onboarding/run.jet);
the CLI recovery behavior is exercised by
[`tests/onboarding_recovery.rs`](../../../tests/onboarding_recovery.rs) and the
terminal/editor matrix by
[`tests/onboarding_matrix.rs`](../../../tests/onboarding_matrix.rs).

## 1. Check the supported host

The documented release install path covers x86_64 Linux and x86_64 macOS with
Nix flakes. Other hosts need the platform-specific project track. Do not
replace a local installation with an SSH session or a manual remote rebuild.

Check the operating system and Nix before installing:

```sh
uname -s
uname -m
nix --version
```

If `nix` is missing, install Nix using its host instructions and repeat the
check. Stop if the host is outside the supported set; the platform-specific
project track is safer than guessing at a compiler install.

## 2. Install Jet

Install Jet into the user profile. Enabling the two Nix features on the
command line also works when the local Nix configuration has not enabled
flakes:

```sh
nix --extra-experimental-features "nix-command flakes" profile install github:jet-language/jet
jet version
```

The second command verifies that the new executable is on `PATH`. If Nix
reports an offline or network failure, reconnect and repeat the install
command. If installation succeeds but `jet` is not found, start a new shell
and run `jet version` again. Do not continue with a partial install.

A contributor working from a checkout can use the explicit source path
instead:

```sh
git clone https://github.com/jet-language/jet.git
cd jet
nix --extra-experimental-features "nix-command flakes" build
export PATH="$PWD/result/bin:$PATH"
jet version
```

## 3. Lesson 1: `new`

Choose a parent directory that does not already contain a `hello` entry.
`jet new` creates a project directory and uses `run.jet` as its default entry
file:

```sh
mkdir -p "$HOME/jet-projects"
cd "$HOME/jet-projects"
jet new hello
cd hello
```

The scaffold contains the package manifest, the default entry, and a Git
ignore file:

```text
package.jet
run.jet
.gitignore
```

The generated `run.jet` prints a greeting and carries one smoke `#Test`:

```jet
fn greeting(name: String) -> String { "hello, {name}" }

fn run() {
    print(greeting("world"))
}

#Test("the greeting stays stable") {
    assert_eq(greeting("world"), "hello, world")
}
```

Richer starters are opt-in templates: `jet new hello --template cli` adds a
typed `#CLI` argument struct, `--template ui` a native UI tree, `--template web`
the browser app for `jet dev`, and `--template overrides` the commented
`@run.jet`/`@build.jet`/`@dev.jet`/`@test.jet` command-override homes.

If `hello` already exists, choose another name. `jet new` does not overwrite
an existing directory.

## 4. Lesson 2: `run`

The generated `run.jet` is already runnable. Run it from the project
directory:

```sh
jet run
```

The output is:

```text
hello, world
```

Bare `jet run` resolves `run.jet` in the current project. Name the same file
explicitly when a project contains several possible targets:

```sh
jet run run.jet
```

The explicit form selects the same source. Continue to the next lesson before
replacing the generated source.

## 5. Lesson 3: `check`

Replace `run.jet` with the canonical onboarding source below. Its function
signature uses the current `fn name(params) -> Type { ... }` form:

```jet
fn greet(name: String) -> String { "hello, {name}" }

#CLI
struct GreetingArgs {
    #Doc("name to greet") name: String{"Jet"}
}

#Test("greet says hello") {
    assert_eq(greet("Jet"), "hello, Jet")
}

fn run(args: GreetingArgs) { print(greet(args.name)) }
```

Check the edited source without running its entry function:

```sh
jet check run.jet
```

A successful check exits with code zero and prints one line:

```text
ok: `run.jet` has no problems
```

`jet check` is a source check; it does not run `fn run`. Add `--verbose` to see
the check's scope and proof rows.

## 6. Lesson 4: `test`

Run the `#Test` block in the edited source file:

```sh
jet test run.jet
```

To request the passing-test lines in the terminal, use the capture option:

```sh
jet test run.jet --capture=all
```

The canonical source prints:

```text
greet says hello: pass
1 passed, 0 failed, 0 skipped
```

Tests stay beside the source they exercise. Use `jet run` for the program's
normal output and `jet test` for its test blocks.

## 7. Lesson 5: repair an explicit entry

Make the source invalid in a recoverable way by replacing its contents with a
top-level statement:

```jet
print("before")
```

Check it:

```sh
jet check run.jet
```

Jet reports E0621 because an ordinary source file executes only through an
explicit `fn run`. Put the statement inside the entry function, then check
and run it:

```jet
fn run() {
    print("before")
}
```

```sh
jet check run.jet
jet run
```

The repaired program prints:

```text
before
```

The author chooses the execution boundary; Jet does not invent an implicit
entry function. `jet fix` is available for registered safe automatic fixes,
but it is not an implicit-run repair.

## 8. Lesson 6: `explain`

Ask Jet for the registered explanation of the diagnostic you just saw:

```sh
jet explain E0621
```

`jet explain` looks up the stable diagnostic code and prints its explanation.
The compiler diagnostic itself includes the actionable source location and
fix. Use the [diagnostic contract](../diagnostics.md) when you need the
registered What, Why, and Fix text, and use the command while working so that
the code and the source rule stay together.

Continue with the [examples index](../../../Examples/README.md) for executable,
golden-tested lessons.

## Terminal and editor state matrix

Keep the editor and terminal on the same source file. Bare `jet run` is the
beginner default; explicit files and workspace members are recovery and expert
controls.

| State | Editor | Terminal | Expected result or recovery |
|---|---|---|---|
| Install ready | Open a clean folder after `jet version` succeeds. | `jet new hello` | Jet creates `package.jet` and `run.jet`; continue in the new project directory. |
| Scaffolded | Open `run.jet`; the file contains a print-only `run` and a smoke `#Test`. | `jet run` | The generated program prints `hello, world`; replace it with the lesson source when you want the deterministic greeting above. |
| Valid edit | Save the changed `run.jet`; diagnostics are clear. | `jet check run.jet`, `jet test run.jet`, then `jet run` | Check, test, and run all use the same source. Editor run/test code lenses invoke the corresponding file commands. |
| Invalid edit | The diagnostic pane shows the stable code and fix. | `jet check run.jet` | Read the diagnostic, repair the file, and rerun the check. |
| Missing entry | Open the project directory and create or restore `run.jet`. | `jet run` | The diagnostic names `run.jet`; create it, or pass `jet run path/to/file.jet`. |
| Ambiguous project | Open the intended workspace member. | `jet run -p <member>` or `jet run path/to/run.jet` | Jet refuses an ambiguous bare run and tells you how to select one member or file. |
| Legacy layout | Open the migrated canonical file. | `jet run` | One retired `main.jet` layout is renamed to `run.jet` with a notice; a canonical-plus-retired pair must be reduced to one entry. |
| Offline or unsupported host | No editor path is usable until Jet is installed on a supported host. | Reconnect and finish install/fetch; on an unsupported release host, use the platform-specific track. | Do not continue with a partial install or replace the local workflow with a manual remote rebuild. |
| Learn next | Open the executable first-hour example. | `jet run Examples/features/basics/first_hour.jet` | Continue with the golden-tested example and then choose the next example from its index. |

When the terminal is not attached to a TTY, `jet ?` renders a static command
palette and `jet ? run` searches commands. `--color=never` or `NO_COLOR` keeps
captured output plain. Interactive TTY use adds safe command-prefill guidance;
it never runs the selected example for you.


## 9. Recover when the path breaks

| Situation | Recovery |
|---|---|
| The host is unsupported | Stop the release install path. Use the platform-specific project track; do not substitute SSH or a manual remote rebuild. |
| Installation fails | Verify that Nix and flakes work on the supported host, then repeat the install command. Do not continue with a partial install. |
| The first install is offline | Reconnect and repeat the install. The generated project has no registry dependency, but Jet must be installed before the first run. |
| `jet` is not found after install | Start a new shell and run `jet version`. If it still fails, repeat the Nix install check before creating a project. |
| `jet new` says the directory exists | Pick a new project name. The scaffold never overwrites a directory. |
| Bare `jet run` has no entry file | Read the diagnostic, create `run.jet` in the project, or use `jet run path/to/run.jet` for an explicit target. |
| A workspace has more than one possible project | Read the listed candidates, select one with `jet run -p <member>`, or pass the intended `run.jet` path. |
| The source is invalid | Run `jet check run.jet`, read the stable diagnostic code and fix, then use `jet explain <code>` to learn the rule. |
| An old project uses `pkg.jet`, `pack.jet`, `payload.jet`, or `jet.toml` | Rename the manifest to `package.jet`; Jet reports E1226 with that fix. |
| An old project uses `main.jet` | Run bare `jet run` in the project. Jet renames one retired entry to `run.jet` and prints a notice. If a canonical entry already exists, keep one and run again. |

## 10. Capstone: the first-hour shipping path

From a Jet checkout, run the executable, golden-tested capstone:

```sh
jet run Examples/features/basics/first_hour.jet
```

It prints the three first shipping steps:

```text
Shipping first-hour
[ok] check source
[ok] build binary
[ok] run smoke test
```

The matching expert path is
[`first_hour_expert.jet`](../../../Examples/features/basics/first_hour_expert.jet).
It demonstrates raw `process.argv()` while the flagship keeps the typed
`#CLI` entry short.
