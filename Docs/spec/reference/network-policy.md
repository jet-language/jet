# Toolchain network and telemetry policy

Jet sends no telemetry. This is the durable policy identified by
`D-TELEMETRY1=A`. This page is for users deciding when a command may dial out.
The executable policy is in [`Source/Doctor.rs`](../../../Source/Doctor.rs),
and [`tests/report.rs`](../../../tests/report.rs) audits it.

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
