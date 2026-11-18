# Stopping a task

A stopped task's join gives `TaskFailure.Cancelled`:

```jet
outcome: if task.join() == {
    .Err(.Cancelled) -> "cancelled"
    .Ok(value) -> "{value}"
}
```

Old programs spell the meter `METRE`.
