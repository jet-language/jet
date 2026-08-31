# Install Jet and learn your first workflow

This guide teaches one complete beginner workflow on a supported host. The
lessons are ordered: `new` -> `run` -> `check` -> `test` -> `fix` -> `explain`.
The same commands work with a bare project target or with an explicit `.jet`
file when an expert needs to name the target.

## 1. Check the supported host

The measured release install path covers x86_64 Linux and x86_64 macOS. It
uses Nix flakes. Windows and other architectures are outside this release
install path, so do not replace it with an SSH session or a manual remote
rebuild.

If your host is in the supported set, check that Nix is available:

```bash
nix --version
```

If the command is missing, install Nix using its host instructions before
continuing. If the host is outside the supported set, stop here and use the
platform-specific project track instead of guessing at a compiler install.

## 2. Install Jet

Install the current Jet package into your user profile. The extra feature
setting makes the command work on Nix installations that have not enabled
flakes yet:

```bash
nix --extra-experimental-features "nix-command flakes" profile install github:jet-language/jet
jet version
```

The second command is the install check. If Nix reports an offline or network
failure, reconnect and repeat the install. If installation succeeds but the
shell cannot find `jet`, start a new shell and run `jet version` again. Do not
continue with a partial install.

Contributors can use a source build as an explicit expert path:

```bash
git clone https://github.com/jet-language/jet.git
cd jet
nix --extra-experimental-features "nix-command flakes" build
export PATH="$PWD/result/bin:$PATH"
jet version
```

## 3. Lesson 1: `new`

Choose a clean parent directory. `jet new` creates the canonical project
layout and uses `run.jet` as the default entry file:

```bash
mkdir -p "$HOME/jet-projects"
cd "$HOME/jet-projects"
jet new hello
cd hello
```

The scaffold contains:

```text
package.jet
run.jet
@run.jet
@build.jet
@dev.jet
@test.jet
.gitignore
```

The four `@*.jet` files are commented command-override homes. Ignore them for
the beginner path; they do not change the stock commands until you opt in.

The generated `run.jet` contains a small greeting, a typed `#CLI` entry, and
one smoke test. Its default name is `world`, so it prints:

```text
hello, world
```

If `hello` already exists, choose another project name. `jet new` does not
overwrite an existing directory.

## 4. Lesson 2: `run`

Run the generated program from the project directory:

```bash
jet run
```

The result is:

```text
hello, world
```

Bare `jet run` resolves the project's `run.jet`. The explicit form selects the
same source and is useful when a project has more than one possible target:

```bash
jet run run.jet
```

## 5. Lesson 3: `check`

Open `run.jet` in your editor and replace it with this small program. It is the
same source as the [canonical onboarding example](../examples/features/basics/onboarding/run.jet):

```jet
#CLI
struct GreetingArgs {
    #Doc("name to greet") name: String{"Jet"}
}

fn greet(name: String) String -> {
    return "hello, {name}"
}

#Test("greet says hello") {
    assert_eq(greet("Jet"), "hello, Jet")
}

fn run(args: GreetingArgs) { print(greet(args.name)) }
```

Check the source without running it:

```bash
jet check run.jet
```

`jet check` reports source errors without creating a binary. A successful check
means the source is ready for the next lesson; it does not run `fn run`.

## 6. Lesson 4: `test`

Run the `#Test` block in the same file:

```bash
jet test run.jet
```

The test report names the passing test and ends with a count. Tests are part of
the source workflow; they do not require a second project layout. Run the
program when you want its normal output:

```bash
jet run
```

## 7. Lesson 5: `fix`

Use your editor to make the source invalid in a recoverable way:

```jet
print("before")
fn run() { print("middle") }
print("after")
```

`jet check run.jet` reports E0621 because loose script statements already form
the entry body and the file also declares `fn run`. Apply the safe automatic
fix, then check the result:

```bash
jet fix run.jet
jet check run.jet
jet run
```

The fix moves the loose statements into the one `fn run` body. Read the change
in your editor before continuing. `jet fix` is a recovery tool for registered
safe fixes; it is not a replacement for checking ordinary typing mistakes.

## 8. Lesson 6: `explain`

Ask Jet to teach the diagnostic you just recovered from:

```bash
jet explain E0621
```

