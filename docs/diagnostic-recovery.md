# Diagnostic recovery exercises

Use these exercises after the first successful `jet run`. Each exercise uses
the same source, the same `run.jet` entry, and the same recovery loop:

```text
jet check run.jet
jet explain <code>
edit run.jet
jet test run.jet
jet run
```

`jet check` finds source problems without running the program. The diagnostic
code is stable. `jet explain` shows the full What, Why, and Fix row.

## 1. Fix invalid code

Create a project and enter it:

```bash
jet new recovery
cd recovery
```

Change `print` to `pirnt` in `run.jet`. Check the file:

```bash
jet check run.jet
```

Read `E0102`. Then read its full page:

```bash
jet explain E0102
```

Change `pirnt` back to `print`. Run the test and the program:

```bash
jet test run.jet
jet run
```

The same diagnostic is pinned in
[`tests/ui/unknown_function.jet`](../tests/ui/unknown_function.jet) and its
snapshot. The first-hour source example is
[`examples/features/basics/hello.jet`](../examples/features/basics/hello.jet).

## 2. Recover a missing entry

Move the default entry for one check:

```bash
mv run.jet saved.jet
jet run
```

The error names `run.jet` and gives the recovery. Run the saved file with an
explicit target, then restore the default entry:

```bash
jet run saved.jet
mv saved.jet run.jet
jet run
```

Bare `jet run` is the beginner path. `jet run <file.jet>` is the explicit
target when a project is incomplete or the source location is unclear.

## 3. Select an ambiguous project

When a workspace has more than one runnable member, bare `jet run` stops and
names the member choices. Select one member:

```bash
jet run -p <member>
```

You can also run its entry file directly:

```bash
jet run path/to/member/run.jet
```

Do not guess. A named member or an explicit `run.jet` keeps the source choice
visible.

## 4. Recover old layouts

The current package manifest name is `package.jet`. If an old project has
`pkg.jet`, `pack.jet`, `payload.jet`, or `jet.toml`, rename the file:

```bash
mv pkg.jet package.jet
jet run
```

Jet reports `E1226` when it finds a retired manifest name. If the old project
has one retired `main.jet`, bare `jet run` renames it to `run.jet` and prints a
notice. If both `main.jet` and `run.jet` exist, remove or move one, then run
again. Jet does not choose between two project entries.

## 5. Recover install and host failures

The release install path supports x86_64 Linux and x86_64 macOS with Nix
flakes. Check the host and installer before creating a project:

```bash
uname -s
uname -m
nix --version
nix --extra-experimental-features "nix-command flakes" profile install github:jet-language/jet
jet version
```

If the host is outside the supported set, stop and use the platform-specific
project track. Do not replace a failed install with SSH or a manual remote
rebuild.

If the install fails because the network is offline, reconnect and repeat the
install command. If the install succeeds but `jet` is not found, start a new
shell and run `jet version`. Do not run a partial install.

The generated project has no registry dependency. The first install still
needs its package source. Finish the install before the first project run.

## Recovery rule

Keep one source of truth:

- `run.jet` is the default project entry.
- `jet check` is the source diagnosis command.
- `jet explain <code>` is the diagnostic lesson.
- `jet test` checks the repaired source.
- `jet run` proves the repaired source.
- An explicit file or `-p <member>` is the recovery target when resolution is
  not unique.

Read [diagnostic law](spec/diagnostics.md) for the stable error contract and
the [first-hour guide](first-hour.md) for the install and scaffold path.
