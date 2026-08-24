# Install Jet and run your first project

This guide covers the beginner path from a supported-host install to the
first successful `jet run`. It ends there. Ordered lessons continue in #1034;
diagnostic recovery exercises are in #1035.

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

## 3. Create and run the first project

Choose a clean parent directory. `jet new` creates the canonical project
layout and uses `run.jet` as the default entry file:

```bash
mkdir -p "$HOME/jet-projects"
cd "$HOME/jet-projects"
jet new hello
cd hello
jet run
```

The scaffold contains:

```text
package.jet
run.jet
.gitignore
```

The generated `run.jet` contains a zero-argument `fn run()` and prints:

```text
hello, world
```

Bare `jet run` resolves `run.jet`. You can name the file explicitly when you
need to make the target clear:

```bash
jet run run.jet
```

If `hello` already exists, choose another project name. `jet new` does not
overwrite an existing directory.

## 4. Edit, check, and test

Open `run.jet` in your editor and change the message:

```jet
fn run() {
    print("hello from Jet")
}
```

Run the same source through the explicit development checks:

```bash
jet check run.jet
jet test run.jet
jet run run.jet
```

`jet check` reports source errors without running the program. `jet test` runs
any `#Test` blocks in the file. `jet run` compiles and runs the entry function.
These commands are expert escapes from the zero-ceremony `jet run` default;
they do not require a different project layout.

The small executable hello example is also an
[executable, golden-tested example](../examples/features/basics/hello.jet).
Use the [examples index](../examples/README.md) when the first run is complete.

## 5. Recover when the path breaks

| Situation | Recovery |
|---|---|
| `jet` is not found after install | Start a new shell and run `jet version`. If it still fails, repeat the Nix install check before creating a project. |
| `jet new` says the directory exists | Pick a new project name. The scaffold never overwrites a directory. |
| Bare `jet run` has no entry file | Run it in the generated project, or use `jet run path/to/run.jet` for an explicit target. |
| A workspace has more than one possible project | Select the member with `jet run -p <member>`, or pass the intended `run.jet` path. |
| The source is invalid | Run `jet check run.jet`, read the stable diagnostic code and fix, then use the recovery exercises in #1035. |
| The first install is offline | The generated project has no registry dependency, but the installer still needs its package source. Reconnect and finish installation before the first run. |
| An old project uses `pkg.jet`, `pack.jet`, `payload.jet`, or `jet.toml` | Rename the manifest to `package.jet`; Jet reports E1226 with that fix. |
| An old project uses `main.jet` | Run bare `jet run` in the project. Jet renames one retired entry to `run.jet` and prints a notice. If a canonical entry already exists, keep one and run again. |

Do not teach SSH plus a manual rebuild as the remote path. Installation ends
when a local supported host runs the project. Later remote updates use
`jet deploy`.

## Continue learning

After the first `hello, world`, continue with the ordered lessons in #1034.
When a diagnostic needs practice, use the diagnostic-led exercises in #1035
instead of repeating this install walkthrough.
