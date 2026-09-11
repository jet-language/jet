# Toolchain network and telemetry policy

Jet sends no telemetry. This is a permanent policy under ratified decision
`D-TELEMETRY1=A`.

The compiler and package tools do not collect or transmit command use, build
times, crashes, source facts, environment values, or machine identifiers.
They do not add telemetry to another request.

## When the toolchain can use the network

Network access happens only for an operation that the user requested:

- Registry and package operations send the package name, version constraint,
  and protocol facts that the operation needs.
- `jet self doctor` stays offline unless the user writes `--online`.
- A build can use network access only when its declared build work requires it
  and the user writes `--allow-net`.
- A program that the user runs can use its own declared network effects.

A remote server can still observe transport facts such as the source IP
address. Jet does not attach command history, a machine ID, an environment
snapshot, or unrelated package data.

Ordinary local commands such as `jet check`, `jet build`, and `jet fmt` do not
open a network connection.

## Inventory of toolchain network paths

These are the only Jet/jetpack paths that may dial out, and only when the user
asks for that operation:

| Path | Trigger | What it may send |
|---|---|---|
| `jetpack` / `jet` registry fetch (`Provider/fetch`, sparse index, script registries) | user-requested add/update/fetch/vendor | package name, version constraint, HTTPS URL for that artifact, protocol headers the fetch needs |
| `jetpack doctor` / `jet self doctor --online` | explicit `--online` | TCP reachability probe to the configured registry host |
| Recipe `fetch(url, sha256:)` during a build | declared locked fetch in the recipe | the named URL bytes |
| Build/`--allow-net` and user program network effects | user opt-in or program code | only what that build step or program declares |

There is no background usage, crash, or analytics sender in `jet` or `jetpack`.

## Local reports

`jet report` creates a private bundle under:

```text
.jet/reports/<content-hash>/
```

The command is explicit and local. It does not send the bundle.

The bundle contains two readable text files:

- `README.txt` lists included and excluded data.
- `report.txt` contains Jet version, edition, compiler target, operating-system
  family, architecture, and the zero-telemetry policy.

The bundle excludes source code, paths, the current directory, arguments,
environment values, hostname, username, machine identifiers, network
addresses, crash data, and package names. On Unix, Jet sets the directory to
mode `0700` and each file to mode `0600`. Reusing an unchanged bundle restores
those exact private modes before Jet accepts it.

The content hash makes the path repeatable. The same Jet binary on the same
platform produces the same path and bytes. Jet writes a private staging
directory first. The content-hash path becomes visible only after both files
are complete. Jet rejects linked directories, linked bundle files, and changed
existing bundles instead of following or overwriting them.

## Sharing stays outside Jet

Ratified `D-REPORT-SEND1=A` keeps sharing outside Jet. There is no `jet report --send` command.
After you inspect the bundle, attach the two text files through a support
channel you already trust. Jet never chooses a destination, account, or
retention policy for a report.
