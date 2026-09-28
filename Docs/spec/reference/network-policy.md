# Toolchain network and telemetry policy

Jet sends no telemetry. This is the durable policy identified by
`D-TELEMETRY1=A`. This page is for users deciding when a command may dial out
and for operators reviewing a local report. The executable policy is in
[`Source/CmdReport.rs`](../../../Source/CmdReport.rs),
[`Source/Doctor.rs`](../../../Source/Doctor.rs), and
[`tests/report.rs`](../../../tests/report.rs).

The compiler and package tools do not collect or transmit command use, build
times, crashes, source facts, environment values, or machine identifiers. They
do not add telemetry to another request. A remote server can still observe
transport facts such as a source IP address; Jet does not attach command
history, a machine ID, an environment snapshot, or unrelated package data.

## User-requested network operations

Network access is tied to an operation the user requested:

- Registry and package operations send the package name, version constraint,
  and protocol facts that the requested operation needs.
- `jet self doctor` stays offline unless the user writes `--online`.
- A build may use a network effect only when its declared build work requires it
  and the user grants that effect, for example `--allow=Net`.
- A program that the user runs may use its own declared network effects.

Ordinary local commands such as `jet check`, `jet build`, and `jet fmt` do not
open a network connection merely because they are invoked. A missing grant or
an undeclared effect is an error, not permission to broaden the command's
network access.

## Inventory of toolchain network paths

The following toolchain paths may dial out, and only when their trigger is
explicit:

| Path | Trigger | What it may send |
|---|---|---|
| `jetpack` / `jet` registry fetch (`Provider/fetch`, sparse index, script registries) | user-requested add/update/fetch/vendor | package name, version constraint, HTTPS URL for that artifact, and protocol headers required by the fetch |
| `jetpack doctor` / `jet self doctor --online` | explicit `--online` | TCP reachability probe to the configured registry host |
| Recipe `fetch(url, sha256:)` during a build | declared locked fetch in the recipe | bytes from the named URL |
| Build with `--allow=Net` and user-program network effects | user opt-in or program code | only what that build step or program declares |

There is no background usage, crash sender, or analytics sender in `jet` or
`jetpack`.

## Local reports

`jet report` creates a private bundle under:

```text
.jet/reports/<content-hash>/
```

The command is explicit and local. It does not send the bundle. **There is no `jet report --send` command.** `D-REPORT-SEND1=A` keeps sharing outside Jet: after inspection, a user may attach the two text files through a support channel they already trust. Jet never chooses a destination, account, or retention policy for a report.

The bundle contains:

- `README.txt`, which lists included and excluded data;
- `report.txt`, which records Jet version, edition, compiler target,
  operating-system family, architecture, and the zero-telemetry policy.

The bundle excludes source code, paths, the current directory, arguments,
environment values, hostname, username, machine identifiers, network
addresses, crash data, and package names. On Unix, Jet sets the directory to
mode `0700` and each file to mode `0600`. Reusing an unchanged bundle restores
those private modes before Jet accepts it.

The content hash makes the path repeatable for identical report bytes. Jet
writes a private staging directory first and exposes the content-hash path only
after both files are complete. Linked directories, linked bundle files, and
changed existing bundles are rejected rather than followed or overwritten.
