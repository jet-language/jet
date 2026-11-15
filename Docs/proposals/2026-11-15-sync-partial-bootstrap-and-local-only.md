# Sync: partial bootstrap for large workspaces, and a local-only mode

Status: design note for #3709 c1. Not built. It needs an adversarial review
in Docs/audits before any part of it is built or called better than an
existing system (AGENTS.md, Adversarial design review).

Date: 2026-11-15.

## Scope

Docs/proposals/2026-11-13-web-app-framework-shape.md (the shape) already
covers the device cache of live results, prediction and rollback, socket
fan-out of invalidations, and the offline queue (its 4.6, 4.8 and 4.9). It
does not say:

- what a device loads first when a workspace is large, how much of it the
  device keeps, and how it catches up after a long time away (partial
  bootstrap);
- how an app runs with sync switched off, for a whole app or for one
  collection, and what happens when sync is switched on or off later
  (local-only mode).

This note covers those two parts, inside the shape's ratified choices:
O4 A (the server is the authority; the device keeps the last live results
and a queue of writes), D-SYNC2 (data that must merge offline opts into
`app.collection`), D-INVALIDATE1 (a write refreshes every live query whose
read set it meets) and D-LIVEQUERY1 (a live query is one call of an
endpoint with codable arguments).

## 1. What the device holds today

Under O4 A a device never downloads a workspace. It holds:

- the last result of each live query a screen read, keyed by the endpoint
  and its encoded arguments. Today's live registry keys a binding the same
  way (`Core/App/App.jet`, `LiveRecord`: its source's wire name and
  `live_key` of the encoded arguments) and holds at most
  `LIVE_MAX_QUERIES` (1024) bindings, evicting the oldest;
- the queue of writes, under `.offline(.Queue)`;
- the documents of any `app.collection` it opened.

So the first screen of a cold device costs the live reads of that screen,
which the server runs together before it sends HTML (the shape's 4.6). A
workspace with 200,000 tasks costs the same first paint as one with 200, as
long as no single read returns all of them. That is the partial bootstrap
Linear had to add to its sync engine for large workspaces [LINEAR-SCALE],
and the query-driven sync of Zero [ZERO-SYNC]: the reads decide what
arrives.

Three things are missing for large workspaces. Sections 2 to 4 add them.

## 2. Bounded reads

A live read that returns a list must be bounded, or one read becomes a full
bootstrap.

- A live query's result is at most `LIVE_MAX_PAYLOAD` bytes (4 MiB,
  `Core/App/App.jet`). Today an oversized result is silently not stored.
  The design: the server refuses it with a typed failure that names the
  endpoint and the size, `Loadable.Failed` on the device, and a row in
  `jet inspect choices` for every live endpoint whose return type is an
  unbounded list.
