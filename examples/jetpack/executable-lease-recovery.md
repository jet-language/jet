# Executable lease recovery

An executable package consumer owns one authenticated lease container. Its
inheritable lifetime lock stays held through the complete child process tree.
An audit observes that state; it does not clean it up.

Read the state first:

```text
jetpack audit --no-color
```

When the report contains a stale lease, the stable lease portion is:

```text
           ▸   Leases:      0 active, 1 stale
           ▸   Lease Note:  stale executable leases await `jetpack hangar recover`
```

Repair only at the Hangar recovery boundary:

```text
jetpack hangar recover --no-color
jetpack audit --no-color
```

Recovery removes a lease only when both its authenticated owner lock and its
container lifetime lock are idle. It can remove interrupted generations and
stale snapshots, but it keeps a snapshot protected by a running descendant
and never replaces the last complete generation with a partial one.
