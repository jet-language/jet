# Probe area-cli — CLI tools, scripts, automation

Read `~/.cache/jet-dev/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: cli.

## Build this

A `notes` command-line tool as one Jet package: subcommands with typed args and help; reads and writes files under a config dir; an ignore-aware recursive search (respecting .gitignore) with colored, aligned output; a pipeline that spawns a process, reads its stdout as lines, filters, and writes to another process's stdin; environment and exit codes; a typed object pipeline (records flowing between stages, not text); a `#Job` that runs a scheduled task; a single-file script mode run; and a release build installed to a bin dir. Start from Examples/features/cli/**, Examples/features/io/** (cli_args, grep_scan, walk_files, process_builder, stdin_*, terminal, log_*), Examples/features/script_mode/**, Examples/features/tooling/**.

## Answer these

1. Can a library author build a PowerShell-style typed object pipeline and a ripgrep-class search with today's primitives (iteration protocol, process streams, Unicode)?
2. How much boilerplate does a subcommand with ten flags cost?
3. What does a CLI battery need?

## Research to mine

Domain census and reports: `~/.cache/jet-dev/dx2/shell-sysadmin/`, `~/.cache/jet-dev/dx2/text-processing-parsing/`, `~/.cache/jet-dev/dx2/build-systems-monorepo/`, `~/.cache/jet-dev/dx2/devops-iac-cloud/`, `~/.cache/jet-dev/dx2/testing-qa-automation/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-dev/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-dev/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-dev/dx3/area-cli/probe.md`, `gaps.json`, `batteries.json`, and the code under `~/.cache/jet-dev/dx3/area-cli/pkg/`. Gap ids start with `area-cli-G`.