- A list endpoint that a screen reads live takes a page (`limit` and an
  opaque cursor from the last row). The device cache keeps each page under
  its own key, so scrolling back reads the cache and a write invalidates
  only the pages whose read set it meets (D-INVALIDATE1 already keys by
  read set; a page's read set is its range).
- The limit counts rows after the row policy, as #6827 c3 requires, so a
  caller who may read few rows still gets a full page.

## 3. The warm set: what a device keeps for offline use

Under O4 A a device can read offline only what it has seen. On a phone that
opened one board, every other board is unreadable on the next flight. Linear
loads the parts a person uses most first and the rest on demand
[LINEAR-SCALE]; PowerSync downloads the buckets its sync rules give a user
[PS]; ElectricSQL syncs declared shapes [ELEC].

The design adds a warm set to the offline mode of an entry:

```jet
// Proposed, not built.
screens.crewboard()
    .offline(.Queue, warm: fn(crew: app.Remote<logic.Crew>) {
        crew.assigned_to_me()
        loop project in crew.my_projects() -> crew.board(project.id)
    })
```

- Each statement of the closure is one call of an endpoint with codable
  arguments (the D-LIVEQUERY1 rule), or a `loop` over the result of such a
  call whose body is one such call. Anything else is a compile error that
  names the statement. Each call is a live query under the same key as a
  screen's read, so a later screen finds it in the device cache.
- After the first paint, and only on a connection the platform does not
  report as metered, the device runs the warm reads in order, one at a time,
  until the device budget (section 4) is used. A read that fails is retried
  with backoff and does not block the next one.
- A `loop` warms in the order its list gives: the boards of the projects
  `my_projects` lists first come first, so the server's order is the
  priority.
- Warm entries refresh like any live query: a write that meets their read
  set marks them stale, and the device re-reads them when it is online.

What the person sees: nothing new while online. Offline, a screen whose read
is in the device cache shows it with its age ("Updated 2 h ago"); a screen
whose read is not shows `Failed(CallError.Unreachable)` with "Not available
offline".

## 4. The device budget, eviction and catching up

- **Budget.** Each signed-in account on a device has one byte budget for
  cached live results and collection documents: 256 MiB on desktop, 64 MiB on
  phones, and on the web what `navigator.storage.estimate()` grants, capped
  at 64 MiB. The entry may lower it. Firestore's offline cache has the same
  kind of size limit with collection of unused documents [FS-OFF].
- **Eviction.** Least recently read first. Warm entries are evicted last,
  and in reverse warm order. The queue is never evicted: when queued writes
  alone exceed the budget, new writes fail with a typed `QueueFull` failure
  instead of dropping old ones.
- **Catching up.** Each cached result keeps the server's invalidation
  version at the time it was read. On reconnect the device sends its cached
  keys with their versions, in pages of 500 keys, and the server answers
  only the keys whose read sets met a write since. A device away for a month
  therefore re-reads what changed, not what it holds. When the server no
  longer has the write log back to a key's version (it keeps 30 days), it
  answers "re-read" for that key, which costs one read, never a wrong value.
- **Revocation.** A re-read that the row policy now refuses removes the
  entry from the device. A person removed from a project loses its cached
  boards on the next connection, not when the cache happens to expire.
- **Wire versions.** Entries are keyed by the wire version of their
  endpoint (the shape's 4.12). After an update, entries of a changed
  endpoint are dropped, and entries of unchanged endpoints are kept.
- **Collections.** An `app.collection` is partitioned by a key its server
  policy reads (a project ID). A device opens partitions, not collections.
  A partition's first load is a snapshot at a version, sent in pages with a
  checkpoint, so an interrupted load resumes at the last page; changes made
  while the snapshot streams are applied after it, by version. Losing access
  to a partition removes it from the device, as for live results.

## 5. Local-only mode

Local-only means that the device is the only store for some or all data and
nothing is sent to a server.

### 5.1 A whole app with sync switched off

The shape already has the pieces. The web entry opens the database with
`logic.open_crew(path)` and mounts the service with `.mount(crew)`; the
desktop and phone entries never mount it, so every call goes to `server:`
(the shape's 3.4 to 3.6). In today's code `app.remote(service)` runs each
call on a service the process holds, and `app.remote_at(address)` posts it
to a server (`Core/App/App.jet`). A desktop or phone entry that opens the
database in the app's data directory and mounts the service itself is a
local-only app:

```jet
// Proposed, not built: the desktop entry of a local-only Crewboard.
run: fn() logic.TaskError! {
    crew: logic.open_crew(native.data_path("crewboard.db"))
    screens.crewboard()
        .mount(crew)
        .window(width: 1200, height: 800)
        .serve()
}
```

- The screens do not change: they call `app.Remote<Crew>` either way.
- Live queries and invalidation by read set work the same, in one process.
  Nothing is predicted and nothing is queued: each call runs at once on the
  device's own database, and nothing is sent.
- Row policies still run. The caller is the device's local account, which
  the entry creates on first run, so a policy that reads `app.caller()` has
  one answer.
- On the web a local-only app keeps its database in the origin's storage
  (OPFS or IndexedDB). The entry asks for persistent storage
  (`navigator.storage.persist()`); when the browser refuses, the app says so
  once, because the browser may then delete the data under storage
  pressure.

### 5.2 One collection with sync switched off

A collection can be created with sync off: `app.collection<logic.Note>(
"drafts", sync: .Off)`. Its documents stay on the device and are never
pushed. The rest of the app syncs as usual.

### 5.3 Switching sync on later

Local rows use client-made UUIDv7 IDs (`app.new_id`, #3709 c23), so they can
reach a server without renumbering. Switching on:

- asks which account the local data joins. It never joins whichever account
  happens to be signed in, which is the rule #3709 c17 keeps for the queue;
- sends each local row as one call of the same server method an online
  client calls to create it, carrying the row's ID and its current value, in
  creation order, under that account's session, so every policy runs (as
  D-SYNC2 already does for collection changes). Local edit history is not
  replayed; the server sees the rows as they are;
- turns each refused call into a kept notice (#3709 c26) and keeps the
  refused row locally, marked "Not synced".

### 5.4 Switching sync off for a synced app

For server-owned data (O4 A) there is no local authority to fall back to,
so sync off means: the device keeps reading its cache and queues writes,
and the queue is held, not sent, until sync is on again. The status bar
shows "Sync is off: 3 changes waiting". Signing out while writes are held
asks whether to discard them, and never sends them under another account.

### 5.5 Sign-out and shared devices

Sign-out deletes the account's cached live results and synced partitions.
Local-only data belongs to the device profile, not to an account; it is
deleted only by an explicit "Remove local data", which names what it
removes.

## 6. Failure modes

| Failure | Behaviour |
|---|---|
| A live read is larger than `LIVE_MAX_PAYLOAD` | Typed failure naming the endpoint; never a silent miss |
| The device budget is full of queued writes | New writes fail with `QueueFull`; queued writes are never dropped |
| A partition snapshot is interrupted | Resumes from the last page's checkpoint |
| The server's write log no longer reaches a cached version | That key is re-read |
| Access is revoked while the device is offline | The cached entry stays readable offline and is removed on the next connection; the note states this window |
| The browser refuses persistent storage | The app says so once; data may be deleted by the browser |
| Local data joins an account and the server refuses rows | Each refusal is a kept notice; the rows stay local, marked "Not synced" |

## 7. What the review must attack

- Whether the warm set is enough for "works offline" on a phone, against
  Linear's on-demand loading [LINEAR-SCALE] and PowerSync's per-user buckets
  [PS], which need no warm list.
- The revocation window: revoked data stays readable on a device that stays
  offline. The review must check whether Firestore and PowerSync have the
  same window, and whether encrypting the cache per session closes it at an
  acceptable cost.
- The fixed budgets in section 4, which are guesses until measured.
- Whether `sync: .Off` on a collection and the in-process service of 5.1
  are one concept or two.

## 8. Owner choices this raises

These are ballot drafts with same-program alternatives; none is ratified.

- **Warm set.** A: `.offline(.Queue, warm: fn)` in the entry (section 3).
  B: a `.warm()` mark on screens, so the warm set is the screens marked.
  C: no warm set; offline reads only what was seen, and the docs say so.
- **Local-only app.** A: the in-process service of 5.1; the one new
  function is `native.data_path`. B: an `Offline.Local` case that the
  framework turns into 5.1.
- **Collection sync off.** A: `sync: .Off` on `app.collection` (5.2).
  B: a separate `app.local_collection`.

## Sources

| Key | Source |
|---|---|
| LINEAR-SCALE | [Linear: Scaling the Linear Sync Engine](https://linear.app/blog/scaling-the-linear-sync-engine) |
| ZERO-SYNC | [Zero: introduction (query-driven sync)](https://zero.rocicorp.dev/docs/introduction) |
| PS | [PowerSync: sync rules](https://docs.powersync.com/usage/sync-rules) |
| ELEC | [ElectricSQL: shapes](https://electric-sql.com/docs/guides/shapes) |
| FS-OFF | [Firestore: offline data](https://firebase.google.com/docs/firestore/manage-data/enable-offline) |
