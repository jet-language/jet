# Systems

Files, processes, the terminal, time and command-line programs.

Run a lesson from the repository root, for example `jet run Examples/Systems/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [ByteBuffer](ByteBuffer.jet)
- [IOPrelude](IOPrelude.jet) — The readable Core prelude opens its closed ambient surface without a `use` line.
- [IOSurface](IOSurface.jet) — Qualified Core IO remains available for #NoPrelude and explicit IO work.
- [Log](Log.jet) — core.log — structured logging with levels, fields, spans and sinks.
- [LogHuman](LogHuman.jet) — Human-readable log format.
- [LogJob](LogJob.jet) — Composed flagship: file → pattern → aggregate.
- [LogTraceExport](LogTraceExport.jet) — CORE-F023: one finite exported-trace job through `core.log`.
- [NoPrelude](NoPrelude.jet)
- [Stream](Stream.jet)

## CLI

- [AppConfig](CLI/AppConfig.jet) — One declaration, one call, one typed config value.
- [ArgsAudit](CLI/ArgsAudit.jet) — One ArgsSpec model covers builder-driven CLIs with short flags, typed options, defaults, repeated values, subcommands, suggestions, and completion text.
- [ArgsSpec](CLI/ArgsSpec.jet) — Declarative CLI arg parsing.
- [CLIArgs](CLI/CLIArgs.jet) — `jet run file.jet -- arg1 arg2` forwards everything after `--` to the program verbatim.
- [DirectLaunch](CLI/DirectLaunch.jet)
- [Positionals](CLI/Positionals.jet) — Required command values fill from bare argv in declaration order.
- [ScriptMode](CLI/ScriptMode.jet)
- [Subcommands](CLI/Subcommands.jet) — One program struct owns root flags and callable commands.
- [TypedEntryArgs](CLI/TypedEntryArgs.jet) — The entry function's typed parameter IS the CLI spec.

## Files

- [Archive](Files/Archive.jet) — core.archive owns zip/tar containers only.
- [CompressGzip](Files/CompressGzip.jet) — Canonical gzip stream codec.
- [CompressZstdStream](Files/CompressZstdStream.jet)
- [DirEntry](Files/DirEntry.jet) — fs.list_dir returns [DirEntry] ({name, path, is_dir}).
- [Files](Files/Files.jet)
- [FilesDepth](Files/FilesDepth.jet) — Std-backed filesystem depth beyond read/write/list_dir.
- [FilesProjection](Files/FilesProjection.jet) — Temporary paths make directory, mode, append, and rename operations explicit.
- [FilesReaderSeek](Files/FilesReaderSeek.jet)
- [FilesRootedLifecycle](Files/FilesRootedLifecycle.jet) — Rooted file and temporary-resource lifecycle (CORE-F010, SCRIPT-F13).
- [FilesSetMode](Files/FilesSetMode.jet) — `files.set_mode` replaces a path's permission bits and `files.stat` reads them back; a missing path is a typed `IOError`.
- [FilesStatLinks](Files/FilesStatLinks.jet)
- [FilesStorePrimitives](Files/FilesStorePrimitives.jet)
- [Path](Files/Path.jet) — Typed Path API — construction, decomposition, and directory listing.
- [PathWithin](Files/PathWithin.jet) — Lexical containment compares normalized components.
- [ReadBytesPaths](Files/ReadBytesPaths.jet)
- [WalkFiles](Files/WalkFiles.jet) — File-only traversal keeps the walk policy and drops directories.
- [Watcher](Files/Watcher.jet) — core.watcher owns file/process/port watch handles.

## Process

- [EnvOverlay](Process/EnvOverlay.jet)
- [OSFacts](Process/OSFacts.jet) — System facts live in core.sys; core.sys stays environment vars.
- [OSProcessControl](Process/OSProcessControl.jet)
- [ProcessBuilder](Process/ProcessBuilder.jet)
- [ProcessExitCleanup](Process/ProcessExitCleanup.jet) — Explicit process termination runs the one cleanup boundary.
- [ProcessPassDirectory](Process/ProcessPassDirectory.jet)
- [ProcessRunAll](Process/ProcessRunAll.jet) — process.run_all runs a list of specs through a bounded worker pool.
- [ProcessStdinClose](Process/ProcessStdinClose.jet) — A spawned child owns an explicit closeable stdin pipe.
- [ResourceLimits](Process/ResourceLimits.jet) — This program's resource limits and usage live in core.sys.
- [SysExecutable](Process/SysExecutable.jet) — `os.executable()` names the running program: the script's absolute path under `jet run`, the built binary after `jet build`.
- [SystemPrivilegedEntry](Process/SystemPrivilegedEntry.jet) — Audited identity/session controls for a disposable user namespace only.
- [SystemProjection](Process/SystemProjection.jet) — Process directories, timestamps, signals, and child reaping.

## Terminal

- [GrepScan](Terminal/GrepScan.jet) — Parallel directory queue + regex file scan.
- [Progress](Terminal/Progress.jet) — io.progress wraps a List/Iter and updates as the loop pulls.
- [ReadSentinel](Terminal/ReadSentinel.jet) — Pull one stdin line per turn and stop on the blank sentinel.
- [StdinFilter](Terminal/StdinFilter.jet) — Streaming line-by-line stdin.
- [StdinInput](Terminal/StdinInput.jet) — `input()` reads one line from standard input.
- [StdinTTY](Terminal/StdinTTY.jet) — Every standard stream answers whether it is a terminal.
- [StdoutBytes](Terminal/StdoutBytes.jet) — `Stdout.write_bytes` sends raw bytes to standard output.
- [StdoutFlush](Terminal/StdoutFlush.jet) — Standard output is buffered when it goes to a pipe or a file: bytes leave when the buffer fills or the program ends.
- [Terminal](Terminal/Terminal.jet) — terminal direct-input control.
- [TerminalParity](Terminal/TerminalParity.jet) — Terminal facts, streams, and interactive prompts.

## Time

- [Dates](Time/Dates.jet)
- [DateTime](Time/DateTime.jet)
- [DateTimeParts](Time/DateTimeParts.jet) — A DateTime answers its calendar parts directly; `weekday()` counts from Sunday = 0, the same as `LocalDate.weekday()`.
- [DurationAddSub](Time/DurationAddSub.jet)
- [DurationFacts](Time/DurationFacts.jet) — Fixed values use exact duration literals.
- [DurationNs](Time/DurationNs.jet)
- [Expiring](Time/Expiring.jet) — core.time.expiring expired and remaining_ms are Core source: a deadline is still valid at its own tick, expires on the next one, and the remaining time…
- [InstantPoint](Time/InstantPoint.jet) — `time.instant()` is the compiler-owned monotonic Time point on every tier.
- [TimeCalendar](Time/TimeCalendar.jet)
- [TimeOrder](Time/TimeOrder.jet)
- [TimeRender](Time/TimeRender.jet)
