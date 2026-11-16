# Adversarial review: Sync partial bootstrap and local-only mode, third draft (2026-11-15)

Subject: the third draft of the sync partial bootstrap and local-only mode note (lane GoalsB-5-17r3, patch base `84df7e8189`). Reviewed against dev `2ded9a1ded`, which carries the registry generation fix (`2ded9a1ded`, GoalsB-5-21) and phase 1 of the live hub watch (`fad05cd5f6`, GoalsB-5-20r2a). Phase 2 of the hub watch is not on dev.

Method, as in the first two reviews: every claim in the note's section 0 table was attacked again, together with the claims the third draft adds. For each claim that failed in the second review (S5, S8, S14, N1, N3, N5) and each gap (G5, G10, G14, G22, G36 to G64), this review decides whether the draft engineers the problem away, bounds it honestly, or leaves it open. Code was read from dev with `git show`. External facts were checked against the SQLite WASM persistence page where it decided a question; other external facts are marked "from knowledge". Nothing was built and no tests were run.

## 1. Measured facts

| # | Fact | Where |
|---|---|---|
| F1 | The generation rule is on dev. `LiveRecord` has `generation`; `live_invalidate` increments it with `dirty`; `state` captures the generation from `live_current` before `binding.run()` and passes it to `live_store`, which clears `dirty` only if the generation is unchanged. A push (`started` None) stays current as stored. | `git show 2ded9a1ded -- Core/App/App.jet` |
| F2 | The proof case is on dev: `tests/cases/web_app/live_rerun_overtaken_by_an_invalidation_stays_stale` (`.case`, fixture, expected stdout). The commit message reports it failing without the fix and passing with it; this review did not run it. | `git ls-tree -r 2ded9a1ded tests/cases` |
| F3 | The hub on dev still publishes `{"live":"invalidate","source":…,"key":…,"invalidations":…}` on topic `live:<source>` to every joined server connection. The event carries the encoded key, not a binding id. | `Core/App/App.jet` at `2ded9a1ded`, `live_invalidate`, L608-633 |
| F4 | Phase 1 of #6868 c9 adds `jet_web_live_watch(conn, topic)` and its rows. Nothing calls it, and every joined connection still receives every event (commit message and `WsRoutes.jet` L536-543). `app.watch` does not exist on dev. | `git show --stat fad05cd5f6`; `WsRoutes.jet` at `2ded9a1ded` |
| F5 | `live_invalidate` publishes nothing when no binding of the source is registered in this process (`if hit == 0 -> return`). The hub is per process. | `Core/App/App.jet` L620-622 |
| F6 | `live_store` still returns without storing a value larger than `LIVE_MAX_PAYLOAD`, so an oversized result still re-runs on every read until 4.2 is built. | `Core/App/App.jet` L582-585 |
| F7 | Tower: #6868 c3 (caps and coalescing), c5 (SQL-narrowed read sets), c6, c8 and c9 are all `open`; c8 is open although its code is on dev. #6867 c2 to c8 open; #6869 c2 to c4 open; #6554, #6558, #6827 in planning. | `jq` over `Tools/tower/.tower/tower.json` at `89cd083b38` |
| F8 | #6868 c5 is "Predicates the compiler can turn into SQL narrow the read set; any other predicate falls back to table-level invalidation". The note's precondition table labels table-level invalidation as "#6868 c5, Built". Table-level invalidation is built; c5 is open. | Tower; patch L144 |
| F9 | SQLite WASM: "The `opfs-wl` VFS works identically to the `opfs` vfs with the following exceptions": Web Locks for locking, a dependency on `Atomics.waitAsync()` ("widespread browser circulation since 2025 ... If that function is not found, the VFS will not install"), and its OO1 name. "The two VFSes share more than 90% of their code, with only the locking-related parts being VFS-dependent." It was "Added in 3.53.0". The `opfs` VFS needs `SharedArrayBuffer`, and so the COOP and COEP headers, "to coordinate communication between the synchronous and asynchronous parts" of its proxy. That requirement is not among the listed exceptions. | [SQLITE-WASM], sections "OPFS with Web Locks" and "COOP and COEP HTTP Headers" |
| F10 | SQLite WASM: with WAL, "the requirement for exclusive locking eliminates all concurrency support from the `opfs` and `opfs-wl` VFSes". | [SQLITE-WASM], "WAL Mode with OPFS" |
| F11 | SQLite WASM: `opfs-sahpool` "does not require COOP/COEP" and does not support multiple simultaneous connections to one database. | [SQLITE-WASM] |

