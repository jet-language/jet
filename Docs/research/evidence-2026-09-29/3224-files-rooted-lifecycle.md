# #3224 — Rooted file and temporary-resource lifecycle (CORE-F010, SCRIPT-F13)

Date: 2026-09-29. Binary: `jet-debug-snapshot14` via `~/.cache/jet-dev/safe-jet.sh`,
source rev `e9c708fa7` (shared, dirty checkout). Host: Linux x86_64.
Author: Closer03 (evidence closer).

## Question

Does the current `core.files` surface give a hermetic, typed lifecycle for rooted
reads (escape and replacement), symlinks, copy metadata, temporary files and
directories, locks, ignore-aware walks, partial reads, early loop exit and write
failure? Which pieces are missing capabilities?

## Method

- New witness `Examples/features/io/files_rooted_lifecycle.jet` (everything under
  one `files.temp_dir`). Ran it on the default tier (`safe-jet.sh run --allow=FS,IO,Env`)
  and AOT (`safe-jet.sh build --allow=FS,IO,Env` then the produced
  `.jet/build/files_rooted_lifecycle`), and on the interpreter (`run --interpret`).
- Minimal FileScope probe `~/.cache/jet-dev/scratch/Closer03/scope_probe.jet`.
- Read: `Core/files/files.jet` (`scope` :95, `walk_impl` :661-676, `copy_file_impl` :408-413,
  `temp_dir`/`temp_file`/`lock`), `crates/jet-codegen/src/Prelude/CoreLib/Top/FSRuntimeOps.rs`
  (`jet_std_fs_scope_read` :38, temp/lock owners :127-196),
  `Compiler/JetSema/Source/Sema/Calls/StreamHandles.jet:293-315` (FileScope.read special call).

## Evidence

Default run and AOT printed identical output (cross-tier `cmp` equal):

```
lexical is_within via link: true
resolved is_within: false
read_link names outside: true
copy2 content: mode
copy2 mode preserved: false (source 416)
temporaries while held: dir=true file=true
temporaries after scope: dir=false file=false
lock held: true
second lock while held: rejected Other
lock after release: exists=false
relock after release: true
walk: keep.txt
read_at 7+10: 3 bytes 789
read_at past end: 0 bytes
descriptors left after 5 early exits: 0
missing file: NotFound
write /dev/full: Other
```

This is the committed golden `Examples/features/expected/io/files_rooted_lifecycle.out`.

Interpreter: `jet run --interpret` stops before running with
`E0956 core.files.set_mode() isn't supported by the current evaluator yet` (line 73).
With the `set_mode` lines removed (scratch `files_nomode.jet`), the interpreter next stops at `files.walk_files(tree, ".gitignore")` with E0956 `non-string argument to a Core string call`.

### FileScope (rooted read) is unusable on the current binary

`scope_probe.jet`:

```jet
policy :: Authority.from_rights(["FS.Read:/etc"])
scope :: fs.scope(policy)
text :: scope.read("hostname") ?? { ... }
```

Both `run` and `run --interpret` reject it at compile time:
`E0102 <corelib>/Core/files::Core/files/files.jet::FileScope has no method read`.
The same failure occurs when the scope is a local, as in
`Examples/features/foundations/capabilities/run.jet:23-24`. Sema only routes
`FileScope.read` when `sema_stream_handle_is_canonical` accepts the receiver type
(StreamHandles.jet:314-315); the Core `pub struct FileScope` (files.jet:65) does not
pass. The runtime kernel (`jet_std_fs_scope_read`, descriptor-relative no-follow)
exists but no program can reach it, so the `..`, absolute, symlink-to-outside and
replaced-directory escapes could not be exercised. They are left out of the golden
rather than blessed as failures.

## Matrix (Linux; macOS and Windows unknown — not run)

| Behaviour | Status | Evidence |
|---|---|---|
| Scoped `..` / absolute / symlink escape → typed `IOError` | **blocked** (FileScope.read E0102) | scope_probe.jet |
| Directory replacement race (dir swapped for outside link) | **blocked** (same); no `openat`-style public rooted open exists beyond `FileScope.read` | read of files.jet / FSRuntimeOps.rs |
| Pure path vs FS effect (c4) | supported: lexical `is_within` true through a link, resolved `is_within` false | golden lines 1-3 |
| Symlink create / read_link | supported | golden line 3 |
| copy2 metadata | content only; mode not preserved (0o640 source → default) | golden lines 4-5; `copy_file_impl` reads and writes bytes |
| temp_dir / temp_file release at scope exit | supported | golden lines 6-7 |
| Second lock on held path fails, succeeds after release | supported; failure kind is `Other` (create-new `AlreadyExists`) | golden lines 8-11 |
| `.gitignore`-aware `walk_files` | supported; ignore file itself is also skipped | golden line 12 |
| Partial `read_at`, read past end | supported (3 bytes, 0 bytes) | golden lines 13-14 |
| Early exit from `FileReader.lines()` closes the handle | supported (0 descriptors left after 5 early exits, `/proc/self/fd`) | golden line 15 |
| Missing file / full device write | typed `NotFound` / `Other` | golden lines 16-17 |
| Interpreter parity | **fails** (E0956 `set_mode`) | above |

## Verdict

PARTIAL. Criteria 2, 3, 4 and 5 are met by the witness on default run and AOT; the
`symlink`/`lock`/`copy2` doc comments in `Core/files/files.jet` now state the observed
behaviour (c5). Criteria 1 and 7 are not met: the rooted-read (`FileScope.read`)
escape and replacement cases cannot compile on the current binary. Criterion 6: the
missing capability is recorded below as a follow-up, not inferred.

## Follow-ups

1. Defect: `FileScope.read` is E0102 on every tier (repro above). Owner: JetSema
   stream-handle canonical-type check for `FileScope`.
2. Defect: interpreter has no `core.files.set_mode` (E0956).
3. Decision needed: `copy2` is named after Python's metadata-preserving copy but copies
   bytes only. Either preserve mode/timestamps or rename/document; owner-gated API meaning.
4. After (1), add the four escape cases (`..`, absolute, symlink-to-outside, replaced
   directory) to `files_rooted_lifecycle.jet` and regenerate the golden.
