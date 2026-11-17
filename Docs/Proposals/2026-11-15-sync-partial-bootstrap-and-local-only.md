# Sync: partial bootstrap for large workspaces, and a local-only mode

Status: design note for #3709 c1, fifth draft. Not built.

- The first draft failed its adversarial review
  (`Docs/Audits/2026-11-15-sync-partial-bootstrap-review.md`): six claims
  failed and gaps G1 to G35 were filed.
- The second draft failed its second review
  (`Docs/Audits/2026-11-15-sync-partial-bootstrap-review-2.md`): S5, S8,
  S14, N1, N3 and N5 failed, and gaps G36 to G64 were filed.
- The third draft failed its third review
  (`Docs/Audits/2026-11-15-sync-partial-bootstrap-review-3.md`): S5 and N1
  failed narrowly on Postgres, N4 and N5 failed, and N7 failed narrowly.
  Gaps G65 to G88 were filed, with G5, G10, G14, G22, G41, G45 and G64
  carried.
- The fourth draft failed its fourth review
  (`Docs/Audits/2026-11-15-sync-partial-bootstrap-review-4.md`): S5 and N6
  failed narrowly, because a held stale value reached devices as current,
  and N4 failed narrowly as written. Gaps G89 to G101 were filed, with G5,
  G10, G14, G41, G45, G64 and G88 carried.

This draft answers every claim that failed and every gap of the fourth
review. Section 12.1 maps each gap to the text that answers it. The draft
needs a fifth adversarial review before any part is built. It makes no
claim that Jet is better than an existing system (AGENTS.md, Adversarial
design review).

Facts about other systems that were not opened for this note are marked
"from knowledge" where they are used. Each one is to be confirmed against
its documentation before the part that relies on it is built.

Date: 2026-11-15.

## 0. What changed after the fourth review

| Claim | Fourth review | This draft |
|---|---|---|
| S5 catch-up, N6 the hold | Fail narrowly: a held stale result reaches a device as `Loaded`, and nothing is published when the hold ends (G89) | A held result is never current to a watching device. The hold's start, and every invalidation during it, publish the binding's event with `after_ms`, the rest of the hold. The binding's first current result after a hold publishes the event again. Written, with a WebSocket case. With versions, a held result carries its run's version and a stale mark (3.2, 3.3) |
| N4 web store | Fails as written: `pauseVfs()` throws with a connection open; two installs collide; one pool spans every account | The connection is opened after the unpause and closed before the pause. Installation runs under the pool's lock, followed by a pause, with `preserveOnInitFailure`. Each account has its own pool, and the lock is named for the pool. Reads take the lease. SQLite 3.54 or later, one connection per worker. The measured lease includes the open, the schema read and a first-paint read (8.1, G94) |
| N4 crash order | Half closed: the answer path's order is unstated | The cache stores the answer and settles the request ID in one transaction, then the queue entry is removed. Rows record every request ID that touched them. The queue's and the store's locks are never held together (8.1, G95) |
| N1 Postgres watermark | Within limits; conditions, costs and latency unstated | `xid8` in the record; `pgoutput` with streaming and two-phase decoding off; idempotent record writes; the prune horizon from the pruning transaction's own snapshot; a recreated slot invalidates every server's bindings (3.1, G90). Write amplification is a required measurement. The marker is flushed on Postgres 17 and later, and transactional before; a server learns of its marker through a notification (3.5, G91) |
| S5 on SQLite | Within limits; a trigger dropped and re-created between two polls hides a write | Any schema change Jet did not make advances the store epoch (3.1, G92) |
| N7 ledger | Within limits; "below" contradicts 12.1; a live restore is unseen; size understated | One rule, strictly below. A restore the poll finds advances the epoch and closes every live connection. Sizes per store kind; one minute entry per store, not per server (3.7, G93) |
| S2 bounded reads | Within limits; "stated and measured" overstated; the rolling-deploy burst is not measured | The diff state is stated, and measured before building. A reconnecting subscriber sends its window's row hashes, so a restart resends only changed rows. The deploy burst is part of the measurement (4.1, G100) |
| S8, S12 partitions, sync off | Within limits; a server delete against a pending edit, and the checksum's state, unstated | A delete wins over a pending edit, which becomes a kept notice with its payload (7.2, G98). Each subscriber's running sum lives in server memory and is rebuilt at the next checkpoint when lost (7.5, G99) |
| S6 revocation | Within limits; the policy hash's scope unstated | The policy hash covers the policy's typed call graph (7.5, G99) |
| S9 local-only mount | Within limits; publishing a draft loses it on a later refusal | The kept notice of a refused write carries its payload (9.4, G101) |
| S11 sync on later, N5 seeds | Within limits; a deleted seed or row comes back at a later join | A seed table keeps deleted seeds and answers `Deleted` (9.7, G96). Every create under a join consults the join record; deletes made outside Jet mark it; a join older than 30 days does not resume (9.7, G97) |
| S1 first paint | Within limits; its lease is not in the measurement | First paint takes one lease for all its reads, and the measurement includes it (8.1) |
| S14 failure table | Within limits | A row for every failure the fourth review found (10) |
| N2, N3, S3, S4, S7, S10, S13 | Within limits | No change |

## 1. Scope and preconditions

The shape (`Docs/Proposals/2026-11-13-web-app-framework-shape.md`) covers
the device cache of live results, prediction and rollback, socket fan-out
and the offline queue (its 4.6, 4.8 and 4.9). This note adds partial
bootstrap and local-only mode. It stays inside the shape's ratified choices:

- O4 A: the server is the authority, and the device keeps the last live
  results and a queue of writes.
- D-SYNC2: data that must merge offline opts into `app.collection`.
- D-LIVEQUERY1: a live query is one call of an endpoint with codable
  arguments.

No part of this design is built before its precondition is met with its
proof (G1).

