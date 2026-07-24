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

No report upload command is shipped. A future send operation needs a separate
ratified decision for its destination, transport, authentication, and exact
privacy boundary.
