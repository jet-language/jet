# Diagnostic recovery exercises

This guide is for a Jet user whose project no longer checks or resolves its
entry. Each exercise uses the real CLI and keeps one source of truth:
`run.jet`, the diagnostic registry in
[`docs/spec/diagnostics.md`](../diagnostics.md), and the recovery cases in
[`tests/onboarding_recovery.rs`](../../../tests/onboarding_recovery.rs). Start
after one successful `jet run` from the
[first-hour guide](first-hour.md).

Use this loop for a source error:

```text
jet check run.jet
jet explain <code>
edit run.jet
jet test run.jet
jet run
```

`jet check` diagnoses source without running the program. The compiler prints a
stable diagnostic code with its source location and fix. `jet explain` looks up
the registered explanation for that code.

## 1. Fix an unknown function

Create a project and enter it:

```sh
jet new recovery
cd recovery
```

Open `run.jet` and change the call `print` to the misspelled `pirnt`. Check the
file:

```sh
jet check run.jet
```

The diagnostic is E0102. Ask for the registered explanation:

```sh
jet explain E0102
```

Change `pirnt` back to `print`, then test and run the repaired source:

```sh
jet test run.jet
jet run
```

The same diagnostic has an executable fixture in
[`tests/ui/unknown_function.jet`](../../../tests/ui/unknown_function.jet) and a
matching UI snapshot. The small first-contact program is
[`examples/features/basics/hello.jet`](../../../examples/features/basics/hello.jet).

## 2. Recover a missing entry

Move the default entry aside so that the resolver cannot find it:

```sh
mv run.jet saved.jet
jet run
```

Read the diagnostic. It names `run.jet` and gives the recovery. An explicit
file target lets you run the saved source while the default entry is absent:

```sh
jet run saved.jet
mv saved.jet run.jet
jet run
```

Bare `jet run` is the beginner path. `jet run <file.jet>` names the source
when a project is incomplete or its location is unclear.

## 3. Select an ambiguous project

A workspace with more than one runnable member cannot choose for you. Read the
member names in the diagnostic, then select one by name:

```sh
jet run -p <member>
```

You can also name the entry file directly:

```sh
jet run path/to/member/run.jet
```

Do not guess. A named member or an explicit `run.jet` keeps the source choice
visible and makes the recovery repeatable.

## 4. Migrate an old layout

The current package manifest is `package.jet`. If an old project uses
`pkg.jet`, `pack.jet`, `payload.jet`, or `jet.toml`, rename that file and run
again:

```sh
mv pkg.jet package.jet
jet run
```

Jet reports E1226 when it finds a retired manifest name and points to
`package.jet`. A project with one retired `main.jet` is migrated to `run.jet`
by bare `jet run`, with a notice. If both `main.jet` and `run.jet` exist, move
one aside or remove the duplicate before running again; Jet must not choose
between two project entries.

## 5. Recover install and host failures

The release install path supports x86_64 Linux and x86_64 macOS with Nix
flakes. Check the host and installer before creating a project:

```sh
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

The generated project has no registry dependency, but Jet itself must be
installed before the first project run. Finish the install before creating a
project or diagnosing its source.

## Recovery rule

Keep one source of truth for each kind of action:

| Need | Command or target |
|---|---|
| Default project entry | `run.jet` |
| Source diagnosis | `jet check run.jet` |
| Diagnostic lesson | `jet explain <code>` |
| Repaired-source test | `jet test run.jet` |
| Repaired-source run | `jet run` |
| Non-unique resolution | An explicit file or `jet run -p <member>` |

If a recovery changes a file name, return to the canonical layout before
continuing. Read the [diagnostic contract](../diagnostics.md) for the stable
error rules and the [first-hour guide](first-hour.md) for installation and
scaffolding.
