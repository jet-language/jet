# Jet command line

## Project jobs

Mark a top-level function with `#Job` to make it a project job. Use `#Job(.Dev)`, `#Job(.Ship)`, or
`#Job(.Internal)` to choose its build visibility. Use `#Doc` to add one help line.

```jet
#[Job(.Dev), Doc("Seed local data"), Every(5min)]
fn seed() {
    // ...
}
```

Run `jet jobs` in the project directory to list all declared jobs. Scheduled jobs also show their `#Every` schedule.
`.Internal` jobs appear in this inventory but are callable only from code or schedulers.

```text
$ jet jobs
seed  [dev] Seed local data (every 5min)
```

Run a job with `jet run app.jet -- seed`. Cross-job dependencies are ordinary function calls.

In a workspace, use `jet jobs -p member` to list jobs for one member.
A bare `jet jobs` names the members when the choice is ambiguous.