| Precondition | State on 2026-11-15 | Parts that need it |
|---|---|---|
| Invalidation after commit (#6868 c8) | On dev in `5fa1fd1c85`: a write inside a transaction marks its tables stale at the commit or rollback, not at the statement. Proof: `db_policy/db_scope_live_marks_writes_stale_only_after_commit` | 3.2 |
| A re-run overtaken by an invalidation stays stale (#6868 c8, G36) | On dev in `2ded9a1ded`. Proof: `web_app/live_rerun_overtaken_by_an_invalidation_stays_stale` | 3.2, 3.3 |
| Overtaken re-runs settle under sustained writes (#6868 c11, G86) | On dev in `8630ccf4dd`. Proof: `web_app/live_binding_under_sustained_writes_settles` | 3.2 |
| A held binding tells its watchers when to read it again (G89) | Written and submitted on 2026-11-15 as lane SUB `GoalsB-5-6868-hold`; not yet on dev. Proof: `web_app/live_hold_tells_watchers_when_to_read_again` | 3.2, 3.3 |
| Live hub reaches only watching connections (#6868 c9) | On dev in `1e0820d052`. An event goes only to connections that `app.watch` the binding, and carries the binding id only. Proof: `web_app/live_hub_events_reach_only_admitted_subscribers` | 3.3, 7.6 |
| Table-level invalidation | Built: `core.db.live:<table>` in `Core/DB/DB.jet`. SQL-narrowed read sets are #6868 c5, open (G84) | Everything that does not need read sets |
| Caps and coalescing (#6868 c3) | Open | The load measurement of 3.2, 4.1 |
| Cross-server broker (#6868 c4) | Open. The version map poll of 3.4 works without it | 3.4 wake-ups |
| D-INVALIDATE1 read sets (#6554, planning) | Unbuilt | 3.4 (the write log), 3.5 (per-range catch-up) |
| Row policies beyond `true` and `owner == user` (#6869) | Unbuilt | 4.3, 6.2 (membership), 7.5 |
| Limit counted after the row policy (#6827 c3) | Open | 4.3 |
| Offline queue and rebase (#6867), UUIDv7 IDs (#6867 c6), kept notices (#6867 c7), request keys (#6867 c2) | Unbuilt | 8.4, 9.7, 9.8 |
| Wire-skew rules (#6558, planning) | Unbuilt | 2.2 |
| Warm coverage per screen (#3709 c27, G10) | Open | 5.3, the compile-time check |
| Test host: disconnect and restore a client mid-test (#3709 c28, G64) | Open. A server restore needs no new ability on SQLite; on Postgres it shares #6868 c4's Postgres test host | Cases of 3.7, 5.1, 7.2, 9.7, 10 |
| SQLite WASM 3.54 or later in the web build (`preserveOnInitFailure`; one connection per worker) | Not checked | 8.1 |

## 2. What the device holds

### 2.1 A persistent per-account store

Today's live registry is one process-wide cell, decoded whole on every read
(`Core/App/App.jet`, R1 in the first review). It stays as the in-process
binding table. The device cache is a separate store:

- One SQLite database per account on the device: one file per account in
  the app's data directory on desktop and phone, and SQLite in OPFS on the
  web (section 8).
- One row per cached result. The row holds:
  - the key (2.2);
  - the value;
  - its version, server store id and endpoint epoch (section 3, 6.1);
  - its read-set summary (3.5);
  - a checksum (3.6);
  - the byte size and the device time of receipt (10);
  - the last-use time, the warm rank (section 5) and a pinned flag.
- Every read and write touches one row. Nothing decodes the whole store.
- On desktop and phone the offline queue (#6867) is a table in the same
  file. On the web it lives in IndexedDB (#6867 c4), and 8.1 states the
  write order between the two.

### 2.2 The key

A key is (account, endpoint wire name, canonical encoded arguments).

- The account comes first, so one account's entries are never another's
  (G27).
- The server store id, the version and the endpoint epoch are fields of the
  entry, not parts of the key (G46). A device that installs an app update
  while offline forms the same keys and still finds its entries. Catch-up
  compares those fields (3.5, 3.7, 6.1).
- **Canonical arguments.** Fields are encoded in declaration order, and an
  optional field equal to its declared default is omitted. So `board(id)`
  and `board(id, filter: .All)`, where `.All` is the default, are one key.
  For an endpoint whose wire version is unchanged, the encoding stays the
  same across app versions (G19). That rests on the wire-skew rules of
  #6558.
- A list window's `limit` is an argument, so windows of one list are
  different keys. Coverage (5.3) and eviction (4.1) treat the windows of one
  list endpoint as one family.

### 2.3 One limit, one eviction order

- The store has one byte budget. Each entry costs its value bytes plus a
  fixed 256-byte overhead, so the byte budget also bounds the entry count.
  `LIVE_MAX_QUERIES` does not apply to the device store (G13).
- Eviction is least recently used. A use is a screen's `state` read, or a
  warm fetch (section 5).
- An entry that a mounted screen reads is pinned and never evicted. If
  pinned entries alone pass the budget, the store goes over budget and
  devtools reports it, rather than evicting what is on screen. Proof: a case
  fills the budget while a screen is open and shows that its entry survives.
- The defaults are 256 MiB on desktop, 64 MiB on phones, and on the web the
  smaller of `navigator.storage.estimate()` and 64 MiB.
  - These are defaults pending measurement, not measured values (G14).
  - The measurement is Crewboard with 200,000 tasks: bytes per board, per
    warm set and per row duplicated across results, on desktop, phone and
    web.
  - The numbers in this section are replaced by the measured ones before
    the store is built.

## 3. Versions and catch-up

The rule throughout: a device may re-read too often, but it never keeps a
value older than the server's as current.

### 3.1 Commit-ordered versions from every writer

Versions must advance for every write to a synced table, including writes
Jet does not make: admin SQL, `db.migrate` data migrations, other services
and manual fixes (G38).

**SQLite.**

- Each synced table has `AFTER INSERT`, `AFTER UPDATE` and `AFTER DELETE`
  triggers. They advance one counter row (`jet_version`) and set that
  table's entry in the version map (3.4) inside the writing transaction,
  whoever the writer is.
- SQLite has one writer at a time, so the counter is commit-ordered.
- A read reads the counter in its own read transaction, so the version it
  records is its snapshot's version.
- `db.migrate` installs the triggers. When it creates, drops, renames or
  rebuilds a synced table (SQLite's create, copy, drop and rename pattern
  drops a table's triggers), it advances the store epoch (3.7) itself and
  reinstalls the triggers in the same migration (G68).
- **Changes made outside Jet (G68, G92).** Each schema change Jet makes
  (`db.migrate`, a trigger install) records the `schema_version`
  [SQLITE-PRAGMA] it leaves, in the same transaction. The server reads
  `schema_version` and `data_version` on its one-second poll.
  `data_version` changes when another connection commits; `schema_version`
  changes on any schema change.
  - Any `schema_version` other than the recorded one is a change Jet did not
    make. It advances the store epoch (3.7), so every device re-reads; the
    server reinstalls any missing trigger and records the new value.
  - So a trigger that an outside tool drops and re-creates between two polls,
    with a write in between, still advances the epoch. The write is never
    hidden; it costs every device one whole re-read.
  - An outside `ANALYZE` or `VACUUM` may change `schema_version` too (from
    knowledge) and then costs the same re-read. Jet's own maintenance
    records its value.
- Incremental BLOB writes (`sqlite3_blob_write`) fire no triggers, and a
  connection that turns triggers off (`SQLITE_DBCONFIG_ENABLE_TRIGGER`)
  writes without them (both from knowledge). Jet does neither on synced
  tables. An outside tool that does is not seen until the next epoch
  advance, and the docs state both.
- **The trigger cost is a required measurement.** Each row a statement
  writes also updates the counter row and its table's map row. Before the
  store is built, the note records the time and bytes of a 10,000-row
  update with and without the triggers.

**Postgres.**

- Write sets come from the decoded WAL, not from a row Jet's write path
  inserts. The server's log reader follows a logical replication slot over a
  publication of the synced tables [PG-LOGICAL]. Every committed
  transaction that touched a synced table arrives with its commit LSN and
  the tables it wrote, whoever made it. The commit LSN is the write's
  version.
- The reader records what it decodes in the version map (3.4). That table is
  not in the publication, so the reader's own writes are never decoded as
  application writes, and the reader cannot feed itself.

**The commit record (N1, G65, G90).**

- For each decoded commit the reader writes (top-level xid, commit LSN) to a
  commit record table, in the same transaction as its version map update
  and its new confirmed position C. That table is outside the publication
  too.
- **The xid is an `xid8` (G90).** `pgoutput` reports a 32-bit xid, which
  wraps; `pg_visible_in_snapshot` and `pg_snapshot_xmin` take the 64-bit
  `xid8`, which does not wrap [PG-INFO]. The reader extends a decoded xid
  with the epoch of its own `pg_current_xact_id()`, taking the previous
  epoch when the decoded xid's 32 bits are above the current ones. A
  decoded commit is never 2^31 transactions behind the reader: long before
  that its slot passes `max_slot_wal_keep_size` and is invalidated (10)
  [INFERENCE].
- **Decoding options (G90).** The reader starts `pgoutput` with `messages`
  on and `streaming` and `two_phase` off [PG-PROTO], so each transaction
  arrives once, whole, at its commit record.
- C is the position up to which every commit has been decoded and recorded.
  The reader advances it at each decoded commit and at each decoded marker
  (3.5), and stores it with the record rows it covers, so a reader restart
  resumes from a C whose rows are all present.
- **Idempotent writes (G90).** The record's key is the `xid8`, written with
  `INSERT ... ON CONFLICT DO NOTHING`. The version map keeps the larger of
  its value and the new one, and C only grows. The reader confirms a
  position to the slot only after the transaction that records it has
  committed. A restart therefore re-sends at most the commits after the
  last confirmation, and recording them again changes nothing.
- The reader records decoded commits in batches: one transaction per 100
  commits or 10 ms, whichever comes first.
- **Pruning (G90).** The pruning transaction deletes the record rows whose
  xid is below its own `pg_snapshot_xmin(pg_current_snapshot())`.
  - Every xid below that horizon has ended, so a committed one is visible to
    every later snapshot.
  - A snapshot that was already open and does not see such a commit still
    sees its record row: the delete is not visible to that snapshot, and
    vacuum keeps the row while the snapshot is open.
  - This needs no privilege. The fourth draft read `backend_xmin` from
    `pg_stat_activity`, a 32-bit `xid` that ordinary roles may see as null
    for other sessions [PG-STATS].
  - The record holds about the commits within the span of the longest open
    transaction: a long transaction grows it, as it holds back vacuum, and
    devtools shows its size.
- **A recreated slot (G90).** Writes made while the slot was gone were never
  decoded. Recreating it advances the store epoch. Every server reads the
  epoch on its one-second poll and invalidates every binding it holds, so
  connected devices re-read too. Devices that connect later re-read at the
  connection check (3.7).
- **Write amplification (G91).** Per batch the reader writes one
  transaction: a record row per commit, a version map row per table written
  and the C row. The prune deletes the record rows later, and vacuum
  reclaims them. Before the reader is built, the note records the rows, WAL
  bytes and vacuum time per synced commit under Crewboard's seeded write
  load, beside the SQLite trigger cost of the SQLite section above.

**The watermark of a Postgres read (N1, G37).**

- A versioned read runs at `REPEATABLE READ`, so its queries, C and the
  commit record are read in one snapshot S [PG-INFO].
- C as read in S is the reader's position as of S, and every commit at or
  below it has its record row visible in S, because both were written in
  one transaction.
- The read's version is the watermark W: the largest commit LSN at most C
  such that every recorded commit at or below W is visible in S. It is
  computed as:
  - the smallest commit LSN at most C among record rows whose xid is at
    least `pg_snapshot_xmin(S)` and not `pg_visible_in_snapshot(xid, S)`,
    minus one;
  - or C, if there is no such row.
  Only an xid at or above xmin can be invisible, so an index on the xid
  keeps this to the in-progress span.
- Bounding W by C is what makes the rule sound:
  - In Postgres, the order of commit records in the WAL and the order in
    which commits become visible to snapshots can differ (from knowledge;
    this is why Electric and other logical-replication consumers carry
    `xmin` and `xip_list` [ELEC-HTTP]).
  - Every commit at or below C is recorded, so the visibility check sees all
    of them. A commit the reader has not yet decoded is above C, and so
    above W. Catch-up from W therefore reports it.
- A visible write above W only makes the device re-read once more than
  needed, and never keeps a wrong value. A long transaction only keeps W
  low. The record holds top-level xids, and a snapshot lists only top-level
  xids [PG-INFO], so subtransactions do not change the rule.
- **Replicas.** A read served by a replica records the replica's replay
  position (`pg_last_wal_replay_lsn()`) and bounds W by the smaller of C and
  that position. A replica that cannot report it serves no versioned reads;
  those go to the primary.
- Proof: a case computes W for a snapshot that has an in-progress
  transaction which later commits below C, and shows that no device keeps
  an older value as current. A second case delays the log reader. A third
  updates a row with plain SQL outside Jet, on SQLite and on Postgres, and
  shows that the cached result is re-read. A fourth restarts the reader
  between its record write and its slot confirmation, and shows no record
  row twice and no device stale. A fifth recreates the slot after an outside
  write and shows connected devices re-read. On SQLite, a case drops a
  synced table's trigger, writes and re-creates the trigger within one poll
  interval, and shows every device re-read.

### 3.2 Publish after commit, the generation rule and the hold

- An invalidation is published only after its transaction commits, and a
  rollback publishes too (#6868 c8, on dev in `5fa1fd1c85`). Proof:
  `db_policy/db_scope_live_marks_writes_stale_only_after_commit`.
- That narrows the first draft's race from "statement to commit" to one
  re-run's duration (G60). A re-run that reads the old row just before a
  commit, and stores after that commit's invalidation, would clear the
  binding's stale mark and keep the old value as current.
- **The generation rule (G36), on dev in `2ded9a1ded`.** Each server
  registry binding counts the invalidations that reached it.
  - A re-run notes the count before it runs.
  - Its result is stored as current only if the count is unchanged when it
    stores.
  - Otherwise the result is stored but the binding stays stale.
  - Proof: `web_app/live_rerun_overtaken_by_an_invalidation_stays_stale`.
- The rule is the device's 3.3 rule, applied on the server. Without it, a
  device that has seen a newer invalidation would re-read and get the
  registry's stale-but-clean value back, again and again.
- **The hold (G86), on dev in `8630ccf4dd` (#6868 c11).** Under the
  generation rule alone, a binding whose run takes longer than the gap
  between writes to its table is never stored as current, so every read
  would run the source again.
  - From the second run in a row that an invalidation overtook, the binding
    serves its stored result, still stale in the registry, until a hold
    ends: that run's end plus the larger of its duration and 100 ms.
  - The first read after the hold runs the source again. A run that nothing
    overtook clears the hold.
  - The registry never keeps an older value as current, spends at most half
    its time running under sustained writes, and catches up within one hold
    after the writes stop.
  - Proof: `web_app/live_binding_under_sustained_writes_settles`.
  - The re-runs per second for a slow query over a table written ten times a
    second are recorded with #6868 c3's caps and coalescing, which is open.
- **A held result is never current to a device (G89).** The fourth review
  measured on dev that a held result reached a device as `Loaded`, and that
  nothing was published when the hold ended: a device whose last re-read
  fell inside a hold kept the older value until some later write. The fix,
  in `Core/App/App.jet` (lane SUB `GoalsB-5-6868-hold`, not yet on dev):
  - The hold's start publishes the binding's hub event with
    `"after_ms":<n>`, the hold's length in milliseconds, rounded up.
  - An invalidation during a hold publishes the event with the rest of the
    hold as `after_ms`.
  - A device that reads the binding inside a hold gets the held result and
    reads again after `after_ms`. That read runs the source.
  - The binding's first current result after a hold publishes the event
    again, without `after_ms`, so every watcher that read a held result
    reads the current one, whether or not it waited.
  - No timer runs on the server. A device that honors `after_ms` is what
    ends the hold; if none does, the next read of any caller does, and its
    result is published.
  - A connection that starts watching during a hold first receives the
    topic's latest event, which carries the `after_ms` of its time: it waits
    at most one hold longer than it needs to.
  - Server code that calls `state` itself still receives the held result as
    `Loaded`. The stale mark #6868 c11 asks for is carried where a result
    leaves the server: the hub event now, and the result's version and stale
    mark once versions are built (3.3).
  - Proof: `web_app/live_hold_tells_watchers_when_to_read_again`. A device
    reads twice under writes, then once inside the hold and gets the held
    value, waits the `after_ms` its event named after the writes stop, and
    reads the newer value, preceded by the republished event.

### 3.3 The monotonic store rule

- The device stores a result only if its version is at least the version it
  already holds for that key (G4).
- The device also keeps, per key, the highest invalidation version it has
  seen. An entry is current only if its version is at least that. A result
  that arrives after a newer invalidation is stored, but stays stale and is
  re-read.
- **Versions on every message (G41).** Every hub event and every catch-up
  answer carries the commit version this rule compares.
  - The event on dev (`1e0820d052`) is
    `{"live":"invalidate","binding":<id>}`; the hold's fix adds
    `"after_ms":<n>` while the binding is held (3.2). It gains a `version`
    field when versions are built.
  - A held result carries the version of the run that produced it and a
    stale mark. The device stores it as stale, because the invalidation that
    overtook that run has a higher version, and re-reads after the hold.
    It never carries a later version, which would make the stale value
    current.
  - Until versions are built a device treats every event as newer than
    anything it holds and re-reads, after `after_ms` when the event has one.
    Since the hold's end publishes again (3.2), no device's last read is a
    held result once the writes stop.
- Proof: a case delivers a re-read at v1 after a push at v2 > v1, and shows
  that v2's result stays.

### 3.4 The version map, and the write log only with read sets

The second review showed that under table-level invalidation the 30-day
write log answers nothing that a per-table map cannot (G39). So:

- **The version map.** The application database holds one row per synced
  table: the table's highest write version.
  - On SQLite the triggers of 3.1 update it in the write transaction.
  - On Postgres the log reader updates it as it decodes each commit.
  - It does not grow with writes. The nonce ledger of 3.7 grows with time,
    one entry per minute at most.
- **Catch-up under table-level invalidation.** A key is current if every
  table in its read set has a map version at most the key's version.
- **Across servers (G70).** The live hub is per process: `live_invalidate`
  in `Core/App/App.jet` reaches only the bindings registered in its own
  process, and publishes nothing in a process that holds no binding of the
  source. A write on server A therefore reaches a subscriber on server B
  through B's poll of the version map: B invalidates its own bindings of
  every table whose map version advanced. The cross-server bound is the
  poll interval, one second.
- **Wake-ups.** Postgres `LISTEN/NOTIFY` is only a wake-up hint that makes
  the poll run at once (#6868 c4 owns the broker). Its payload must be
  shorter than 8000 bytes [PG-NOTIFY], and a server that is not listening
  misses it. A server that misses a wake-up still polls at least once a
  second, so a broker outage delays invalidations and loses none.
- Proof: a two-server case shows a write on server A reaching a subscriber
  on server B within one second, with the broker down.
- **The write log** (version, write set, nonce, timestamp, kept 30 days) is
  built only with D-INVALIDATE1 read sets (#6554). Only then does a write
  set name ranges that a map cannot summarize. Before it is built, two
  numbers are measured and recorded here (G5):
  - log bytes per day for Crewboard with 200,000 tasks under its seeded
    write load;
  - the server time to catch up a device holding 1,000 keys after 30 days.
- Partition replay (section 7) needs row changes, not table versions. Its
  change buffer covers collection tables only. Section 7.4 states its
  retention.

### 3.5 Read sets and catch-up

- Each result comes back with a read-set summary, which the device stores
  with the entry and sends back at catch-up. Under table-level invalidation
  it is the list of tables the query reads.
- A summary has a byte cap of 4 KiB per key. Past it, the summary falls back
  to its tables (G40).
- A summary is device input, trusted only for that device's own freshness.
  No server decision about another party relies on it: not an audit, not a
  purge proof, not a revocation proof.
- At catch-up the device sends (key, version, epoch, read-set summary) in
  pages of 500 keys. The server answers "current", "re-read" or "unknown"
  for each key.
- **Catch-up waits for the log reader (G37, G66, G91).** On Postgres the
  server writes a catch-up marker, a logical message with prefix
  `jet.catchup` and the marker's id, and answers once the reader has
  decoded it.
  - On Postgres 17 and later the marker is non-transactional and flushed:
    `pg_logical_emit_message(false, 'jet.catchup', <id>, true)`. A
    non-transactional message is decoded "as soon as the logical decoder
    reads the record", and `flush` writes it to WAL at once [PG-ADMIN],
    [PG-17].
  - Before 17 there is no `flush`, and a non-transactional message waits
    for the next WAL flush, up to `wal_writer_delay` (200 ms by default,
    from knowledge). There the marker is a transactional message in its own
    one-statement transaction, whose WAL record "is flushed along with its
    transaction" [PG-ADMIN]. It is decoded at its commit record, after every
    commit before it, and costs one xid per marker.
  - Logical decoding emits each transaction at its commit record, in commit
    order, with streaming off (3.1), so when the marker arrives every commit
    before it has been recorded (from knowledge). The reader asks `pgoutput`
    for messages.
  - A quiet database does not stall it: the marker is itself the WAL the
    reader waits for.
  - **How the server learns its marker was decoded (G91).** The reader, in
    the transaction that records a decoded marker, writes the marker's id
    beside C and sends `NOTIFY jet_catchup, '<id>'`, which Postgres
    delivers at that commit. The waiting server listens on `jet_catchup`
    and also reads C's row every 50 ms, so a lost notification costs at most
    50 ms. The time from the emit to the answer (the flush, the decode, the
    reader's commit and the notification) is part of G91's measurement, on
    Postgres 17 and on 16; the target is under 100 ms on an idle database.
  - Catch-ups that start within 50 ms share one marker.
  - On SQLite there is no reader: catch-up reads the version map.
- **When the reader is down or behind (G67).** If the marker is not decoded
  within 10 seconds, every key of the page is answered "unknown", never
  "re-read". The device keeps showing its entries with their age and retries
  the catch-up with jittered backoff from 1 second to 5 minutes. A screen
  that opens such a key re-reads that key alone. So a reader outage sends no
  device into a whole-cache re-read during the incident. Devtools shows
  "log reader behind".
- **Under table-level invalidation (`core.db.live:<table>` in
  `Core/DB/DB.jet`), which is all the code has today,** one task write marks
  every cached key that reads `tasks`. After a month away, catch-up re-reads
  every cached key over a table written in that month. For Crewboard that is
  nearly every board, bounded by the device budget. Catch-up still never
  sends a wrong value, but it saves little.
- With D-INVALIDATE1 read sets (#6554), catch-up re-reads only the boards
  that changed. That saving is not claimed before #6554 is built and
  reviewed (G2).

### 3.6 Checksums

- Each result carries a 128-bit hash of its encoded value, computed by the
  server. The device checks the hash whenever it loads the entry. A mismatch
  deletes the entry and re-reads it (G6).
- Each partition checkpoint carries a checksum over the subscriber's rows
  (7.5).
- Proof: a case corrupts one cached entry and shows that it is repaired on
  the next connection.

### 3.7 Store identity and server restore

- The server database holds a store id (a random UUID made at creation) and
  an epoch. Every version the device sees comes with both.
- **The nonce ledger (N7, G69, G93).** The server keeps a ledger of
  (sequence, version, nonce) entries, each with a fresh random nonce and the
  highest version at the moment it is written. It writes one:
  - at the end of each minute in which anything was written. The entry is
    keyed by its minute and written with `INSERT ... ON CONFLICT DO
    NOTHING`, so with several servers the first one's entry stands and the
    minute entries do not multiply with the server count;
  - at every server start;
  - whenever its one-second poll finds its own latest entry missing or
    changed.
- **Size (G93).** Entries are kept 30 days: at most 43,200 minute entries,
  plus one per server start. For Crewboard's seeded write load, which writes
  every minute, that is the maximum.
  - On SQLite an entry is about 40 bytes: about 1.7 MiB for 30 days.
  - On Postgres a row of three fields, with its tuple header and its index
    entry, is about 85 bytes: about 3.5 MiB for 30 days.
  - With several servers only the start entries multiply: ten servers each
    restarted daily add 300 entries in 30 days.
  - The ledger grows with time, not with writes, and the version map rows
    carry no nonce.
- **One rule: strictly below.** Every answer at version v carries the
  server's latest entry whose version is strictly below v. The device keeps
  its highest version v_d and the entry that came with it.
- At connection, the server reports its store id, epoch and highest version,
  and checks the device's entry against its own latest entry strictly below
  v_d. The device re-reads every key if any of these holds:
  - the store id differs;
  - the epoch is later;
  - the server's highest version is below the device's;
  - the server's latest entry strictly below v_d is not the device's entry,
    or is missing.
- Why this detects a restore that loses versions the device has seen (G35):
  a restore to a point P below v_d is followed by a server start (or found
  by the poll), which writes an entry at version P. That entry, or the loss
  of the device's entry, makes the server's latest entry strictly below v_d
  differ from the device's. A later entry can never have a version below v_d
  otherwise, because versions only grow. A restore to a point at or above
  v_d lost nothing the device saw. This is Electric's `must-refetch` answer
  [ELEC-HTTP].
- A plain restart with no writes since the device's last answer writes an
  entry at the highest version, which is at least v_d, so it is never
  strictly below v_d and causes no re-read.
- **A restore without a restart (G93).** The connection check alone would
  miss it for a device that stays connected. So:
  - the server whose poll finds its own latest entry missing or changed
    writes a new entry and advances the store epoch;
  - every server reads the epoch on its own one-second poll, invalidates
    every binding it holds, and closes every live WebSocket connection with
    a "store restored" reason;
  - each device reconnects, runs the connection check, sees the later epoch
    and re-reads every key.
  - A connected device can therefore keep a result newer than the restored
    server for up to two poll intervals plus its reconnect: about two
    seconds. That window is stated in the docs. A result it receives in the
    window carries a version below the one it holds, so the monotonic store
    rule (3.3) does not store it as current either.
- **A device away more than 30 days** finds its entry pruned, counts it as
  missing and re-reads every key. The failure table states it (10).
- A documented restore procedure also advances the epoch. The ledger check
  catches a restore that skips that step.
- Proof: a case restores the server from a backup and shows that no device
  keeps a result newer than the server. A second restores to a point between
  two ledger entries and shows every key re-read. A third replaces a running
  server's SQLite file with a copy, without a restart, and shows a connected
  device closed and re-reading within two seconds. A fourth restarts the
  server with no writes and shows no re-read.

## 4. Bounded reads

### 4.1 One live window per list

- A list endpoint read live takes a `limit`. Scrolling further asks for a
  larger window (50, then 100, then 150 rows), which is one live query over
  one snapshot. Rows therefore appear exactly once, even when a row moves
  between what were "pages" (G7). This is how a limited, sorted query stays
  one view in Zero [ZERO-Q].
- A larger window is a new key of the same family (2.2). Its smaller
  predecessor becomes unused and is evicted first.
- **Updates travel as row changes (G44).**
  - An invalidation re-runs the window on the server.
  - The server compares the new result with the one the subscriber holds,
    by row ID, and sends inserts, updates, deletes and moves.
  - The bytes sent per write are therefore the changed rows, not the window.
  - This is Zero's delivery [ZERO-Q]. Unlike Zero, the server still re-runs
    the window rather than maintaining it incrementally.
- **The state the diff needs (G71).**
  - Per subscriber and window, the server keeps, in its memory, each row's
    ID and a 64-bit hash of its encoded value, in window order: about 32
    bytes per row, so about 32 KiB for a 1,000-row window, not the rows.
    New and changed rows are encoded from the re-run's result.
  - The state is not durable. After a server restart, or when the
    subscriber reconnects to another server, the state is gone.
- **Resuming a window (G100).** A subscriber that re-subscribes sends, with
  the request, the (row ID, value hash) list of the window its device holds:
  32 bytes a row, 32 KiB for 1,000 rows. The server re-runs the window,
  rebuilds its state from the result, and sends only the rows whose hash
  differs, with deletes and moves.
  - The list is device input, trusted only for that device's own view. A
    wrong list makes the server send that device wrong changes, which harm
    only it: the window result's hash (3.6) then fails on the device, which
    re-reads the window whole.
  - So a rolling deploy, which restarts every server in turn, costs each
    subscriber one re-run and the rows that changed, not its whole window.
    Without the list the burst would be the whole window per subscriber:
    200 times the window's bytes, up to 200 times 4 MiB, 800 MiB, per window
    at the cap.
- **Rows with no ID (G85).** A window's endpoint must return rows with an ID
  field; that is what makes it windowable, and the compiler checks it. Any
  other live result, such as an aggregate or a join with no stable key, is
  not windowed, and is resent whole when it changes.
- **The re-run cost is a required measurement.** Under table-level
  invalidation a task write re-runs every open window over `tasks` for every
  subscriber (caller-dependent windows are not shared, #6868 c6). Before
  windows are built, the note records:
  - the re-runs, bytes sent and server memory per task write for a
    1,000-row window with 200 subscribers;
  - the bytes sent and received when every server restarts in turn (a
    rolling deploy) with 200 subscribers per window, with and without the
    device's hash list (G100);
  - the same with D-INVALIDATE1 read sets, once #6554 exists.
- The window is bounded by the payload cap (4.2). Reading past it uses a
  non-live paged read. Those rows are not consistent with the window, are
  not cached as current, and are not available offline (5.4).
- Proof: a case moves a row from the third screenful to the first between
  two reads and shows it once. A second case shows that one changed row of a
  1,000-row window sends one row change. A third restarts the server after
  one row changed and shows the subscriber receive that one row. A fourth
  sends a wrong hash list and shows the device re-read the window whole.

### 4.2 Oversize results

- A live result over `LIVE_MAX_PAYLOAD` (4 MiB) fails as
  `Loadable.Failed(CallError.TooLarge)`, naming the endpoint, the cap and the
  bytes encoded when the encoder stopped. The server's encoder counts bytes
  and stops at the cap instead of building the whole result (G8).
- The failure is stored like any result, so `state` does not re-run the
  source on every read. Today it does, because an oversized value is not
  stored (`Core/App/App.jet`, `live_store`, first review R3). The source
  runs again only when the entry is invalidated.
- **Unbounded lists follow choose, explain, pin (G45).**
  - `jet inspect choices` lists each live endpoint whose return type, after
    removing an outer error and `Loadable`, is `[T]`, and whose query has no
    row limit that one of its parameters sets.
  - The listing shows that fact: the return type, and the query found with
    no limit. It does not judge by a parameter's name.
  - A pin on the endpoint, a declared maximum row count, silences the
    listing. A result over the pin fails as `TooLarge` at the server, and a
    dev run that reaches the pin reports it.
  - The pin's spelling is new syntax, so it is ballot B7.
- `jet inspect` warns when an endpoint's measured result size passes half
  the cap in a dev run.

### 4.3 The limit counts rows after the policy

The limit counts rows after the row policy, so a caller who may read few
rows still gets a full window. This waits for #6827 c3 and for row policies
beyond `true` and `owner == user` (#6869). It is not built before both are
(G9).

## 5. The warm set

### 5.1 What a warm read is

```jet
// Proposed, not built (ballot B1).
screens.crewboard()
    .offline(.Queue, warm: fn(crew: app.Remote<logic.Crew>) {
        crew.assigned_to_me()
        loop project in crew.my_projects() -> crew.board(project.id)
    })
```

- Each statement is one endpoint call with codable arguments, or a `loop`
  over one such call's result whose body is one such call. Anything else is
  a compile error that names the statement.
- A warm read is a one-shot read. The result is stored with its version,
  epoch and read set. It holds no server subscription. If a mounted screen
  reads the same key, that screen's live read takes the entry over (G11).
- **Re-checks while online (G42, G72).** Catch-up (3.5) checks warm entries
  at each reconnect and when a screen that reads the key opens. Every 15
  minutes while the device stays online it re-checks them too, under the
  warming rule of 5.2:
  - on a connection the platform does not report as metered, entries found
    stale are re-read;
  - on a metered connection, or with no network type, the check only marks
    stale entries unconfirmed. An unconfirmed entry is re-read when a screen
    opens its key, never in the background.
- An entry is shown as current only after a check that found it current.
  Otherwise it shows with its age while the re-read runs.
- **Batches (G43).**
  - A `loop` sends one batched request carrying every argument list. The
    server runs the batch in one read transaction, so the batch has one
    version.
  - The server streams the items. Each item is capped at
    `LIVE_MAX_PAYLOAD`, and the batch at the remaining device budget, which
    the request states.
  - An oversized item fails alone with `TooLarge`. The other items arrive.
- Proof: a case changes a warmed board while the device stays online and
  shows the entry is re-read before it is shown as current. A second case
  sends a batch with one oversized item.

### 5.2 When warming runs

- Warming runs after first paint, on a connection the platform does not
  report as metered.
- Where the platform reports no network type (for example, a browser
  without the Network Information API), warming runs only while the app is
  in the foreground, up to 16 MiB per launch.
- A read that fails is retried with backoff and does not block the next
  one. The server's list order is the warm priority.
- Proof: a case warms 50 boards and reports the requests sent and the
  server re-runs per task write.

### 5.3 Coverage

- A warm call helps only if its key equals a key some screen reads (2.2).
  Windows of one list endpoint count as one family.
- A compile-time check needs the endpoints and argument shapes each screen
  reads. #3709 c27 owns it (G10): the build reports, for each screen, which
  of its live reads are in the warm set, and a check fails a screen marked
  offline that reads anything outside it.
- Until that is built, coverage is checked only at run time:
  - `jet inspect` lists the warm set with each entry's key and estimated
    bytes;
  - devtools logs a warm key that no screen read during the session.
- The docs say that the run-time check catches drift only in a session
  that visits the screen.

### 5.4 What stays out of reach offline

Offline, a screen whose key is cached shows the result with its age
("Updated 2 h ago", 10). Any other read fails with "Not available offline":

- a search;
- a new filter;
- a board that was never warmed;
- rows read past a window's cap (4.1).

PowerSync and Zero answer those reads from rows on the device [PS-STREAMS]
[ZERO-Q]. This design does not. Ballot B2 offers a normalized device row
store with local queries as the alternative (G12). If offline search becomes
a requirement, B2 A is ruled out: it finds only exact cached queries.

## 6. Revocation, epochs and the offline window

### 6.1 Endpoint epochs

- Each endpoint has an epoch, a hash of three things:
  - its typed source call graph: the endpoint and every function it calls,
    Core functions included, as the typed syntax tree without comments or
    formatting, not generated code;
  - the row policies it reads;
  - its argument encoding.
- A deploy that changes any of these changes the epoch: a new sort order, a
  changed policy, a fixed query (G46, G73).
  - A compiler upgrade that changes only code generation does not.
  - A Core upgrade that changes the source of a Core function on the
    endpoint's call path does. Most Core upgrades therefore re-read most
    keys: a Core fix can change results, and the epoch cannot tell which
    fixes do.
  - A change of formatting or comments does not.
- **Configuration is not in the epoch (G73).** Configuration and
  environment values an endpoint reads are not hashed. A deploy that changes
  such a value, and must not leave old results current, advances the store
  epoch (3.7), which re-reads every key. The docs state this.
- The server computes the epoch and sends it with every result. The device
  stores it in the entry, never in the key (2.2).
- At catch-up the device sends each key's epoch. A key whose epoch differs
  from the server's is answered "re-read" (G17, S7).
- An app updated while offline keeps every cached entry readable with its
  age. Only the keys whose epoch changed are re-read at reconnect.
- Proof: a case updates the app while offline and shows that cached entries
  still render with their age, then are re-read at reconnect only where the
  epoch changed.

### 6.2 Revocation without a data write

- **Session revoked, account disabled, or signed out on another device.**
  - At launch the device shows cached results before it makes any request,
    so the cache is on screen until the first connection.
  - The server answers that first request with `SessionRevoked`. The device
    then deletes that account's store (9.6) and leaves the cached screens.
- **Membership change.** A policy that reads a membership table records that
  table in its read set, so a membership write marks the keys stale like any
  other write. This needs policies that read tables (#6869).
- **Policy input from the session token.** A token refresh whose claims
  changed advances the account's policy epoch, and every key of the account
  is re-read.
- Proof: one case per path (G17).

### 6.3 The offline window

A device that stays offline keeps showing what it cached, as Firestore and
PowerSync devices do [FS-OFF]. Encrypting the cache per session does not
close this window: the key is on the device while the session is.

- Ballot B5 no longer offers "no maximum age by default" (G47). The entry
  either sets a maximum offline age, for example
  `.offline(.Queue, max_offline_age: 14d)`, or takes the default. Past the
  age, a cached result shows as `Failed` with "Not available offline".
- An entry that wants no age must say so, and the docs state that with no
  age the revocation window is unbounded on a device that never reconnects.
- The store files use the platform's file protection (iOS Data Protection,
  Android credential-encrypted storage; from knowledge).
- The docs state the window for sign-out, device loss and backups.

## 7. Collections and partitions

### 7.1 Loading a partition

- A partition is one collection under one partition-key value, such as a
  project ID.
- The snapshot is sent in pages. Each page is its own read transaction and
  records its own watermark v_i (3.1). The server never holds one snapshot
  open across requests, which on Postgres would hold back vacuum.
- **Row versions (G48).** A row sent in page i has version v_i, on SQLite and
  on Postgres. After the last page, the server replays every change to the
  partition after the smallest v_i. A replayed change carries its commit
  version, and it applies to a snapshot row only if that version is greater
  than the row's.
- The device shows the partition to queries only once it holds a complete
  checkpoint: every page plus the replay up to the checkpoint version, with
  a matching checksum. It applies the checkpoint in one transaction, as
  PowerSync does [PS-CONS] (G20).
- Proof: a case commits a write between two page reads and shows the row
  once, at the newer value.

### 7.2 Replay merges, and never discards a local edit

Collections are D-SYNC2 data, merged as CRDTs (G49).

- A server change to a row the device holds is merged into the row's CRDT
  state, never written over it. A local edit that is not yet acknowledged
  survives, and both effects show.
- **No checkpoint hold for collections (G74).** The third draft held a
  partition's checkpoint while its uploads were pending, as PowerSync does
  [PS-CONS]. PowerSync holds because a checkpoint replaces local state that
  does not merge. A collection's state merges, and the merge commutes, so
  the hold is dropped:
  - the device keeps each row's server base (its last server state and
    version) apart from its unacknowledged local operations;
  - a server change applies to the base, and the row shown is the base with
    the pending operations merged in;
  - checkpoints and checksums compare the base only (7.5), so a pending
    upload never makes a checkpoint fail.
- A device with an upload pending for days, or with sync off for weeks
  (9.8), therefore stays current for collections.
- **A server delete against a pending edit (G98).** Row presence merges as
  a remove-wins set: a server delete applies to the base as a tombstone, as
  dev's map deletes are tombstones (`Core/Sync/Sync.jet`).
  - The row leaves the screen. Its pending edit is not merged into nothing
    and not dropped: when the queue sends it, the server answers `Deleted`,
    and the edit becomes a kept notice carrying its payload (#6867 c7):
    "You edited a task that was deleted on another device", with export and
    "Restore", which creates a new row from the row's last merged state.
  - A pending local delete of a row the server edited also wins. A delete is
    idempotent, so it needs no notice.
  - With sync off (9.8) the edit is held. The row leaves the screen at the
    server's delete, and the sync status shows "1 change to a deleted row"
    until the person releases the queue, which yields the notice.
- Proof: a case edits a row offline, receives a newer server change to the
  same row, and shows both effects merged after reconnect. A second case
  holds a write with sync off for a simulated week while the server writes
  the same partition, and shows the partition current with the held edit
  merged in. A third, with sync off, has the server delete a row the device
  holds an edit to, and shows the row leave the screen and the release yield
  the notice with the edit's payload.

### 7.3 Rows that move

A row whose partition key changes is a move: a delete in the old partition
and an insert in the new one, both at the same version. A change of sort
key inside a partition is an update. Each row therefore appears exactly
once, in the right partition.

### 7.4 Resume and expiry

- An interrupted load resumes at the last page it received.
- The change buffer for collection tables keeps 7 days. It holds row values,
  so it is a write log for collection tables. Before it is built, its bytes
  per day for Crewboard's seeded write load are measured and recorded here,
  as for the write log of 3.4 (G88).
- If the buffer no longer reaches the smallest v_i, the load restarts from
  the first page.

### 7.5 The row policy, retraction and checksums

- **Refused rows.** The server checks the row policy for each subscriber and
  each row it sends, in snapshot pages and in the change stream, as that
  subscriber. A private task in a shared project is never sent to a member
  who may not read it (G21). For `owner == user` the check is an index
  lookup. This waits for #6869.
- **Rows that leave a subscriber's policy (G50).** When a write changes
  whether a subscriber may read a row, the server sends that subscriber a
  delete for the row. Examples: the task is made private, the member is
  demoted, or the membership is removed. This is the policy form of
  move-out, which Electric's move-out and Replicache's client view record
  both handle (from knowledge).
- **The cost of detecting it (G75).** For a write to a row of a partition
  with S subscribers, the server evaluates the policy twice per subscriber,
  on the stored row and on the new row: 2S evaluations per write. For
  `owner == user` each evaluation is a comparison; a policy that reads a
  table costs its reads. Before partitions are built, the note records the
  evaluations per second and the server CPU for Crewboard with 50
  subscribers per project under its seeded write load.
- **Membership and role changes.** These re-evaluate the subscriber's view
  of each affected partition. The cost is one policy check per row of the
  partition for that subscriber. This is a re-snapshot of that subscriber's
  view, and it travels as row changes.
- **A deploy that changes a row policy (G75).** The hash of the policy is
  part of the partition checkpoint, as the endpoint epoch is part of a cached
  entry (6.1). A deploy that changes it makes every subscriber's checkpoint
  of that collection stale, and each partition re-snapshots at the
  subscriber's next connection: one partition load per subscriber per
  changed policy, stated in the deploy output. The hash covers the policy's
  typed call graph: every function it calls, Core included, as 6.1's
  endpoint epoch does (G99). Configuration a policy reads is not in the
  hash; 6.1's rule for configuration applies.
- **Checksums (G76).** A checkpoint checksum is per subscriber, because
  policies give each subscriber a different row set. It is additive: the
  sum, modulo 2^64, of a 64-bit hash of each (row ID, row version, hash of
  the row's encoded server value) the subscriber holds, so a row whose
  content was corrupted at the same version fails it. The server updates it
  with each change it sends, as PowerSync's bucket checksums are additive
  [PS-CONS]. The device computes it over its server base only (7.2), never
  over merged unacknowledged edits.
- **Where the running sum lives (G99).** Per subscriber and partition the
  server keeps an 8-byte sum, an 8-byte version and the key, about 48 bytes,
  in its memory beside the window state of 4.1. For 50 subscribers each
  reading 20 partitions that is about 48 KiB per project.
  - It is not durable. When it is gone (a restart, a reconnect to another
    server), the server rebuilds it at the subscriber's next checkpoint by
    one pass over the partition's rows, with one policy check per row: the
    cost of a membership re-evaluation.
  - The rebuild's time and the memory are recorded with G75's measurement.
- Proof: a case makes a task private and shows it leaves a member's device
  while online. A second case deploys a changed policy and shows every
  subscriber's partition re-snapshot at its next connection. A third
  corrupts one row's content at the same version and shows the checksum
  fail.

### 7.6 The live hub

Partition changes and invalidations travel over the live hub. It reaches
only the connections that watch the binding or partition, and no event
carries another caller's key: on dev in `1e0820d052` (#6868 c9 met), whose
event is `{"live":"invalidate","binding":<id>}`; the hold's fix adds
`"after_ms":<n>` while the binding is held (3.2). The proof case is
`web_app/live_hub_events_reach_only_admitted_subscribers`. Partition events
use the same watch table when partitions are built.

### 7.7 Proofs

One case for each rule above, including a write committed while the
snapshot streams.

## 8. The web store

### 8.1 One writer at a time per origin

A sync access handle exists only in a dedicated worker [MDN-CSAH], and its
`"readwrite"` mode locks the file exclusively.

**What SQLite's VFSes need (G77).** From SQLite's persistence page
[SQLITE-WASM]:

- "Clients which value performance more than concurrency, or are unable to
  set the COOP/COEP response headers, should use the `opfs-sahpool` VFS.
  Clients which requires multi-tab concurrency should use either the `opfs`
  VFS or `opfs-wl` VFS."
- `opfs` needs `SharedArrayBuffer`, and so the COOP and COEP headers.
  `opfs-wl` differs from `opfs` only in its locking, its name and its need
  for `Atomics.waitAsync()`; the header requirement is not among the
  differences. This note therefore treats `opfs-wl` as needing COOP and
  COEP too [INFERENCE from the page; a test confirms it before the store is
  built]. `opfs-wl` also needs SQLite 3.53.0 or later.
- `opfs-sahpool` needs no COOP or COEP, and allows one connection at a
  time. `pauseVfs()` releases all its sync access handles, so that another
  tab's instance can open them, and "throws if SQLite has any opened file
  handles hosted by this VFS"; the async `unpauseVfs()` takes them back. The
  VFS cannot be installed paused, and two clients installing it at once for
  one origin and VFS name collide, one failing. Only one instance can use a
  directory at a time; another VFS name with its own directory is a
  separate pool. By default a failed install wipes the pool's storage;
  3.54 adds `preserveOnInitFailure`, which keeps it. Several connections in
  one thread meet a locking bug before 3.54. `clearOnInit` is off by
  default.
- With WAL, the WASM build needs exclusive locking mode, which removes all
  concurrency from `opfs` and `opfs-wl`.

The third draft's default, `opfs-wl`, would run only where the deployment
sends COOP and COEP, which breaks cross-origin embeds and popups that do not
opt in (from knowledge). So it is no longer the default.

**The default: `opfs-sahpool`, taken per lease under a Web Lock (G78, G94).**

- Each tab runs SQLite 3.54 or later in its own dedicated worker over
  `opfs-sahpool`, with one connection at a time, opened only inside a lease.
- **One pool per account (G94).** Each account's store is its own pool: a
  VFS name and a directory derived from the app and a hash of the account,
  never the account's name. The Web Lock [MDN-LOCKS] is named for the pool.
  Two tabs signed in to two accounts take two locks and two sets of handles,
  and neither waits for the other.
- **Installing (G94).** A tab installs an account's pool the first time it
  needs it, under that pool's lock, with `clearOnInit` off and
  `preserveOnInitFailure` on, and pauses it before it releases the lock. So
  no two tabs install one pool at once, an install never meets handles
  another tab holds, and a failed install leaves the store intact and is
  retried at the next lease.
- **A lease** runs as:
  1. request the exclusive Web Lock named for the pool;
  2. `unpauseVfs()`;
  3. open the connection;
  4. one SQLite transaction, or a burst of the tab's pending store reads and
     transactions;
  5. close the connection;
  6. `pauseVfs()`;
  7. release the lock.
- A pause with the connection open would throw, so the connection lives only
  inside the lease. Each lease therefore also pays an open and a schema
  read, and starts with a cold page cache and no prepared statements.
- **Reads take the lease too.** `opfs-sahpool` has no concurrent readers. A
  screen's first paint gathers every cache read it needs into one lease
  (S1).
- The lock is held for store work only, never across a network request.
- There is no leader, no heartbeat, no `steal` and no request forwarding.
  Every tab writes its own transactions, so the third draft's handover
  problems (a stolen lock whose holder still has its handles open, a
  resumed leader that writes again, an answer stored apart from its write)
  do not arise. No lock is ever stolen.
- **A tab that hangs or is frozen while it holds the lock** blocks the
  other tabs' cache reads and writes until it resumes, is discarded or is
  closed, which frees the lock (from knowledge). The others wait; after 2
  seconds they say "Another tab is using local data", keep new results in
  memory, and keep queueing writes, because the queue is in IndexedDB under
  its own lock (#6867 c4). Chromium does not freeze a page that holds a Web
  Lock (from knowledge), and the lock is held for milliseconds, so this
  needs a hung tab in practice. It is a stated limit, not a handled case.
- **Journal mode.** `DELETE`, the rollback journal. Never WAL, which would
  need exclusive locking.
- **The cost of a lease** is a required measurement on Chromium, Firefox
  and Safari before the store is built: the time of the whole lease above,
  for a pool sized for one account's store, including the unpause, the open,
  the schema read, a first-paint read of 20 entries, the close and the
  pause. If first paint is too slow under it, the measurement is the
  evidence for recommending `opfs-wl` with COOP and COEP.

**With COOP and COEP.** A deployment that sends both headers, with SQLite
3.53.0 or later and `Atomics.waitAsync()`, may select `opfs-wl`. Each tab's
worker then opens the store directly, and SQLite's own Web Lock locking
replaces the lease. The journal mode is the same. Devtools shows which VFS
runs.

**The queue and the cache on a crash (G79, G95).**

- The queue lives in IndexedDB (#6867 c4) and the cache in OPFS, so no
  commit spans both. The queue's IndexedDB transaction and the store lease
  are never held together: each step commits and releases before the next
  starts, so no lock order arises between them.
- **Prediction path.** A predicted write commits to the queue first, with
  its request ID. Then one cache transaction applies the whole prediction.
  Every cache row it writes adds that request ID to the row's set of
  pending request IDs, so a row two predictions touched records both.
- **Answer path.** When the server answers, one cache transaction:
  - writes the server's result to the rows;
  - removes the request ID from their pending sets;
  - re-applies, over the new server base, the other pending predictions
    those rows still record;
  - records the request ID in a settled table.
- After that commit the queue entry is removed, and the settled ID is
  deleted at the next lease.
- **At launch**, for each queued write:
  - its ID is settled: its answer is stored, so the queue entry is removed
    and nothing is re-derived;
  - some cache row records its ID: its prediction is present;
  - neither: its prediction is re-derived.
- **Each crash window.**
  - Between the queue commit and the prediction: re-derived once at launch.
  - Between the answer's cache commit and the queue removal: the ID is
    settled, so the prediction is not applied again (an increment stays one
    increment).
  - Between the queue removal and the settled ID's deletion: harmless; the
    ID is deleted at the next lease.
  - Two predictions on one row: answering one keeps the other applied over
    the new base.

Proof: a three-tab case writes from every tab, hangs one tab inside a
lease, shows the others' notice and their queued writes, then resumes it.
A case opens two tabs at once on a fresh origin and shows both install the
pool and write. A case signs two tabs in to two accounts and shows neither
waits for the other's lock. A crash case stops in each window above and
shows no prediction applied twice and none missing. A case makes two
predictions on one row, answers the first, and shows the second still
applied. All run on `opfs-sahpool`, and on `opfs-wl` where the test host
sends COOP and COEP.

### 8.2 Persistence and eviction

- The entry calls `navigator.storage.persist()` [MDN-PERSIST]. If the
  browser refuses, the app says so once, and the store is treated as
  best-effort.
- Safari deletes all script-writable storage of a site that is not a
  home-screen web app after seven days of Safari use without a visit
  [WEBKIT-ITP].
  - The device treats a missing store as a cold start.
  - On Safari the app tells the person that unsent changes are kept only
    seven days unless the app is installed.
- **Incognito and guest profiles.** Here `estimate()` can be tiny and
  storage is dropped when the window closes [SQLITE-WASM]. The store runs in
  memory only, the app says so once, and no queue survives the session.
- While unsent writes are queued, closing the tab asks first.

### 8.3 Quota errors

Every store write is one SQLite transaction, so a quota error aborts it
whole and leaves the store consistent. A result that cannot be stored
evicts least-recently-used entries and tries once more. If that fails, the
result is kept in memory only and devtools shows it as not cached.

### 8.4 `QueueFull` before prediction

The queue reserves the bytes of the encoded call, including any upload
bodies, before the handler predicts. If the reservation fails, the handler
fails with `QueueFull` and nothing is predicted, so nothing has to be
rolled back (G16). Proof: a case fills the queue offline and shows that the
next write is refused without a predicted change.

## 9. Local-only mode

### 9.1 One concept: store authority

Ballot B3 gives each store one authority: a whole app's database, a
collection or a table names `.Server` or `.Device` (G34).

- A whole local-only app is every store `.Device`.
- A local drafts collection is that one collection `.Device`.
- Backup, telemetry and account scope follow the same rules for every
  `.Device` store (9.3 to 9.6).

```jet
// Proposed, not built (ballot B3): the desktop entry of a local-only Crewboard.
run: fn() logic.TaskError! {
    crew: logic.open_crew(native.data_path("crewboard.db"))
    screens.crewboard()
        .mount(crew, authority: .Device)
        .window(width: 1200, height: 800)
        .serve()
}
```

`native.data_path` is proposed too. Today `core.sys.data_dir` returns the
XDG data directory, which is not a phone app's data directory.

The screens do not change: they call `app.Remote<Crew>` either way, and
`app.remote(service)` already runs calls in-process (first review R11). A
synced read is an ordinary endpoint call, checked by the same marker and the
same row policy as an online read.

### 9.2 What can carry device data out

- Ballot B4, option A: a type marker for device-authority data. The compiler
  refuses passing a marked value to an `app.Remote` call, a span or log
  attribute, or a sync push without an explicit conversion that names the
  destination.
- Option B: no check. The docs state that device data can be passed to any
  of these, and say why the check was not taken (G25).

### 9.3 Backups and telemetry

**Backup switches per platform (G55).**

- **iOS.** `isExcludedFromBackup` on the store files [APPLE-BACKUP].
- **Android, API 31 and later.** `dataExtractionRules` excludes the store
  directory under both `cloud-backup` and `device-transfer`
  [ANDROID-BACKUP].
- **Android, API 30 and earlier.** `fullBackupContent` excludes it.
  `allowBackup="false"` alone is not used, because on some manufacturers'
  devices it does not stop device-to-device transfer [ANDROID-BACKUP].
- **Desktop.** Time Machine, Windows File History and Linux backup tools copy
  the app data directory. Jet sets no exclusion there. The docs state that
  desktop backups include `.Device` stores.

**The default is ballot B8.**

- For a mixed app, `.Device` drafts and every cache are excluded from
  backup.
- For an app whose every store is `.Device`, the default is a ballot,
  because the two costs are real:
  - Exclusion protects privacy. A person who replaces the phone loses every
    note.
  - Inclusion keeps the data. The backup goes to the person's own cloud,
    end-to-end encrypted on Android 9 and later [ANDROID-BACKUP].
- An entry option `backup: .Include` or `backup: .Exclude` overrides
  either default (G26).

**Telemetry (G56).**

- Under B4 A, spans and crash reports never carry a marked value.
- Under B4 B the runtime cannot tell where a value came from. Spans and
  crash reports then carry endpoint names and sizes, and no argument values
  at all unless the endpoint declares them safe to log.

Proof: a case per platform where the test host can check it, and a
documented decision where it cannot.

### 9.4 Device mounts and mixed services

A service mounted with `authority: .Device` runs in the app's process (G23):

- Jobs and schedules run in-process while the app runs. A schedule that
  fell due while the app was closed runs once at launch, however many times
  it was missed, and receives the count of missed runs.
- Mail, webhooks, push sends, signing keys and account flows are compile
  errors in a client build, naming the feature, as the D-SECRETEDGE1 cases
  require for a service mounted in a client build.
- `.serve()` on desktop and phone opens no listening socket. The window
  calls the service in-process.

**A service with mixed authorities (G54, G83).** In a synced app with a
`.Device` collection:

- A method that touches only `.Server` stores runs on the server.
- A method that touches any `.Device` store runs on the device. It reaches
  `.Server` data only by calling a server method through `app.Remote`,
  which goes through the queue like any write (#6867). A direct `.Server`
  store access in a device method is a compile error that names the store
  and the call that reaches it.
- So the case the third review named, a method that reads a `.Device` draft
  and then writes a `.Server` row, compiles as a device method that calls a
  server method. The server write is its own server transaction; the draft
  read is not part of it.
- A transaction that reaches a `.Server` and a `.Device` store is a compile
  error that names both stores. The fix is two methods, with the device one
  calling the server one.
- A `.Server` table whose column references a `.Device` table is a compile
  error, because the reference would dangle on the server. A `.Device` row
  may reference a `.Server` row.

**How the compiler finds the stores.** Under B3 a store handle's type
carries its authority. A handle passed as a value carries it in its type,
and the check walks the method's typed call graph, the graph the epoch
hashes (6.1), collecting the store types each call reaches. A helper generic
over the authority is checked once per instantiation. A value whose type
hides the authority of a store it holds is refused where it is made.

Proof: a case per rule, and a diagnostic snapshot for each of the three
compile errors (a mixed transaction, a `.Server` access in a device method,
a server-to-device reference).

**Publishing a draft (G101).** A device method that reads a `.Device` draft,
queues a server create through `app.Remote` and deletes the draft returns
before the server answers, and offline it cannot wait for the answer. If the
server later refuses the create, the draft is gone from the `.Device` store,
but nothing is lost: the kept notice of a refused write (#6867 c7) carries
the write's encoded arguments, so the person can edit and retry it, or
export it, from the notice. Proof: a case publishes a draft offline, has the
server refuse the create after the method returned, and shows the notice
holding the draft's content.

### 9.5 Device migrations

- Every device store has a schema version (G24).
- **Caches are dropped and rebuilt, not migrated (G63).** They are rebuilt
  by catch-up and re-reads.
- **`.Device` stores are copied, then migrated.**
  - The copy is taken with SQLite's backup API, or with `VACUUM INTO` after
    a WAL checkpoint. A raw copy of a WAL database is not consistent.
  - A free-space check runs first: the store's size plus 10 percent. A disk
    that cannot hold the copy refuses the migration with a typed notice, and
    keeps the old store readable under the old app's schema.
  - If the migration fails, the copy is restored.
  - A crash mid-migration leaves the copy and a marker. The next launch
    restores the copy and migrates again.
- A store whose schema is newer than the app (a downgrade) is refused with
  a typed notice and left untouched.
- Proof: a case migrates a local-only database across an update, survives a
  crash mid-migration, and refuses on a full disk.

### 9.6 Accounts and deletion

- Every device store, cache or `.Device` data, is one file per account (G27).
  A local-only app with no sign-in uses one device account per OS user.
- In a mixed app, `.Device` drafts belong to the account that wrote them.
  Person A writes drafts and signs out. Person B signs in, sees none of
  them, and never reads A's cached results.
- **Drafts with no signed-in owner (G62).** In a mixed app, `.Device` data
  written while no one is signed in belongs to the OS user's device
  account, as in a local-only app. A sign-in moves none of it. The person
  can join it to the account through 9.7.
- Proof: a case signs in as A, writes a draft, switches to B, and shows
  both. A second case writes a draft signed out, signs in, and shows that
  the draft stays with the device account.
- Sign-out and "Remove local data" run a WAL checkpoint with truncate, then
  delete the files. Stores run with SQLite `secure_delete` on, so deleted
  rows are overwritten while the store lives [SQLITE-PRAGMA].
- `secure_delete` does not cover deleting the whole file. On flash storage,
  wear leveling can also keep old blocks. The platform's disk encryption is
  the protection there, and the docs say so (G28).

### 9.7 Switching sync on later

Local rows use client-made UUIDv7 IDs (`app.new_id`, #6867 c6), so they
reach a server without renumbering. Switching on (G29, G30):

- **The account.** It asks which account the data joins. It never joins
  whichever account happens to be signed in.
- **Order.** It uploads rows in dependency order, references before
  referrers. Each row goes through its normal create method under that
  account's session, so every policy runs. A reference cycle is uploaded as
  create, then update.
- **Request keys.** It gives each row a request key (row ID plus device ID).
  A retry after a crash resends with the same key.
- **Batches.** It sends rows in batches of 100 and records progress per row,
  so an interrupted upload resumes where it stopped.
- **Unsupported fields.** It reports per row any field the create method
  cannot carry ("`color` is not accepted by `create_task`"). No field is
  dropped silently.
- **Refused rows (G82).** A refused row moves, with every row that depends
  on it, to the account's "Not synced" store: one small `.Device` file per
  account, with a kept notice per row (#6867 c7). The person can edit and
  retry a row there, export it, or delete it. Moving the rows out is what
  lets the joined file retire.
- **Seeded rows (N5, G52, G80, G96).**
  - A seed's ID is a UUIDv7 like any other, never derived from the app and
    seed name.
  - The server keeps a framework table `jet_seeds`: (account, seed name,
    table, row ID, deleted), unique on (account, seed name). It replaces the
    fourth draft's `jet_seed` column.
  - A seed uploads through its create method with its seed name. In the
    create's transaction the server inserts the `jet_seeds` row first. If
    the key already exists, the create answers from it instead of inserting:
    `SeedExists` with the row's ID while the seed lives, `Deleted` once it
    was deleted. The unique key decides, not an earlier lookup, so two
    devices of one account that join at once get one row per seed: the
    first create wins, and the other device renumbers.
  - Deleting a seeded row marks its `jet_seeds` row deleted in the same
    transaction: through Jet's delete path, or, for deletes made outside
    Jet, through the synced table's delete trigger on SQLite and the log
    reader on Postgres (3.1). The `jet_seeds` row stays, one per seed per
    account, so a seed deleted on one device is never re-created by a join
    months later (G96).
  - A device whose seed answers `Deleted` drops its local seed row if the
    person never changed it, and otherwise moves it to the "Not synced"
    store with the notice "deleted on another device".
  - At join the device may first ask for the account's seeds by name, which
    saves uploads but decides nothing.
  - Two accounts never share a seed ID. A seed edited on both devices
    follows the queue's rebase rules (#6867), as an update of the kept row.
- **Renumbering (G81).** A seed whose ID changes is renumbered in this order,
  one transaction per step:
  1. a renumber journal (old ID, new ID) is written to the joining store;
  2. queued writes that name the old ID are rewritten;
  3. rows and references in the joining store, then in every other `.Device`
     store of the account, are rewritten;
  4. cached results that contain the old ID are dropped and re-read.

  Each step marks its progress in the journal. A launch finishes an
  unfinished journal before anything uploads, and no row that references
  the old ID uploads before step 3 is done.
- **The join record (G53, G82, G97).** When a join starts, the server
  creates a join record for (account, device, join ID). It lists the ID and
  table of every row created under the join. It is kept until the device
  reports the join complete, and at most 30 days. Its size is one entry per
  uploaded row, freed at completion.
  - A listed row is marked deleted when any device deletes it, through Jet's
    delete path, and when it is deleted outside Jet, through the synced
    table's delete trigger on SQLite and the log reader on Postgres (3.1).
  - **Every create under a join consults the record for its row ID (G97)**,
    whether or not a row with that ID exists, and whatever the age of its
    request key (#6867 c2):
    - listed and live: success, the device's own earlier upload;
    - listed and deleted: `Deleted`. The ID is not inserted again, though no
      row has it. The local row moves to the "Not synced" store with the
      notice "deleted on another device";
    - not listed, and a row with that ID exists: `Exists`;
    - not listed and no row: the row is inserted and listed.
  - **After 30 days (G97).** The record is deleted. A device that resumes
    the join later gets `JoinExpired`: its rows the server confirmed before
    stay, and every row not yet confirmed moves to the "Not synced" store
    with the notice "upload interrupted for more than 30 days", to edit and
    retry row by row. It never re-uploads on its own, so a row deleted
    elsewhere does not come back.

  No table needs an owner column for this, so join tables and child rows
  owned through a parent are covered.
- **The end state.** The joined file retires once every row is confirmed by
  the server or moved to the "Not synced" store. The joined data's store
  becomes that account's device cache (O4 A). The local `.Device` file is
  kept read-only until the first complete catch-up, then deleted under 9.6's
  rules. Stores that stay `.Device` are untouched.

Proof: a case with 10,000 rows, dependent rows and a crash halfway; a case
with two accounts; a case where two devices of one account join at once and
end with one row per seed; a case that crashes between renumbering steps; a
case that resumes after a simulated 48 hours, after another device deleted
one uploaded row; a case that resumes after a row was deleted with plain SQL
outside Jet; a case that resumes after a simulated 31 days and shows the
unconfirmed rows in "Not synced"; a case that deletes a seed on one device
and joins a second device a simulated month later, with no seed re-created;
a case with one refused row that shows the joined file retire.

### 9.8 Switching sync off for a synced app

For server-owned data (O4 A) there is no local authority to fall back on.
Sync off holds writes only (G31):

- While online, reads still go to the server and pushes still arrive. That
  includes collections: a held write does not hold a partition's
  checkpoint (7.2, G74).
- Writes are predicted and queued, and the queue is held. The status shows
  "Sync is off: 3 changes waiting".
- A held write older than 7 days is flagged "Waiting 7 days, may conflict".
  The entry can change that age.
- Signing out while writes are held asks whether to discard them, and never
  sends them under another account.
- When sync is switched back on, the queue is released and rebased (#6867).
- Proof: a case.

## 10. Failure modes

| Failure | Behavior |
|---|---|
| A live result grows past the cap | `Failed(CallError.TooLarge)` naming the endpoint, cap and size; stored, not re-run on each read (4.2) |
| Disk full or quota exceeded mid-write | The store transaction aborts whole; the entry is evicted, retried once, then memory-only (8.3) |
| Disk full mid-snapshot | The partition stays hidden; the load resumes at the last page once space frees (7.1) |
| Disk full mid-upload (9.7) | Per-row progress is kept; the upload resumes |
| Disk full before a device migration | Refused with a notice; the old store stays readable (9.5) |
| The queue is full | `QueueFull` before prediction; nothing is rolled back (8.4) |
| A cached entry fails its checksum | The entry is deleted and re-read (3.6) |
| A cache store fails SQLite's integrity check | It is deleted and rebuilt by re-reads. On desktop and phone the queue is a table in that file: its rows that pass are exported first and restored into the new store. On the web the queue is in IndexedDB and is untouched (2.1) |
| A `.Device` store fails SQLite's integrity check | Never deleted (G57). It is kept and opened read-only if SQLite can. The app refuses it with a typed notice and offers an export of what can be read. Writes go nowhere until the person chooses |
| Device clock wrong or changed | Versions come from the server. The age shown is the device's time since it received the result, from the device's own receipt time, never server time subtracted from the device clock. A device clock moved backwards shows "just now", never a negative age (G61) |
| Server restore, versions going backwards | Store id, epoch and ledger checks re-read every key (3.7) |
| A restore that skips the documented procedure | The ledger entry written at every server start, or the missing entry, re-reads every key (3.7) |
| A restore of a running server, without a restart | The poll finds it within a second and advances the epoch; every server closes its live connections; devices re-read within about two seconds (3.7, G93) |
| A device away more than 30 days | Its ledger entry is pruned and counts as missing; it re-reads every key (3.7, G93) |
| The log reader is down or behind | Its slot retains WAL on the primary, bounded by `max_slot_wal_keep_size`, which the deployment must set (from knowledge). Catch-up answers "unknown" after 10 seconds; devices keep showing entries with their age and retry with jittered backoff, so no device re-reads every key during the incident (3.5, G67) |
| The replication slot is invalidated or recreated | Writes while it was gone were never decoded: the store epoch advances, every server invalidates every binding it holds, and every device re-reads (3.1, G90) |
| The log reader restarts after recording commits but before confirming them | The slot re-sends them; recording them again changes nothing (3.1, G90) |
| Catch-up before Postgres 17 | The marker is a transactional message in its own transaction, whose commit flushes it; one xid per marker (3.5, G91) |
| Catch-up on a quiet database | The catch-up marker is decoded at once; no timeout (3.5, G66) |
| Writes made outside Jet | Seen through decoded WAL or triggers (3.1). A synced SQLite table without its triggers, or rebuilt by `db.migrate`, advances the epoch within one second (G68) |
| A trigger dropped and re-created between two polls, or any schema change Jet did not make | `schema_version` differs from the value Jet recorded: the epoch advances and every device re-reads (3.1, G92) |
| An outside connection with triggers off, or incremental BLOB writes | Not seen until the next epoch advance; documented (3.1, G92) |
| A binding whose run is longer than the gap between writes | From the second overtaken run it serves its stored result, stale, for one hold; it spends at most half its time running (3.2, G86) |
| A device reads a binding inside a hold | It gets the held result and an event with `after_ms`; it reads again after it, and the first current result after the hold is published again (3.2, G89) |
| A server holds no binding of a written table | Nothing to invalidate there; another server learns of the write through the version map poll within one second (3.4, G70) |
| A subscriber's window state is gone (restart, another server, a rolling deploy) | The device sends its window's row hashes; the server sends only the rows that changed (4.1, G71, G100) |
| The change buffer no longer reaches a partition | The load restarts from the first page (7.4) |
| A deploy changes a row policy, or a function the policy calls | Every subscriber's partitions of that collection re-snapshot (7.5, G75, G99) |
| A subscriber's running checksum is gone | Rebuilt at its next checkpoint with one policy check per row (7.5, G99) |
| The server deletes a collection row that has a pending local edit | The delete wins; the edit becomes a kept notice with its payload (7.2, G98) |
| An upload pending for days, or sync off for weeks | Collections keep applying server changes; the checkpoint compares the server base only (7.2, G74) |
| A crash during the sync-on upload | Request keys and per-row progress resume it. After 24 hours the join record confirms it (9.7) |
| A resumed join meets a row another device deleted, or one deleted outside Jet | `Deleted`, from the join record; the row moves to the "Not synced" store with a notice (9.7, G82, G97) |
| A join resumed after 30 days | `JoinExpired`; every unconfirmed row moves to the "Not synced" store with a notice; nothing re-uploads on its own (9.7, G97) |
| Two devices of one account join at once | The `jet_seeds` key admits one row per seed; the other device renumbers (9.7, G80) |
| A seed deleted on one device, and another device joins later | `jet_seeds` keeps the deleted seed; the create answers `Deleted` (9.7, G96) |
| A crash while renumbering a seed | The renumber journal resumes it at launch (9.7, G81) |
| A failed device migration | The copy is restored; a downgrade is refused (9.5) |
| An app updated while offline | Cached entries render with their age; keys whose epoch changed are re-read at reconnect (6.1) |
| A second tab of a web app | It takes the pool's Web Lock per lease, for reads and writes (8.1) |
| Two tabs open at once on a fresh origin | Installation runs under the pool's lock and pauses before release; a failed install keeps the store (8.1, G94) |
| Two accounts in two tabs | Two pools and two locks; neither waits (8.1, G94) |
| A tab hangs or is frozen while it holds the store's lock | Other tabs' cache reads and writes wait; after 2 seconds they say "Another tab is using local data", keep results in memory and keep queueing in IndexedDB. The lock frees when the tab resumes, is discarded or is closed (8.1, G78) |
| A crash between a prediction's queue commit and its cache commit | Launch re-derives it once (8.1, G79) |
| A crash between the answer's cache commit and the queue removal | The request ID is settled in the cache; launch removes the queue entry and re-derives nothing (8.1, G95) |
| The server refuses a write a device method queued after deleting its device data | The kept notice carries the write's payload (9.4, G101) |
| Incognito or guest storage | Memory only, said once; no queue survives the session (8.2) |
| Safari's seven-day eviction | A cold start; the person was told about the queue (8.2) |
| The browser refuses persistent storage | The app says so once; the store is best-effort (8.2) |
| A broker outage loses invalidations | The version map is polled every second; nothing is lost (3.4) |
| Schedules missed while the app was closed | Run once at launch with the missed count (9.4) |
| Access revoked while offline | Readable until the next connection or the maximum offline age (6.3) |
| Local data joins an account and the server refuses rows | Each refused row and its dependents move to the account's "Not synced" store with a kept notice; the joined file retires (9.7, G82) |

Each row has a case before the part it covers is built (G32, G58).

## 11. Tooling

- Devtools shows each device-store entry's key, account, bytes, version,
  epoch, age, warm rank and pinned flag. It also shows the result of the
  last catch-up: keys sent, keys re-read and why, including "log reader
  behind" (G33).
- A `jet inspect` view of the same data is a new command surface, so it is
  ballot B6.
- The test host (`testing.world`) needs two abilities:
  - disconnect a device for a simulated duration and restore it mid-test,
    which is #3709 c28;
  - restore the server's database to an earlier version. On SQLite a case
    does that without a new ability: it copies the store with the backup
    API, writes, stops the server and puts the copy back before the
    restart. On Postgres it needs the Postgres test host that #6868 c4's
    broker case needs too.
- `app.live_stats()` adds stored bytes, the lowest and highest versions
  held, and the catch-up counts.

## 12. Gap ledger

### 12.1 Fourth review (G89 to G101, and the carried gaps)

| Gap | Where | Answer |
|---|---|---|
| G5 (carried) | 3.4 | The write log waits for #6554 and its two measurements |
| G10 (carried) | 5.3 | Owned by #3709 c27, open. Run-time listing until then |
| G14 (carried) | 2.3 | Defaults pending measurement, replaced before the store is built |
| G41 (carried) | 3.2, 3.3 | Dev's event has no version yet. Held results' events carry `after_ms` and the hold's end publishes again, so "every event counts as newer" holds under the hold. With versions, a held result carries its run's version and a stale mark |
| G45 (carried) | 4.2, B7 | Ballot B7 |
| G64 (carried) | 11 | #3709 c28, open. A server restore needs no new ability on SQLite |
| G88 (carried) | 7.4 | The change buffer is a required measurement |
| G89 | 3.2, 3.3, 10, 1 | The hold's start and invalidations during it publish `after_ms`; the first current result after a hold publishes again; written in `Core/App/App.jet` with `web_app/live_hold_tells_watchers_when_to_read_again` (lane SUB, not yet on dev) |
| G90 | 3.1, 10 | `xid8` by epoch extension; `pgoutput` streaming and two-phase off, messages on; idempotent record writes confirmed after commit; prune below the pruning transaction's own snapshot xmin, no privilege; a recreated slot advances the epoch and invalidates every server's bindings; reader-restart and slot cases |
| G91 | 3.1, 3.5 | Write amplification (rows, WAL bytes, vacuum) a required measurement; `flush` on 17, a transactional marker before; the reader notifies the waiting server, with a 50 ms poll of C; the emit-to-answer latency measured |
| G92 | 3.1, 10 | Any `schema_version` Jet did not record advances the epoch; triggers-off connections and BLOB writes documented; a drop, write and re-create case |
| G93 | 3.7, 10 | Strictly below, in 3.7 and here; a restore the poll finds advances the epoch and closes live connections, a two-second window stated; sizes per store kind; one minute entry per store; a row for a device away more than 30 days |
| G94 | 8.1, 1, 10 | Open inside unpause, close before pause; install under the pool's lock, then pause, with `preserveOnInitFailure`; one pool and lock per account; reads take the lease; SQLite 3.54, one connection per worker; the lease measurement includes open, schema read and first paint; fresh-origin and two-account cases |
| G95 | 8.1, 10 | The answer path settles the request ID in the cache before the queue removal; rows record every request ID; the two locks are never held together; a case per crash window and for two predictions on one row |
| G96 | 9.7, 10 | `jet_seeds` keeps deleted seeds and answers `Deleted`; outside deletes mark it; a case |
| G97 | 9.7, 10 | Every create under a join consults the record; outside deletes mark it; after 30 days `JoinExpired` moves unconfirmed rows to "Not synced"; cases |
| G98 | 7.2, 10 | Remove-wins presence; the pending edit becomes a kept notice with its payload; a sync-off case |
| G99 | 7.5, 10 | The running sum in server memory, about 48 bytes per subscriber and partition, rebuilt at the next checkpoint; measured with G75; the policy hash covers its call graph |
| G100 | 4.1, 10 | The device's row-hash list on re-subscription; the rolling-deploy burst measured with and without it |
| G101 | 9.4, 10 | The kept notice of a refused write carries its payload; a case |

### 12.2 Third review (G65 to G88, and the carried gaps)

| Gap | Where | Answer |
|---|---|---|
| G5 (carried) | 3.4 | The write log waits for #6554 and its two measurements |
| G10 (carried) | 5.3 | Owned by #3709 c27: the build reports warm coverage per screen, and a check fails an offline screen that reads outside it. Run-time listing until then |
| G14 (carried) | 2.3 | Defaults pending measurement, replaced before the store is built |
| G22 (carried) | 7.6 | Closed on dev (`1e0820d052`, #6868 c9 met) |
| G41 (carried) | 3.3 | Dev's event `{"live":"invalidate","binding":<id>}` has no version; it gains one when versions are built. Until then every event counts as newer |
| G45 (carried) | 4.2, B7 | Ballot B7 |
| G64 (carried) | 11 | Disconnect and restore mid-test: #3709 c28. A server restore needs no new ability on SQLite (the case copies the store file); on Postgres it shares #6868 c4's Postgres test host |
| G65 | 3.1 | The commit record: top-level xid and commit LSN per decoded commit, written with C in one transaction, pruned below the oldest open snapshot's xmin, read inside the reading snapshot; a case with an in-progress transaction that commits below C |
| G66 | 3.5 | Catch-up writes a non-transactional marker and waits for the reader to decode it; a quiet-database case |
| G67 | 3.5, 10 | Past the timeout the answer is "unknown": entries keep their age, the device retries with jittered backoff |
| G68 | 3.1 | `db.migrate` advances the epoch when it rebuilds a synced table; `schema_version` and `data_version` are checked every second; BLOB writes stated; the trigger cost is a required measurement |
| G69 | 3.4, 3.7 | One ledger entry per minute with writes and one per server start; the device keeps the latest entry strictly below its version; 3.4's growth claim narrowed to the map; ledger size stated (revised as G93) |
| G70 | 3.4 | The hub is per process; other servers learn through the version map poll, one second; a two-server case |
| G71 | 4.1 | Per subscriber: (row ID, value hash) per window row, in server memory, about 32 KiB for 1,000 rows; not durable; a whole window once when it is gone; measured for 200 subscribers |
| G72 | 5.1 | The online re-check follows the warming rule on metered and unknown networks |
| G73 | 6.1 | Core sources on the call path change the epoch; trivia does not; configuration is not hashed, documented, with the store epoch as the lever |
| G74 | 7.2, 9.8 | The checkpoint hold is dropped for collections; checkpoints compare the server base only; a simulated-week case |
| G75 | 7.5 | Two policy checks per subscriber per write, measured; the policy hash is part of the partition checkpoint, so a deploy that changes it re-snapshots; configuration inputs documented |
| G76 | 7.5 | The checksum covers a hash of each row's value, over the server base only |
| G77 | 8.1 | `opfs-wl` needs COOP and COEP (the SQLite page groups it with `opfs` for that), 3.53.0 and `Atomics.waitAsync()`; the default is `opfs-sahpool`; journal mode `DELETE`, never WAL |
| G78 | 8.1 | No leader and no `steal`: each tab takes a Web Lock per lease (revised as G94) |
| G79 | 8.1 | Cache rows carry the request ID of their prediction; launch re-derives only predictions the cache lacks (revised as G95) |
| G80 | 9.7 | The server's seed key admits one row per seed; the losing device renumbers (`jet_seeds` since G96) |
| G81 | 9.7 | Renumbering covers the queue, then `.Device` rows, then caches, with a journal that resumes after a crash |
| G82 | 9.7 | The join record replaces owner columns; refused rows move to a "Not synced" store, so the file retires; a deleted row is answered `Deleted` |
| G83 | 9.4 | A method that touches a `.Device` store runs on the device and reaches the server only through `app.Remote`; store authority is in the store handle's type; three diagnostic snapshots |
| G84 | 1 | Table-level invalidation cited by its code |
| G85 | 4.1 | A windowed endpoint returns rows with an ID; others are resent whole |
| G86 | 3.2 | On dev (`8630ccf4dd`, #6868 c11 met): from the second overtaken run, a stale binding serves its stored result for one hold |
| G87 | 1, 3.2 | Updated to dev |
| G88 | 7.4 | The change buffer is a required measurement, like G5 |

### 12.3 Second review (G36 to G64, and the carried gaps)

| Gap | Where | Answer |
|---|---|---|
| G5 (carried) | 3.4 | The write log waits for #6554; the two measurements are required before it is built |
| G10 (carried) | 5.3 | Owner criterion #3709 c27 (see 12.2) |
| G14 (carried) | 2.3 | Defaults pending measurement, replaced before the store is built |
| G22 (carried) | 1, 7.6 | Closed: #6868 c9 is met on dev in `1e0820d052` |
| G36 | 3.2 | Closed: on dev in `2ded9a1ded`, with its case (#6868 c8 met) |
| G37 | 3.1, 3.5 | W at most the reader's position; catch-up waits for it; replica replay position; the reader's table outside the publication |
| G38 | 3.1 | Decoded WAL on Postgres, triggers on SQLite; a missing trigger advances the epoch |
| G39 | 3.4 | Per-table version map; the log only with #6554 |
| G40 | 3.5 | 4 KiB cap per summary; device freshness only |
| G41 | 3.3 | A version on every event and answer; #6868 c9's format gains it |
| G42 | 5.1 | Re-checked at screen open and every 15 minutes online |
| G43 | 5.1 | Per-item and per-batch caps; streamed; one oversized item fails alone |
| G44 | 4.1, 5.4 | Row changes keyed by ID; re-run cost measured before building; past-cap pages offline-unreachable |
| G45 | 4.2, B7 | Fact shown, not judged by name; a row-count pin, enforced |
| G46 | 2.2, 6.1 | Epoch in the entry, from the server; typed call graph, not generated code |
| G47 | 6.3, B5 | No "none by default"; an unbounded window stated |
| G48 | 7.1 | Snapshot row version is the page watermark; replay applies only if newer |
| G49 | 7.2 | CRDT merge. The checkpoint hold is dropped for collections (G74) |
| G50 | 7.5 | Deletes on policy loss; membership re-evaluation cost; additive checksums |
| G51 | 8.1 | Replaced after the third review: `opfs-sahpool` taken per transaction under a Web Lock (G77 to G79) |
| G52 | 9.7 | UUIDv7 seeds; concurrent joins answered by G80 and G81 |
| G53 | 9.7 | Replaced: the join record decides confirmation (G82) |
| G54 | 9.4 | Methods split by store; mixed transactions and server-to-device references are compile errors |
| G55 | 9.3, B8 | Android's three switches, iOS, desktop stated; the default is a ballot |
| G56 | 9.3 | Under B4 B, no argument values unless declared |
| G57 | 10 | Corrupt `.Device` store kept, refused, exportable; queue location per platform |
| G58 | 10 | Every named failure has a row |
| G59 | 1, throughout | Live criteria: #6868 c5, #6867 c6, #6867 c7. Reopened as G84 and answered there |
| G60 | 3.2 | Race narrowed by `5fa1fd1c85`, closed by G36 |
| G61 | 10 | Ages from the device's receipt time |
| G62 | 9.6 | Signed-out drafts belong to the device account |
| G63 | 9.5 | Caches dropped; backup API or `VACUUM INTO`; free-space check |
| G64 | 11 | Disconnect: owner criterion #3709 c28. Restore: see G64 in 12.2 |

### 12.4 First review (G1 to G35)

G1 is the precondition table (section 1).

| Gap | Where | Answer |
|---|---|---|
| G2 | 1, 3.5 | Read-set dependence and table-level cost stated |
| G3 | 3.1, 3.2 | Commit-ordered versions; publish after commit |
| G4 | 3.3 | Monotonic store rule |
| G6 | 3.6 | Per-entry and per-checkpoint checksums |
| G7 | 4.1 | One live window per list |
| G8 | 4.2 | Typed `TooLarge`, stored; encoder stops at the cap |
| G9 | 4.3 | Waits for #6827 c3 and #6869 |
| G11 | 5.1, 5.2 | One-shot reads; batched loops; fallback without network type |
| G12 | 5.4, B2 | Limit stated; row-store ballot |
| G13 | 2.1, 2.3 | Persistent per-entry store; one limit; LRU; pinned entries |
| G15 | 8.1, 8.2, 8.3 | Multi-tab store; persist refusal; Safari; quota |
| G16 | 8.4 | `QueueFull` before prediction |
| G17 | 6.1, 6.2 | Endpoint epochs; session and policy revocation |
| G18 | 6.3, B5 | Maximum offline age; file protection; window stated |
| G19 | 2.2 | Canonical, stable argument encoding |
| G20 | 7.1, 7.3, 7.4 | Per-page versions, replay, hidden until complete, moves, restart |
| G21 | 7.5 | Per-subscriber policy checks |
| G23 | 9.4 | Device-mount rules; `.serve()` opens no socket |
| G24 | 9.5 | Schema versions; copy, migrate, restore; downgrade refused |
| G25 | 9.2, B4 | Marker ballot, with the unchecked option stated |
| G26 | 9.3, B8 | Backup rules per platform; the default is a ballot |
| G27 | 2.2, 9.6 | Account in every key; one file per account |
| G28 | 9.6 | Checkpoint, truncate, delete; `secure_delete`; flash limit stated |
| G29 | 9.7 | Dependency order, request keys, batches, per-row report |
| G30 | 9.7 | Seeds matched by name; rebase for rows edited twice |
| G31 | 9.8 | Reads and pushes continue; 7-day flag |
| G32 | 10 | Every named failure has a row |
| G33 | 11 | Devtools, test host, live_stats |
| G34 | 9.1, B3 | One authority concept |
| G35 | 3.7 | Store id, epoch and commit nonce |

## 13. Owner ballots

These are drafts with alternatives. None is ratified.

- **B1, the warm set.**
  - A: `.offline(.Queue, warm: fn)` in the entry (section 5).
  - B: a `.warm()` mark on screens, so the warm set is the marked screens.
  - C: no warm set: offline reads only what was seen, and the docs say so.
- **B2, what the device stores.**
  - A: one blob per result, keyed by endpoint and arguments (this note). It
    cannot serve offline search.
  - B: a normalized row store with local queries, as Zero and PowerSync
    have, so offline search and new filters work.
- **B3, store authority.** Each store names `.Server` or `.Device` (9.1).
- **B4, a marker for device data.**
  - A: a type marker the compiler checks (9.2).
  - B: unchecked, and documented; telemetry carries no argument values
    unless declared (9.3).
- **B5, a maximum offline age.**
  - A: the entry must choose an age, or explicitly none.
  - B: a default of 30 days, settable per entry.
- **B6, a `jet inspect` view of the device store.** A new command surface,
  or devtools only.
- **B7, the bound pin on a list endpoint (4.2).** Its spelling: an
  attribute on the endpoint, or a bounded list type in its return type.
- **B8, backup of an app whose every store is `.Device` (9.3).**
  - A: excluded by default, for privacy, at the cost of losing the data when
    the device is replaced.
  - B: included by default, with caches excluded, for durability, at the
    cost of the data reaching the person's platform backup.

## 14. What the fifth review must attack

- The hold's events (3.2): whether a device that honors `after_ms` can
  still keep a held result as current, including a watcher that joins
  during a hold and receives the topic's replayed latest event, and server
  code that reads the held result itself.
- The `xid8` extension and the prune horizon taken from the pruning
  transaction's own snapshot (3.1), across a reader restart and a recreated
  slot.
- The catch-up marker before Postgres 17 (3.5): a transactional message in
  its own transaction, and the notification by which the server learns of
  it.
- The `schema_version` rule (3.1): which outside operations advance the
  epoch, and what the spurious re-reads cost.
- The live-restore path of 3.7: closing every live connection, and the
  two-second window.
- The lease of 8.1 as now ordered, its cost with an open and a schema read
  per lease, installation under the lock, and one pool per account.
- The answer path of 8.1: the settled table, and rows that record several
  request IDs.
- `jet_seeds` and the join record's expiry (9.7).
- Delete-wins presence for collections (7.2), and the device's row-hash
  list on re-subscription (4.1).

## Sources

| Key | Source |
|---|---|
| ZERO-Q | [Zero: Queries (preload, TTLs, limited sorted queries)](https://zero.rocicorp.dev/docs/queries) |
| PS-STREAMS | [PowerSync: Sync Streams](https://docs.powersync.com/usage/sync-streams) |
| PS-CONS | [PowerSync: Consistency (checkpoints, checksums)](https://docs.powersync.com/architecture/consistency) |
| ELEC-HTTP | [Electric: HTTP API (must-refetch, snapshot-end metadata)](https://electric-sql.com/docs/api/http) |
| FS-OFF | [Firestore: Access data offline](https://firebase.google.com/docs/firestore/manage-data/enable-offline) |
| PG-INFO | [PostgreSQL: System information functions (`pg_current_snapshot`, `pg_snapshot_xmin`, `pg_visible_in_snapshot`, `pg_current_xact_id`; `xid8` does not wrap, `xid` does)](https://www.postgresql.org/docs/current/functions-info.html) (opened) |
| PG-ADMIN | [PostgreSQL: System administration functions (`pg_logical_emit_message`, its `flush` argument; "flush has no effect with transactional")](https://www.postgresql.org/docs/current/functions-admin.html) (opened) |
| PG-PROTO | [PostgreSQL: Logical streaming replication protocol (`pgoutput` options `messages`, `streaming`, `two_phase`)](https://www.postgresql.org/docs/current/protocol-logical-replication.html) (opened by the fourth review; not reopened for this draft) |
| PG-17 | [PostgreSQL 17 release notes ("Add flush option to pg_logical_emit_message()")](https://www.postgresql.org/docs/17/release-17.html) (opened by the fourth review; not reopened for this draft) |
| PG-LOGICAL | [PostgreSQL: Logical decoding output plugins (commit callback with `commit_lsn`)](https://www.postgresql.org/docs/current/logicaldecoding-output-plugin.html) |
| PG-NOTIFY | [PostgreSQL: NOTIFY (payload shorter than 8000 bytes)](https://www.postgresql.org/docs/current/sql-notify.html) |
| PG-STATS | [PostgreSQL: The cumulative statistics system (`backend_xmin`; other sessions' columns hidden from ordinary roles)](https://www.postgresql.org/docs/current/monitoring-stats.html) (opened by the fourth review; not reopened for this draft) |
| SQLITE-PRAGMA | [SQLite: PRAGMA (`secure_delete`, `data_version`, `schema_version`)](https://sqlite.org/pragma.html) (opened for this draft) |
| SQLITE-WASM | [SQLite WASM: Persistent storage options (opfs, opfs-wl, opfs-sahpool; `pauseVfs` throws with open files, `unpauseVfs`, install collisions, `directory`, `name`, `clearOnInit`, `preserveOnInitFailure` in 3.54, the locking bug before 3.54)](https://sqlite.org/wasm/doc/trunk/persistence.md) (opened) |
| MDN-CSAH | [MDN: FileSystemFileHandle.createSyncAccessHandle() (dedicated workers only; lock modes)](https://developer.mozilla.org/en-US/docs/Web/API/FileSystemFileHandle/createSyncAccessHandle) |
| MDN-LOCKS | [MDN: Web Locks API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Locks_API) |
| MDN-PERSIST | [MDN: StorageManager.persist()](https://developer.mozilla.org/en-US/docs/Web/API/StorageManager/persist) |
| WEBKIT-ITP | [WebKit: Full Third-Party Cookie Blocking and More (7-day cap on all script-writeable storage)](https://webkit.org/blog/10218/full-third-party-cookie-blocking-and-more/) |
| APPLE-BACKUP | [Apple: isExcludedFromBackup](https://developer.apple.com/documentation/foundation/urlresourcevalues/isexcludedfrombackup) |
| ANDROID-BACKUP | [Android: Back up user data with Auto Backup (dataExtractionRules, fullBackupContent, device-to-device transfer)](https://developer.android.com/identity/data/autobackup) |
| Code | `Core/App/App.jet`, `Core/DB/DB.jet`, `Compiler/JetRuntime/Source/WsRoutes.jet` and `Tests/Cases/WebApp/` at dev `1e7819b621`; commits `2ded9a1ded`, `1e0820d052`, `8630ccf4dd`; the hold's fix in lane SUB `GoalsB-5-6868-hold`, with `web_app/live_hold_tells_watchers_when_to_read_again` |

These facts come from knowledge and were not opened for this note:

- Postgres commit-order versus visibility-order behavior,
  `max_slot_wal_keep_size`, and LSNs continuing past a point-in-time
  restore on a new timeline;
- `pg_last_wal_replay_lsn()` on replicas, and hot-standby visibility in
  replay order;
- that logical decoding emits a transaction at its commit record, in commit
  LSN order, when `streaming` is off;
- `wal_writer_delay` and its 200 ms default, and that `NOTIFY` is delivered
  at its transaction's commit;
- that incremental BLOB writes (`sqlite3_blob_write`) fire no triggers, that
  `SQLITE_DBCONFIG_ENABLE_TRIGGER` turns triggers off for a connection, and
  that `ANALYZE` and `VACUUM` may change `schema_version`;
- Page Lifecycle freezing of background tabs, and that Chromium does not
  freeze a page that holds a Web Lock;
- Replicache's client view record retraction, and Zero's CVR storage;
- COOP and COEP effects on cross-origin embeds and popups;
- iOS Data Protection and Android credential-encrypted storage.