## 2. Status of the failed claims and the gaps

### 2.1 The six claims that failed

- **S5 (catch-up).** The registry race is engineered away and on dev (F1, F2). Writes outside Jet are covered on SQLite by triggers and on Postgres by decoded WAL. Two new defects in the Postgres path (N1 below) keep S5 failing narrowly on Postgres. On SQLite it now holds within stated limits.
- **S8 (partitions).** Engineered away: snapshot rows carry their page's watermark, replay merges CRDT state and keeps unacknowledged edits, and a row that leaves a policy is sent as a delete. A new conflict with section 9.8 (below) bounds it. It now holds within stated limits.
- **S14 (failure table).** Engineered away for what the second review named: a corrupt `.Device` store is kept and exportable, the queue location is stated per platform, and every named failure has a row. The new failure modes this review finds are new gaps, not a failure of the claim as stated.
- **N1 (Postgres watermark).** Not engineered away. Bounding W by C is the right idea, but W cannot be computed from the structures the note defines, and C as defined does not reach the position catch-up waits for on a database with no recent synced writes. Fails narrowly.
- **N3 (epoch in the key).** Engineered away: the epoch is an entry field from the server. Holds within stated limits.
- **N5 (seed IDs).** Partly engineered away (UUIDv7, matched by name). Two devices of one account that join at the same time still create two rows per seed, which is the case the note asks this review to attack. Fails.

### 2.2 The carried and second-review gaps

| Gap | Status | Basis |
|---|---|---|
| G5 | Honestly bounded, open | The write log is deferred to #6554 and two measurements are required first. Neither measurement exists. |
| G10 | Open, honestly stated | No owner card; run-time check only. |
| G14 | Honestly bounded, open | Budgets are defaults pending a named measurement. |
| G22 | Open | Dev still sends the encoded key to every joined connection (F3, F4). Phase 2 is not landed. |
| G36 | Closed on dev | F1, F2. The note still says "patch submitted" and "fails on dev" (L142, L304-307): G87. |
| G37 | Partly closed | Replica bound and catch-up wait are designed. W's inputs and C's definition are not: G65, G66, G67. |
| G38 | Closed within limits | Triggers and decoded WAL. Residual holes: G68. |
| G39 | Closed for catch-up; reopened for growth | The map answers every catch-up the log would under table-level invalidation. The nonce ledger of 3.7 grows with writes: G69. |
| G40 | Closed | 4 KiB cap; device-only trust. |
| G41 | Honestly bounded | A `version` field is promised for #6868 c9's event; until then every event counts as newer. Dev's event (F3) has no version. |
| G42 | Closed, with a cost hole | The 15-minute re-check ignores metered networks: G72. |
| G43 | Closed | Per-item and per-batch caps; streamed. |
| G44 | Closed within limits | Row diffs need per-subscriber server state that is not costed: G71. |
| G45 | Honestly bounded | Spelling left to ballot B7. |
| G46 | Closed within limits | Residual: Core source changes and non-code inputs: G73. |
| G47 | Closed | |
| G48 | Closed | |
| G49 | Closed, with a conflict | The checkpoint hold conflicts with 9.8 and long-pending uploads: G74. |
| G50 | Closed within limits | Detection cost per write and policy changes at deploy are not stated: G75. The checksum covers versions, not content: G76. |
| G51 | Not closed | `opfs-wl` needs what `opfs` needs (F9); `steal` does not free a frozen leader's file handles: G77, G78, G79. |
| G52 | Not closed | Concurrent joins: G80, G81. |
| G53 | Closed within limits | "Owning account" undefined for tables without an owner; refused rows versus file retirement: G82. |
| G54 | Closed within limits | Mixed methods outside one transaction are unclassified: G83. |
| G55, G56, G57, G58, G61, G62, G63 | Closed | |
| G59 | Reopened narrowly | Table-level invalidation is cited as #6868 c5 (F8): G84. |
| G60 | Closed | |
| G64 | Open, honestly stated | No owner card. |

## 3. Per-claim review

### S1 First paint from the device store

Prior art: Firestore's persistent cache and Linear's local database paint from disk before the network (from knowledge, [FS-OFF]).

