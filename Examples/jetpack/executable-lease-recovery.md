# Executable lease recovery

Jetpack records an executable lease while a managed process tree can still
use an executable or one of its snapshots. `jetpack audit` observes those
leases; `jetpack hangar recover` is the explicit reclaim operation. The
implementation is in
[`ExecutableLease.rs`](../../crates/jetpack/src/RuntimePolicy/ExecutableLease.rs),
with the audit surface in
[`package_hangar_vendor.rs`](../../crates/jetpack/src/CLI/package_hangar_vendor.rs).

## Inspect before reclaiming

Audit is read-only. It reports active and stale leases, snapshot protection,
and the objects that can be reclaimed without changing the hangar:

```sh
jetpack audit --no-color
```

A representative stale-lease note is:

```text
             ▸   Leases:      0 active, 1 stale
             ▸   Lease Note:  stale executable leases await `jetpack hangar recover`
```

The counts are observations from the current store, not fixed output. Use the
actual audit result to decide whether recovery is appropriate.

## Recover safely

Recovery is deliberately separate from audit and is available only through the
Hangar command:

```sh
jetpack hangar recover --no-color
jetpack audit --no-color
```

The recovery path removes a stale executable lease only when both authorities
agree:

- the authenticated owner lock has been released; and
- the inheritable container-lifetime lock has no live process-tree descendant.

The owner lock authenticates the handoff. The container-lifetime lock is held
by the process tree, so a descendant keeps the executable protected even after
the original owner exits. See
[`RuntimePolicy.rs`](../../crates/jetpack/src/RuntimePolicy.rs) and
[`Store.rs`](../../crates/jetpack/src/Store.rs) for those boundaries.

Interrupted generations and stale snapshots are eligible only when they are no
longer protected. Descendant-protected snapshots remain intact. Recovery also
avoids replacing a complete executable with a partial generation; temporary
reclaim names are quarantined until the operation can be completed safely.
