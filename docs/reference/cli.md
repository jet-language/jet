# Jet command line

## Project tasks

Mark a top-level function with `#Job` to make it a project task. Use `#Doc` to add one help line.

```jet
#[Job, Doc("Seed local data"), Every(5min)]
fn seed() {
    // ...
}
```

Run `jet tasks` in the project directory to list all declared tasks. Scheduled tasks also show their `#Every` schedule.

```text
$ jet tasks
seed  Seed local data (every 5min)
```

Run a task with `jet run --task seed`. Cross-task dependencies are ordinary function calls.

In a workspace, use `jet tasks -p member` to list tasks for one member.
A bare `jet tasks` names the members when the choice is ambiguous.