Attack: unchanged from the second review. Launch paints cache rows before any request (6.2), which is also the revocation window, stated. Nothing new breaks it.

Verdict: **holds only within stated limits.**

### S2 Bounded reads: one live window, row-change delivery

Prior art: Zero keeps a client view record per client and maintains limited, sorted queries incrementally, sending row changes [ZERO-Q]. Electric sends shape logs as row operations [ELEC-HTTP].

Attack:

- To send "inserts, updates, deletes and moves" by comparing with "the one the subscriber holds" (4.1), the server must keep each subscriber's last window. A 1,000-row window is up to the 4 MiB cap; 200 subscribers on one endpoint could be up to 800 MiB per server for one endpoint. Zero keeps that state in its CVR store, durable across restarts (from knowledge). The note says neither where this state lives, whether it survives a server restart or a reconnect to another server, nor what a reconnecting subscriber gets when it is gone (presumably the whole window). The measurement it requires counts re-runs and bytes sent, not this memory.
- The diff must match rows "by row ID". A window over an endpoint whose result rows are not entities with IDs (aggregates, joins without a stable key) has no ID to diff on. The note does not say which rule applies there.

Verdict: **holds only within stated limits.** Gaps G71, G85.

### S3 Warm set

Prior art: PowerSync downloads all of a user's buckets with no warm list [PS-STREAMS]; Zero's preload queries with TTLs [ZERO-Q].

Attack:

- Warming waits for an unmetered connection (5.2), but the 15-minute online re-check (5.1) has no such condition. Under table-level invalidation, the note's own 3.5 says nearly every board is re-read after any task write. A phone on cellular in a busy workspace would re-download its warm set, up to the 64 MiB budget, every 15 minutes.
- Coverage remains run-time only; that is stated.

Verdict: **holds only within stated limits.** Gap G72; G10 carried.

### S4 Budget, eviction, `QueueFull`

Prior art: Firestore's cache size limit with garbage collection [FS-OFF].

Attack: pinned entries may exceed the budget, reported by devtools; `QueueFull` reserves before prediction (8.4). No new hole. G14 still open.

Verdict: **holds only within stated limits.**

### S5 Catch-up by version

Prior art: Electric hands out offsets and answers `must-refetch` when a shape's history is gone [ELEC-HTTP]. PowerSync's write checkpoints wait until replication has passed a recorded LSN, and its service writes logical messages so that the position advances on a quiet database (from knowledge). Debezium documents the same quiet-database problem for logical slots and adds a heartbeat query for it (from knowledge).

Attack, by store:

- **Registry race.** Closed on dev (F1, F2). The rule has a liveness cost the note does not state: a binding whose run takes longer than the gap between invalidations of its table is never stored as current, so every `state` read re-runs the source. Under table-level invalidation that is any slow query over a busy table. The bound would be #6868 c3's caps and coalescing, which is open (F7). Gap G86.
- **SQLite.** Triggers inside the writing transaction are commit-ordered and see every writer that runs SQL. Holes: a migration that rebuilds a table (the standard SQLite pattern of create, copy, drop, rename) drops its triggers, and a runtime migration by an outside tool is only caught at the next startup, not while the server runs. Incremental BLOB I/O (`sqlite3_blob_write`) does not fire triggers (from knowledge). Per-row triggers on a 10,000-row update add two writes per row, unmeasured. A cheap detector for an outside process exists and is not used: `PRAGMA data_version` changes when another connection commits (from knowledge). Gap G68.
- **Postgres.** See N1. The two defects there are the reason S5 still fails on Postgres.
- **Nonce ledger.** See N7.

Verdict: **fails, narrowly, on Postgres (N1); holds only within stated limits on SQLite.**

### S6 Revocation

Prior art: Firestore and PowerSync keep cached data readable offline until the next sync [FS-OFF], [PS-STREAMS].

