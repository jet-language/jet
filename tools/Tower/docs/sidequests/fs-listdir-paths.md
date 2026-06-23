# Plan: `fs.list_dir` full paths + path joining (D-LSDIR1)

**Status: DONE (D-LSDIR1=A+C, 2026-06-23). See examples/features/87_dir_entry.jet.**

Unblocks: **Priya** (directory-scanning tool — the canonical beginner CLI task).

---

## Goal

`fs.list_dir(path)` returns **names only** (`Vec<String>`, confirmed
`Prelude/Std.rs:437` and `CheckerStdlib.rs:1295`), so the user must re-join each
name with the directory by hand (`"{dir}/{name}"`) — fragile across separators
and a stumbling block for the first tool a beginner writes. Give a path-join story
so scanning a directory and operating on its entries is one obvious step.

Verified: `list_dir` returns `result(list_string, io_error)` — bare names. There
is no `path.join` helper (`grep path.join|join_path Source/` → nothing found in
stdlib surface).

## Pipeline touch points

- **stdlib** (`core.fs` or a `core.path` module): either change/augment
  `list_dir` to yield full paths, add a sibling `list_dir_paths`, or ship a
  `path.join(dir, name)` helper. Pure-Jet/Rust, no compiler change.
- **sema**: register any new function/return type.
- **codegen**: a path-join helper in `Prelude/Std.rs` using the platform separator.

## Invariants in play

- **I8** simplicity — don't add five path functions; pick the one obvious form.
- **One-path (philosophy):** beginners should fall into the correct, portable
  thing by default (no manual `"/"` interpolation).
- **I5** example showing a directory scan that operates on each entry.

## Open questions (need owner decision — D-LSDIR1)

1. **The core choice** — (a) `list_dir` returns full paths by default; (b)
   `list_dir` stays names, add `list_dir_paths`; (c) keep names, ship
   `path.join` and teach it. Default-full-paths is the most beginner-magic but is
   a behavior change to a shipped function.
2. **Entry richness** — return `[String]` paths, or a `DirEntry` value
   (`{name, path, is_dir}`) so a scan doesn't re-`stat` each entry? The persona
   also wants `is_dir` filtering.
3. **A `core.path` module?** — if path-join lands, does it pull in `dirname`,
   `basename`, `extension`, `normalize` as a small path module (and is that worth
   a module vs a few `core.fs` helpers)?
4. **Separator policy** — always platform-native, or always `/` with conversion
   at the FS boundary (the latter is friendlier for cross-platform scripts)?

## Test plan

1. `examples/features/dir_scan.jet` — list a fixture dir, print full paths of
   entries (or filter `is_dir`); golden output (I5).
2. Path-join unit test on each platform separator.
3. (If `DirEntry`) a test that `is_dir`/`name`/`path` agree with the FS.