The explanation has the same three parts as the diagnostic: what it means,
why Jet enforces it, and how to fix it. This is the learning loop: `check`
names the problem, `fix` applies a safe repair when one exists, and `explain`
turns the stable code into a lesson.

Use the [examples index](../examples/README.md) for the next executable,
golden-tested lesson.

## Terminal and editor state matrix

Keep the editor and terminal on the same source file. The beginner default is
bare `jet run`; the explicit commands are recovery and expert control over the
same `run.jet`.

| State | Editor | Terminal | Expected result or recovery |
|---|---|---|---|
| Install ready | Open a clean folder after `jet version` succeeds. | `jet new hello` | Jet creates `package.jet` and `run.jet`; continue in the new project directory. |
| Scaffolded | Open `run.jet`; the file contains a typed `#CLI` entry and a smoke `#Test`. | `jet run` | The generated program prints `hello, world`. |
| Valid edit | Save the changed `run.jet`; diagnostics are clear. | `jet check run.jet`, `jet test run.jet`, then `jet run` | Check, test, and run all use the same source. VS Code/Cursor and Zed Run/Test code lenses call these commands for the active file. |
| Invalid edit | The diagnostic pane shows the stable code and fix. | `jet check run.jet` | Read `What`, `Why`, and `Fix`; repair the file, then rerun the check. |
| Missing entry | Open the project directory and create or restore `run.jet`. | `jet run` | The diagnostic names `run.jet`; create it, or pass `jet run path/to/file.jet`. |
| Ambiguous project | Open the intended workspace member. | `jet run -p <member>` or `jet run path/to/run.jet` | Jet refuses an ambiguous bare run and tells you how to select one member or file. |
| Legacy layout | Open the migrated canonical file. | `jet run` | One retired `main.jet` layout is renamed to `run.jet` with a notice; a canonical-plus-retired pair is an ambiguity that must be reduced to one entry. |
| Offline or unsupported host | No editor path is usable until Jet is installed on a supported host. | Reconnect and finish install/fetch; on an unsupported release host, use the platform-specific track. | Do not continue with a partial install or replace the local workflow with a manual remote rebuild. |
| Learn next | Open the executable first-hour example. | `jet run examples/features/basics/first_hour.jet` | Continue with the golden-tested example, then follow lesson #1034. |

The same recovery rules apply when the terminal is not attached to a TTY:
`jet ?` renders a static command palette, `jet ? run` searches commands, and
`--color=never` or `NO_COLOR` keeps captured output plain. Interactive TTY use
adds safe command prefill guidance; it never runs the selected example for you.

## 9. Recover when the path breaks

| Situation | Recovery |
|---|---|
| The host is unsupported | Stop the release install path. Use the platform-specific project track; do not substitute SSH or a manual remote rebuild. |
| Installation fails | Verify that Nix and flakes work on the supported host, then repeat the install command. Do not continue with a partial install. |
| The first install is offline | Reconnect and repeat the install. The generated project has no registry dependency, but Jet must be installed before the first run. |
| `jet` is not found after install | Start a new shell and run `jet version`. If it still fails, repeat the Nix install check before creating a project. |
| `jet new` says the directory exists | Pick a new project name. The scaffold never overwrites a directory. |
| Bare `jet run` has no entry file | Read E2104/E2105, create `run.jet` in the project, run it in the generated project, or use `jet run path/to/run.jet` for an explicit target. |
| A workspace has more than one possible project | Read the listed candidates, select one with `jet run -p <member>`, or pass the intended `run.jet` path. |
| The source is invalid | Run `jet check run.jet`, read the stable diagnostic code and fix, then use `jet explain <code>` to learn the rule. |
| An old project uses `pkg.jet`, `pack.jet`, `payload.jet`, or `jet.toml` | Rename the manifest to `package.jet`; Jet reports E1226 with that fix. |
| An old project uses `main.jet` | Run bare `jet run` in the project. Jet renames one retired entry to `run.jet` and prints a notice. If a canonical entry already exists, keep one and run again. |

Do not teach SSH plus a manual rebuild as the remote path. Installation ends
when a local supported host runs the project. Later remote updates use
`jet deploy`.

## 10. Capstone: first-hour shipping path

From a Jet checkout, run the executable, golden-tested capstone:

```bash
jet run examples/features/basics/first_hour.jet
```

It checks the three first shipping steps and reports each one. The matching
expert path is
[first_hour_expert.jet](../examples/features/basics/first_hour_expert.jet).