Attack: B5 no longer defaults to no age; launch-time deletion is stated. Two holes: a partition's view is re-evaluated on membership writes (7.5) but nothing says a deploy that changes a row policy re-evaluates every subscriber's partitions (6.1's epoch covers endpoints, not partitions). And a policy whose input is configuration rather than a table or a token claim has no path. Gap G75.

Verdict: **holds only within stated limits.**

### S7 Wire versions

Attack: rests on #6558 (planning), stated. No change.

Verdict: **holds only within stated limits.**

### S8 Partitions

Prior art: PowerSync's checkpoints with additive bucket checksums, and its rule of not applying a checkpoint while local uploads are pending [PS-CONS]. Electric's move-out (from knowledge).

Attack:

- **Engineered away:** row versions from page watermarks; replay applies only newer changes; CRDT merge keeps unacknowledged local edits; policy leavers get deletes.
- **The checkpoint hold conflicts with sync off.** 7.2 holds a partition's checkpoint while its uploads are pending. 9.8 holds the queue while sync is off, possibly for weeks, and promises that "pushes still arrive". Under 7.2 a held write freezes every partition it touches, so pushes for collections stop applying. The same happens for any long-pending upload, for example a write retried with backoff for days. PowerSync holds because its local state is not mergeable; collections here are CRDTs, whose merge commutes, so the hold is not needed for correctness of the merge. The note does not say why it adopts the hold, nor bound how far behind a held partition falls.
- **The change buffer.** 7.4 keeps 7 days of row changes for collection tables. That is a write log with row values, which 3.4 deferred for the version log on cost grounds. It is not measured.
- **Retraction detection.** "When a write changes whether a subscriber may read a row" requires evaluating the policy before and after the write for each subscriber of the partition. The per-write cost is not stated or measured.
- **Checksums.** The additive checksum covers (row ID, row version), so it detects a missing or stale row, not a row whose content was corrupted at the same version. The device must also compute it over server versions only, excluding merged local edits; the note does not say so.

Verdict: **holds only within stated limits.** Gaps G74, G75, G76, G88.

### S9 Local-only mount

Prior art: any embedded-database desktop app (from knowledge); Replicache and PowerSync have no in-process server.

Attack: the note's own case, "a method that reads a `.Device` draft and then writes a `.Server` row", is unclassified. The rules cover methods that touch only one authority, and methods that touch both in one transaction. A method that reads a draft, then calls a write on a server table in a separate transaction, is neither. It also does not say how the compiler finds which stores a method touches when a store handle is passed as a value, or reached through a called function. Gap G83.

Verdict: **holds only within stated limits.**

### S10 Device data stays on device

Attack: the platform switches are right as far as the second review verified (Android `dataExtractionRules`, `fullBackupContent`, iOS exclusion). The B8 ballot states both costs.

Verdict: **holds only within stated limits.**

### S11 Sync on later

Attack:

- "Confirmed by row ID and owning account" assumes every uploaded table has an owner column. Join tables and child rows owned through a parent have none. The note does not say how ownership is decided for them.
- The end state retires the `.Device` file "after the server has confirmed every row", and refused rows stay local, "Not synced". With one refused row, the file is never retired, and the person has a half-synced store with no stated end. If instead the file is deleted after catch-up, refused rows are lost.
- A row from the earlier upload that another device deleted within the 24-hour gap is re-created by the resumed upload, because "exists" is the only check. Gap G82.

Verdict: **holds only within stated limits.**

### S12 Sync off

Attack: see S8. The promise that pushes still arrive does not hold for collections under the checkpoint hold.

Verdict: **holds only within stated limits** (was holds). Gap G74.

### S13 Sign-out, shared devices

Attack: signed-out drafts now belong to the device account; no new hole.

Verdict: **holds only within stated limits.**

### S14 Failure table

Attack: the rows the second review named are present. Missing rows for failures found here: catch-up timing out on a quiet database (G66), a mass "re-read" when the reader is down (G67), a frozen leader that holds its file handles (G78), a binding that never becomes current under sustained writes (G86), two devices joining at once (G80). These are new gaps, not a failure of what the claim stated.

Verdict: **holds only within stated limits.**

### N1 The Postgres watermark and the catch-up wait (3.1, 3.5)

Prior art: Electric records `xmin` and `xip_list` and resolves visibility per transaction [ELEC-HTTP]; PowerSync and Debezium (above).

Attack:

1. **W has no input.** W is "the largest commit LSN at most C such that every decoded write at or below W is visible in the snapshot". Deciding that needs, at read time, the transaction id and commit LSN of every decoded commit in the snapshot's open range (between `xmin` and C, and those in `xip`). The note's only shared structure is the per-table version map: one row per table with its highest version. It holds no transaction ids. Nothing in the note records a (xid, commit LSN) list, says how long it is kept, or how every server reads it. Without it, W cannot be computed as defined, and any approximation from the map alone (for example, taking C) is exactly the unsound reading the second review rejected.
2. **C stalls on a quiet database.** C is "the commit LSN up to which every commit has been decoded and recorded". Catch-up notes `pg_current_wal_lsn()` and waits for C to pass it. The publication covers only synced tables, so a database whose recent WAL holds no synced commit (any write to a non-synced table, a vacuum, a checkpoint) never moves C past that position. Every such catch-up times out after 10 seconds and answers "re-read" for every key. This is the quiet-database problem PowerSync and Debezium solve by writing a marker into the WAL (from knowledge). The fix is to define C as the reader's decoded WAL position, advanced by the stream's keepalives, or to have catch-up emit a logical message (`pg_logical_emit_message`, from knowledge) and wait for it.
3. **The timeout fallback is a herd.** When the reader is down or behind, every reconnecting device is told to re-read every key. That load arrives during an incident, from every device at once. The safe answer is "unknown, keep showing with age and retry", which the device rule already permits.
4. **Holds:** replicas. A hot standby makes commits visible in WAL replay order, so bounding W by the replay position is sound (from knowledge). Reader restart is sound if C is persisted only after the map rows it covers, which "decoded and recorded" implies.

Verdict: **fails as written, narrowly.** Gaps G65, G66, G67.

### N2 Device-carried read sets

Attack: 4 KiB cap, device-only trust. No change.

Verdict: **holds only within stated limits.**

### N3 Endpoint epoch

Prior art: content hashing of build inputs (Bazel, Nix, from knowledge).

Attack: hashing the typed source call graph includes Core functions the endpoint calls. A toolchain upgrade that changes any Core source on that path changes the epoch, so most Core upgrades will still re-read most keys; only pure code-generation changes are excluded. That is probably the right trade, since a Core fix can change results, but the note claims more ("a compiler or Core upgrade that changes only code generation") than users will see. Configuration and environment the endpoint reads are not in the hash, so a configuration change alters results without an epoch change. Whether formatting and comment changes alter "typed source" is not stated.

Verdict: **holds only within stated limits.** Gap G73.

### N4 The web store: `opfs-wl`, then a leader worker

Prior art: SQLite WASM's own guidance [SQLITE-WASM]; PowerSync's web SDK and wa-sqlite use shared-worker or leader patterns (from knowledge).

Attack:

1. **`opfs-wl` needs COOP and COEP too.** The note rejects `opfs` as the default because it needs the COOP and COEP headers, and prefers `opfs-wl`. SQLite's page says `opfs-wl` works identically to `opfs` except for locking and `Atomics.waitAsync()`, sharing more than 90% of its code; the `SharedArrayBuffer` requirement for the synchronous and asynchronous proxy is not among the exceptions (F9). So `opfs-wl` almost certainly needs the same headers [INFERENCE from the page's wording; not tested]. The note's preferred path therefore runs only where the deployment sends COOP and COEP, which breaks cross-origin embeds and popups that do not opt in (from knowledge). For most deployments the fallback is the default. It also needs SQLite 3.53.0 or later and `Atomics.waitAsync()`.
2. **`steal` does not free the file.** The fallback leader opens the store with `opfs-sahpool`, which holds sync access handles open, and a `"readwrite"` sync access handle locks its file exclusively [MDN-CSAH]. A frozen leader runs no code, so stealing its Web Lock does not close its handles; the new leader's `createSyncAccessHandle` fails until the frozen page is discarded or resumes. The heartbeat-and-steal design does not replace a frozen leader as claimed. The old leader, when it resumes, must also see that its lock was stolen and stop before writing again; the note does not say so.
3. **Journal mode.** WAL removes all concurrency from `opfs` and `opfs-wl` (F10). The note does not name the journal mode.
4. **Idempotent requests.** "A new leader answers a request it already applied with the stored answer" requires the answer to be committed in the same transaction as the write. Not stated.
5. **Queue-then-cache.** Re-deriving predictions from the queue at launch double-applies a prediction that did reach the cache before the crash (an increment applied twice), unless cache rows record which request produced them. Not stated.
6. A tab frozen in the middle of a transaction under `opfs-wl` still holds its Web Lock, so the claim "no tab can freeze while holding the store" depends on browser freezing policy, which the note does not cite.

