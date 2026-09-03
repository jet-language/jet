# CLI tools, scripts, and automation probe

## What I built

I built a `notes` package with typed root flags, seven subcommands, file storage, search, process control, typed records, and a scheduled job. I also built standalone probes for single-file scripts, process pipeline behavior, and the missing stdin close operation. The package builds as a release executable and the executable runs from a scratch `bin` directory.

Files:

- `pkg/package.jet` — package name and authority.
- `pkg/run.jet` — `#CLI(Standard)` program, search, file I/O, process, data, and `#Job` code.
- `pkg/data/.gitignore`, `keep.txt`, `ignored/skip.txt`, `private.secret` — search fixtures.
- `script.jet` — top-level single-file script.
- `pipeline_builtin.jet` — all-native `process.pipeline` workaround.
- `pipe_head.jet` — bounded-consumer stdin workaround.
- `stdin_close.jet` — close-operation probe.

## What worked

- `scripts/agent/jet-env jet check .../pkg` — passed; the compiler proved run-entry resolution, module graph, and AOT/JIT lowering. It emitted only warnings.
- Typed root CLI and generated commands — `jet run .../pkg` printed root options, command summaries, defaults, `--help`, and `--version`.
- Ten typed options — `jet run .../pkg -- flags --one A --two B --three C --four D --five E --six F --seven G --eight H --nine I --ten J` printed `A,B,C,D,E,F,G,H,I,J`.
- Command help — `jet run .../pkg -- flags --help` listed all ten `--one` through `--ten` options and their defaults.
- File configuration — `.../notes --config-dir .../data/release-config-2 config` wrote and read `color=auto` and `format=aligned`.
- Environment precedence — a `NOTES_CONFIG` run wrote under the environment path; a simultaneous `--config-dir` run wrote only under the flag path.
- Basic recursive regex search — the search command found two `keep.txt` lines and skipped `ignored/skip.txt` and `private.secret` under the simple fixture rules.
- Aligned color — release output with `--color always` contains `1b 5b 33 36 6d` (cyan ANSI), while `--color never` contains no `1b` bytes.
- Environment and exit receipts — `.../notes env release-value` printed `overlay=release-value` and `exit=1 success=false`.
- Typed records — `.../notes objects work` printed `alpha work a.md`, `gamma work c.md`, and `records=2`; the stages keep `NoteRow` values after CSV decode.
- All-native process pipeline — `jet run .../pipeline_builtin.jet` printed `exit=0 output=keep-one\nkeep-two`.
- Scheduled job — `jet run .../pkg -- refresh` printed `job=refresh`; `jet jobs` from the package listed `refresh [dev]` and `every 1s`.
- Single-file script — `jet run .../script.jet` printed `single-file script` and `helper=42`.
- Release build — `jet build .../pkg --release` built `build/run` in 29.8 seconds and reported `stripped 1 dev job(s): refresh`; copying it to `bin/notes` and running `notes flags` succeeded.

## Gaps

### area-cli-G1 — `impossible`

A spawned child has `ProcessStdin.write` but no closable stdin. `scripts/agent/jet-env jet check .../stdin_close.jet` reports `E0102: ProcessStdin has no method close` at `stdin_close.jet:5`. The matching producer-to-`cat` code in `pkg/run.jet:75-85` timed out after 60 seconds at `consumer.wait`, because `cat` never received EOF. `pipe_head.jet:4-11` exits 0 only because `head` stops after two lines. This blocks arbitrary in-process filtering followed by a child consumer, and is relevant to CLI automation.

### area-cli-G2 — `boilerplate`

`core.files` exposes `walk` and `walk_files` but no ignore-aware walk (`docs/reference/core-library.md:437-438`). The package reads one root `.gitignore` and hand-matches only three forms (`pkg/run.jet:15-21`, `45-60`). With `ignored/`, `*.secret`, and `!private.secret`, `jet run .../pkg -- search jet .../data` prints `matches=2` and omits `private.secret`; the matcher is not Git-compatible. A library author must write and maintain the full nested, anchored, negation, and directory-rule matcher.

### area-cli-G3 — `call-site`, `boilerplate`

A PowerShell-style object edge is missing. The working record command (`jet run .../pkg -- objects work`) must capture `ProcessReceipt.output` as `String`, declare `#Codable NoteRow`, call `data.csv<NoteRow>`, then write `data.filter` and `data.sort_by` closures (`pkg/run.jet:99-107`). `ProcessReceipt.output` is `String` (`docs/reference/core-library.md:1743-1745`), so native commands do not enter a typed property-binding pipeline. Ordinary typed records work; each external command still needs a schema, decoder, and adapter.

## Friction

- A ten-flag subcommand needs ten declaration lines (`pkg/run.jet:112-121`), plus one signature, one body print, and documentation. There is no handwritten parser, so this is light author boilerplate.
- The search implementation needs 23 lines for root-file loading and matching (`pkg/run.jet:15-21`, `45-72`) and still lacks full `.gitignore` semantics.
- The typed object workaround needs one record declaration, one captured process, one decode call, and two stage calls. It is safe and readable, but users do not get automatic property binding.
- A visible dev `#Job` claims the first `--help` word for job help. `jet run .../pkg -- --help` prints `Usage: run <job> [options]`; bare invocation prints the root CLI help. The job precedence is documented, so I do not count it as a defect.
- The release workflow prints a named `build/run` artifact. Installing it into a custom `bin` directory required an explicit `cp`; `--output` is a named-output selector, not a path.
- `jet check` reports hidden-cost warnings for repeated string copies in the process loop and ignored-rule loop. No speed gap is claimed because this probe did not compare an incumbent on the same input.

## Defects

No independent Jet defect was found on the successful paths. The stdin close failure is recorded as area-cli-G1 because it is a missing capability, not a wrong result.

## Battery notes

- `typed-cli` — root help, defaults, command summaries, ten options, command-local help, environment precedence, malformed input, and exit status; compiler/tooling surface, not library-only.
- `config-files-and-search` — `Path`, file writes, append, regex, recursive walk, aligned output, color modes, nested fixtures, and negation fixtures; pure library code.
- `process-streams` — argv, line reads, stdin writes, EOF, native pipeline, receipts, nonzero exits, limits, and bounded consumers; pure library code.
- `typed-record-flow` — process output, `#Codable` CSV decode, typed fields, filter, sort, and output; pure library code.
- `environment-and-exit` — Env-backed fields, child overlays, exact exit codes, and `success=false`; pure library code.
- `jobs-scripts-and-release` — job inventory and invocation, script mode, release build, install, and binary smoke; compiler/tooling surface, not library-only.

## Verdict

Buildable today for typed CLI tools, local files, environment overlays, jobs, scripts, and release binaries.

Basic recursive search works, but ripgrep-class ignore behavior needs area-cli-G2.

Ordinary typed record pipelines work, but PowerShell-style native property binding needs area-cli-G3.

Arbitrary filtered child pipelines are blocked by area-cli-G1; fixed consumers and all-native pipelines work.

No performance claim was made; this probe measured capability and call-site cost only.
