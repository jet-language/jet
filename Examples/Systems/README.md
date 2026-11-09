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
- [FilesAccess](Files/FilesAccess.jet) — `files.can_read`, `can_write` and `can_execute` ask whether this program may use a path now, judged with its effective ids; a missing path is false.
- [FilesDepth](Files/FilesDepth.jet) — Std-backed filesystem depth beyond read/write/list_dir.
- [FilesProjection](Files/FilesProjection.jet) — Temporary paths make directory, mode, append, and rename operations explicit.
- [FilesMapAdvise](Files/FilesMapAdvise.jet) — `mapped.advise(advice)` tells the system how a mapped file's bytes will be read (posix_madvise); the bytes stay the same.
- [FilesReaderSeek](Files/FilesReaderSeek.jet)
- [FilesRootedLifecycle](Files/FilesRootedLifecycle.jet) — Rooted file and temporary-resource lifecycle (CORE-F010, SCRIPT-F13).
- [FilesReserve](Files/FilesReserve.jet) — `writer.allocate(size)` reserves disk space for a file (posix_fallocate) and `reader.advise(advice)` tells the system how the file will be read (posix_fadvise).
- [FilesSetMode](Files/FilesSetMode.jet) — `files.set_mode` replaces a path's permission bits and `files.stat` reads them back; a missing path is a typed `IOError`.
- [FilesSpace](Files/FilesSpace.jet) — `files.space` reports a file system's total, free and available bytes (statvfs) and `df`'s used percent; a missing path is a typed `IOError`.
- [FilesStatLinks](Files/FilesStatLinks.jet)
- [FilesStorePrimitives](Files/FilesStorePrimitives.jet)
- [Path](Files/Path.jet) — Typed Path API — construction, decomposition, and directory listing.
- [PathWithin](Files/PathWithin.jet) — Lexical containment compares normalized components.
- [ReadBytesPaths](Files/ReadBytesPaths.jet)
- [WalkFiles](Files/WalkFiles.jet) — File-only traversal keeps the walk policy and drops directories.
- [WalkOrder](Files/WalkOrder.jet) — `walk` yields the entries in path order, depth first, without sorting the whole tree.
- [Watcher](Files/Watcher.jet) — core.watcher owns file/process/port watch handles.

## Process

- [EnvOverlay](Process/EnvOverlay.jet)
- [OSCPUTime](Process/OSCPUTime.jet) — `core.sys.cpu_time` reads the CPU time of any process; `set_wall_clock` sets the machine's wall clock under `#Unsafe`, which needs privilege.
- [OSFacts](Process/OSFacts.jet) — System facts live in core.sys; core.sys stays environment vars.
- [OSIDs](Process/OSIDs.jet) — `core.sys.user_ids` reads the real, effective and saved IDs and `set_user_ids` changes them under `#Unsafe`; `users` and `groups` read /etc/passwd and /etc/group as musl does.
- [OSLockMemory](Process/OSLockMemory.jet) — `core.sys.lock_memory` keeps the program's pages in RAM (mlockall) until it ends; without privilege the locked-memory limit may refuse it.
- [OSMknod](Process/OSMknod.jet) — `core.sys.mknod` makes a character or block device file under `#Unsafe`; only a privileged process may.
- [OSProcessControl](Process/OSProcessControl.jet)
- [OSScheduling](Process/OSScheduling.jet) — `core.sys.scheduling` reads a process's CPU policy and `set_scheduling` changes it under `#Unsafe`; real time needs privilege.
- [ProcessAffinity](Process/ProcessAffinity.jet) — `.cpu_affinity(cpus)` runs the child only on the listed CPUs.
- [ProcessBuilder](Process/ProcessBuilder.jet)
- [ProcessExitCleanup](Process/ProcessExitCleanup.jet) — Explicit process termination runs the one cleanup boundary.
- [ProcessExitUnmapped](Process/ProcessExitUnmapped.jet) — `process.exit(101)` ends a program with the user-error status 1, since 101 is the status of a Jet defect (S36).
- [ProcessIOPriority](Process/ProcessIOPriority.jet) — `.io_idle()` and `.io_priority(level)` run the child in the idle or a best-effort I/O class.
- [ProcessMemoryScope](Process/ProcessMemoryScope.jet) — `.memory_scope(bytes)` caps the child and everything it starts together, and ends them at the reap.
- [ProcessKeepOpen](Process/ProcessKeepOpen.jet) — `spec.keep_open(handle, at:)` keeps a file, writer or directory open in the child at a chosen descriptor; `directory.duplicate()` passes one while the parent keeps its own.
- [ProcessRunAll](Process/ProcessRunAll.jet) — process.run_all runs a list of specs through a bounded worker pool.
- [ProcessRunAllParentDeath](Process/ProcessRunAllParentDeath.jet) — A program killed while its process.run_all pool runs leaves no job behind.
- [ProcessStdinClose](Process/ProcessStdinClose.jet) — A spawned child owns an explicit closeable stdin pipe.
- [ProcessUsage](Process/ProcessUsage.jet) — A finished child's receipt says what it used, read from the kernel when the child is reaped.
- [ResourceLimits](Process/ResourceLimits.jet) — This program's resource limits and usage live in core.sys.
- [SysExecutable](Process/SysExecutable.jet) — `os.executable()` names the running program: the script's absolute path under `jet run`, the built binary after `jet build`.
- [SystemPrivilegedEntry](Process/SystemPrivilegedEntry.jet) — Audited identity/session controls for a disposable user namespace only.
- [SystemProjection](Process/SystemProjection.jet) — Process directories, timestamps, signals, and child reaping.

## Terminal

- [GrepScan](Terminal/GrepScan.jet) — Parallel directory queue + regex file scan.
- [InputLines](Terminal/InputLines.jet) — The ambient `input` reads the files the arguments name, in order, else standard input.
- [Progress](Terminal/Progress.jet) — io.progress wraps a List/Iter and updates as the loop pulls.
- [ReadSentinel](Terminal/ReadSentinel.jet) — Pull one stdin line per turn and stop on the blank sentinel.
- [StdinFilter](Terminal/StdinFilter.jet) — Streaming line-by-line stdin.
- [StdinInput](Terminal/StdinInput.jet) — The ambient `input` is every line of the program's input, read lazily: the files its arguments name, else standard input.
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