Verdict: **fails.** Gaps G77, G78, G79.

### N5 Seed rows

Prior art: Replicache and PowerSync leave seed data to the application; a server-side unique constraint on a natural key is the usual tool (from knowledge).

Attack:

- **Two devices at once.** Device A and device B of one account join together. Both ask the server for the account's seeds by name, both find none, and both upload their own UUIDv7 rows. The account now has two rows per seed, the duplicate the claim exists to prevent. Preventing it needs a server constraint on (account, seed name) and a create that answers with the existing row's ID, so the loser renumbers. That means a seed-name column on every seeded server table, which the note does not mention.
- **Renumbering scope.** "The local row's references are renumbered in one local transaction" covers the `.Device` store. Queued writes, cached results and other `.Device` stores that name the old ID are not mentioned.

Verdict: **fails.** Gaps G80, G81.

### N6 The registry generation rule (new claim, now on dev)

Prior art: optimistic concurrency with version counters; React Query's and SWR's stale-while-revalidate keep a fetch's result but mark it stale when an invalidation arrives during the fetch (from knowledge).

Attack: verified in code (F1). The rule is correct for the race it names: the generation is captured before the run, invalidation increments it, and the store clears `dirty` only on an unchanged generation. The Err path uses the same rule. Remaining: liveness under sustained writes (G86), and oversized values still not stored (F6), already covered by 4.2.

