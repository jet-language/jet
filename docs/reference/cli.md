# Jet command line

## Project jobs

Mark a top-level function with `#Job` to make it a project job. Use `#Job(.Dev)`, `#Job(.Ship)`, or
`#Job(.Internal)` to choose its build visibility. Use `#Doc` to add one help line.

```jet
#[Job(.Dev), Doc("Seed local data"), Every(2h)]
fn seed_data() {
    // ...
}
```

Run `jet jobs` in the project directory to list all declared jobs. Scheduled jobs also show their `#Every` schedule.
`.Internal` jobs appear in this inventory but are callable only from code or schedulers.

```text
$ jet jobs
seed_data  [dev] Seed local data (every 2h)
```

Run a job with `jet run app.jet -- seed_data`. Cross-job dependencies are ordinary function calls.

In a workspace, use `jet jobs -p member` to list jobs for one member.
A bare `jet jobs` names the members when the choice is ambiguous.

## Run a project

Inside a package, `jet run` selects the canonical `run.jet` entry. The shared
bare resolver checks `run.jet`, `src/run.jet`, then `<package>.jet`; `jet dev`,
`jet check`, `jet build`, and `jet test` use the same package target. Pass a
file or directory when the target is explicit:

```bash
jet run
jet run run.jet
jet dev
jet check run.jet
jet test run.jet
```

If a workspace has multiple members, use `jet run -p <member>` or an explicit
path. A single retired `main.jet` is migrated to `run.jet` with a notice;
ambiguous old and new layouts are rejected until one entry remains.

## LLM surface digest

`jet inspect digest` prints the complete generated LLM surface. Use
`jet inspect digest --list-topics` to discover byte-exact slices, then fetch one
with `jet inspect digest --topic diagnostics` or `jet inspect digest --topic core.time`.
Concatenating every listed slice reproduces the complete digest.

## Build provenance

Jet folds these build facts to constants before runtime:

- `@build.stamp.git` is the commit hash. It has a `-dirty` suffix when the worktree has changes.
- `@build.stamp.dirty` states whether the worktree has changes.
- `@build.stamp.toolchain` is the Jet version.
- `@build.stamp.at` is the timestamp stored in `.jet/lock`.

`@build.stamp.at` records the history of the lock file. It is not the time when Jet built the binary.
A locked build replays all four values from `.jet/lock` and does not read the clock or probe Git.

Run `jet explain @build.stamp.at <file>` to see the writer chain.