Verdict: **holds only within stated limits.**

### N7 The per-table version map replaces the write log (new claim, 3.4 and 3.7)

Prior art: PowerSync's per-bucket op ids [PS-CONS]; Electric's per-shape offsets [ELEC-HTTP].

Attack: under table-level invalidation, "any write to table t after version v" is exactly "t's map version is greater than v", so the map answers every catch-up the log would. That part holds. But 3.4 says the map "has no retention and does not grow with writes", while 3.7 makes the server keep "(version, nonce) pairs of the last 30 days" and answer the nonce of the device's highest version. A device's highest version can be any commit's version, so the server must keep a pair for every commit for 30 days: one row per write transaction, a write log without its write sets. On SQLite every synced write transaction adds one. The nonce check is sound for restore detection, but its storage contradicts 3.4, and no size is given. A coarser rule would do: a nonce per checkpoint interval, with the device holding the nonce of the latest checkpoint at or below its version.

Verdict: **fails as written, narrowly** (the equivalence holds; the growth claim does not). Gap G69.

### N8 Row-change delivery, CRDT replay, mixed-service rules (new claims)

Covered under S2, S8 and S9: each holds only within stated limits, with gaps G71, G74 to G76, G83.

## 4. Answers to the note's section 14

- **Postgres rule across reader restart and replicas.** Replica bound and restart are sound. The rule as written is not computable (no xid record) and stalls on quiet databases (N1).
- **SQLite triggers and table rebuilds.** A rebuild drops the triggers; caught only at the next startup unless `db.migrate` advances the epoch itself (G68).
- **Version map versus log; nonce window.** Equivalent under table-level invalidation; the 30-day nonce window makes a per-commit table (N7).
- **Row-change delivery against Zero.** Cannot be judged until the measurement exists; the per-subscriber state it needs is uncosted (G71).
- **CRDT replay with checkpoint hold.** Yes, a device with a long-pending upload, or with sync off, falls arbitrarily far behind (G74).
- **`opfs-wl` and the leader fallback.** `opfs-wl` very likely needs COOP and COEP; `steal` does not free a frozen leader's handles; the crash order double-applies predictions (N4).
- **Seed matching with two devices at once.** Duplicates (N5).
- **Mixed services.** The named example is unclassified (G83).

## 5. Verdicts

| Claim | Second review | Third review |
|---|---|---|
| S1 First paint | holds only within stated limits | holds only within stated limits |
| S2 Bounded reads | holds only within stated limits | holds only within stated limits |
| S3 Warm set | holds only within stated limits | holds only within stated limits |
| S4 Budget, eviction, `QueueFull` | holds only within stated limits | holds only within stated limits |
| S5 Catch-up by version | fails, narrowly | fails, narrowly, on Postgres only (N1); within limits on SQLite |
| S6 Revocation | holds only within stated limits | holds only within stated limits |
| S7 Wire versions | holds only within stated limits | holds only within stated limits |
| S8 Partitions | fails | holds only within stated limits |
| S9 Local-only mount | holds only within stated limits | holds only within stated limits |
| S10 Device data stays on device | holds only within stated limits | holds only within stated limits |
| S11 Sync on later | holds only within stated limits | holds only within stated limits |
| S12 Sync off | holds | holds only within stated limits (checkpoint hold) |
| S13 Sign-out | holds only within stated limits | holds only within stated limits |
| S14 Failure table | fails | holds only within stated limits |
| N1 Postgres watermark | fails as written | fails as written, narrowly (no xid record; C stalls) |
| N2 Device-carried read sets | holds only within stated limits | holds only within stated limits |
| N3 Endpoint epoch | fails | holds only within stated limits |
| N4 Web store | holds only within stated limits | fails (`opfs-wl` needs COOP/COEP; `steal` cannot free a frozen leader) |
| N5 Seed IDs | fails | fails (concurrent joins duplicate seeds) |
| N6 Generation rule (new) | | holds only within stated limits |
| N7 Version map replaces the log (new) | | fails as written, narrowly (nonce ledger grows per commit) |
| N8 Row diffs, CRDT replay, mixed services (new) | | holds only within stated limits |

Of the six claims that failed in the second review, three are engineered away and now hold within stated limits (S8, S14, N3), and three still fail (S5 and N1 narrowly, through the Postgres watermark; N5). N4 fails on facts the second review did not check, and N7 fails narrowly. Under AGENTS.md the note is not yet a reviewed design, and #3709 c1 stays open.

## 6. Remaining gaps

Carried and still open: G5 (two measurements), G10 (coverage owner card), G14 (budget measurement), G22 (dev still sends keys to every joined connection; phase 2 not landed), G41 (version field in the hub event, honestly bounded), G45 (ballot B7), G64 (test-host owner card).

New:

- **G65.** The note defines the record W is computed from: per decoded commit, its top-level transaction id and commit LSN, written by the reader with the version map, kept at least until no snapshot open on any server can have that transaction in progress, and read by every server that serves versioned reads. A case computes W for a snapshot that has an in-progress transaction which later commits below C.
- **G66.** C is the reader's decoded WAL position, advanced by stream keepalives even when no synced table was written, or catch-up writes a logical message and waits for the reader to decode it. A case runs catch-up on a database whose only recent writes are to non-synced tables and shows it answers without the timeout.
- **G67.** When the reader is down or behind past the timeout, catch-up answers "unknown" and the device keeps showing entries with their age and retries with backoff, instead of every device re-reading every key. The failure table gains the row.
- **G68.** `db.migrate` advances the store epoch itself whenever it rebuilds a synced table, and the server checks for missing triggers whenever `PRAGMA data_version` shows a commit from another connection, not only at startup. The note states that incremental BLOB writes bypass triggers and that Jet does not use them on synced tables. The per-row trigger cost is measured on a 10,000-row update.
- **G69.** The nonce ledger of 3.7 is bounded: one nonce per checkpoint interval (stated), the device keeps the nonce of the latest checkpoint at or below its highest version, and 3.4's "does not grow with writes" is corrected or made true. Its size for Crewboard's seeded write load is recorded.
- **G70.** The note states that the live hub is per process and that `live_invalidate` publishes nothing in a process with no binding of the source (F5), so a write on one server reaches subscribers on another only through the one-second version-map poll of 3.4. That latency is stated as the cross-server bound, and a two-server case shows a write on server A reaching a subscriber on server B within it.
- **G71.** The note states where each subscriber's last window is kept for row diffs, its memory per subscriber, whether it survives a server restart or a reconnect to another server, and what a subscriber receives when it is gone. The 4.1 measurement records that memory for 200 subscribers.
- **G72.** The online warm re-check follows the warming rule: on a metered connection, or with no network type, it marks entries unconfirmed and re-reads only when a screen opens the key.
- **G73.** The note states that Core source changes on an endpoint's call path change its epoch, whether formatting and comments do, and how configuration an endpoint reads enters the epoch (or that it does not, documented).
- **G74.** Collections apply merged server changes while uploads are pending, since CRDT merge commutes; or the note states why the hold is kept and bounds it (for example, the hold lifts after a stated age and the partition re-snapshots). 9.8's promise that pushes arrive is made true for collections. A case holds a write with sync off for a simulated week and shows the partition current.
- **G75.** The per-write cost of detecting policy retraction is stated and measured (policy evaluations per write per subscriber). A deploy that changes a row policy re-evaluates every subscriber's partitions, with a case.
- **G76.** The partition checksum covers a content hash of each row, not only (row ID, row version), and the device computes it over server versions only, excluding merged unacknowledged edits.
- **G77.** 8.1 states that `opfs-wl` needs COOP and COEP like `opfs` (or cites a test showing it does not), SQLite 3.53.0 or later, and `Atomics.waitAsync()`. It names the default for a deployment without COOP and COEP, and names the journal mode (not WAL).
- **G78.** The fallback leader handles a frozen leader whose sync access handles stay open: either it uses a store whose handles a stolen lock does release, or it states that a frozen leader blocks the store until the page is discarded, with the person told. A resumed leader that lost its lock stops before writing. The three-tab case covers both.
- **G79.** The leader commits each request's answer in the same transaction as its write. Cache rows record the request ID of the prediction that produced them, and launch re-derives only predictions the cache lacks. A case crashes between the cache commit and the queue removal and shows no prediction applied twice.
- **G80.** Seed matching is made safe for concurrent joins: a server constraint on (account, seed name), and a create that returns the existing row's ID so the losing device renumbers. The seed-name column on seeded tables is stated. The two-device case joins both devices at once.
- **G81.** Renumbering a seed covers queued writes, cached results and every `.Device` store that names the old ID, in one stated order.
- **G82.** "Owning account" is defined for tables without an owner column. A refused row has a stated end state that lets the `.Device` file retire (for example, moved to a "Not synced" store the person can export). A resumed upload does not re-create a row that another device deleted, with a case.
- **G83.** 9.4 classifies a method that touches both authorities outside one transaction, using the note's own example, and states how the compiler finds the stores a method touches through calls and through store handles passed as values. A diagnostic snapshot for each case.
- **G84.** The precondition table cites table-level invalidation by its code (`core.db.live:<table>` in `Core/Db/Db.jet`), not as "#6868 c5, Built"; c5 is SQL-narrowed read sets and is open.
- **G85.** 4.1 states the diff rule for a window whose rows have no stable ID (aggregates, keyless joins): the whole window is resent, or such endpoints are not windowable.
- **G86.** The note states that under the generation rule a binding whose run is longer than the gap between invalidations re-runs on every read, and bounds it with #6868 c3's coalescing and caps (open) or a rule of its own. A load case records re-runs per second for a slow query over a table written ten times per second.
- **G87.** The precondition table and 3.2 are updated: the generation fix is on dev in `2ded9a1ded` and its case passes there; Tower #6868 c8 is still open.
- **G88.** The 7-day change buffer for collection tables is measured (bytes per day for Crewboard's seeded load) like the write log of G5.

## Sources

| Key | Source |
|---|---|
| SQLITE-WASM | [SQLite WASM: Persistent storage options](https://sqlite.org/wasm/doc/trunk/persistence.md) (opened for this review: "OPFS with Web Locks", "COOP and COEP HTTP Headers", "WAL Mode with OPFS", `opfs-sahpool`) |
| MDN-CSAH | [MDN: FileSystemFileHandle.createSyncAccessHandle()](https://developer.mozilla.org/en-US/docs/Web/API/FileSystemFileHandle/createSyncAccessHandle) (verified in the second review) |
| PS-CONS | [PowerSync: Consistency](https://docs.powersync.com/architecture/consistency) |
| PS-STREAMS | [PowerSync: Sync Streams](https://docs.powersync.com/usage/sync-streams) |
| ZERO-Q | [Zero: Queries](https://zero.rocicorp.dev/docs/queries) |
| ELEC-HTTP | [Electric: HTTP API](https://electric-sql.com/docs/api/http) |
| FS-OFF | [Firestore: Access data offline](https://firebase.google.com/docs/firestore/manage-data/enable-offline) |
| Code | `Core/App/App.jet`, `Compiler/JetRuntime/Source/WsRoutes.jet`, `tests/cases/web_app/` at dev `2ded9a1ded`; commits `2ded9a1ded`, `fad05cd5f6`; `Tools/tower/.tower/tower.json` at `89cd083b38` |

From knowledge, not opened for this review: PowerSync's write checkpoints and WAL keepalive messages; Debezium's heartbeat for low-traffic databases; `pg_logical_emit_message`; hot-standby visibility in replay order; `PRAGMA data_version`; incremental BLOB I/O bypassing triggers; Zero's CVR storage; COOP and COEP effects on cross-origin embeds; browser freezing policy for pages holding Web Locks.
