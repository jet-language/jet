# Adversarial review: Sync partial bootstrap and local-only mode, second draft (2026-11-15)

This is the second adversarial review of `Docs/proposals/2026-11-15-sync-partial-bootstrap-and-local-only.md`, the answer to Tower card #3709 c1. The second draft is still a patch (lane GoalsB-5-17r2). It replaces the first draft, and its section 12 maps gaps G2 to G35 of the first review (`2026-11-15-sync-partial-bootstrap-review.md`) to sections of the new text. The review follows AGENTS.md, "Adversarial design review":

- every claim is attacked again;
- each claim that failed the first time (S2, S5, S8, S10, S13, S14) must now be engineered away, or bounded honestly with its cost and its best mitigation;
- a gap's answer counts only if it is a design, not a promise;
- new claims are attacked like old ones.

The repository was read at dev `9ae575e506`, which contains `5fa1fd1c85` ("core.db: a write inside a transaction marks live bindings stale at the commit"). The mirror's checkout is the Tower branch, so dev files were read through `git show 9ae575e506:<path>`. Nothing was built and no test suite was run. Four external documents were opened for this round: MDN `createSyncAccessHandle`, the SQLite WASM persistence guide, Android Auto Backup, and (in the first round) the PowerSync, Electric, Zero, Firestore and CouchDB pages. Every other fact about another system is marked "from knowledge" where it is used.

## What changed, in one paragraph

The redesign takes most of the first review's criteria and makes them design text:

- a persistent store per account;
- commit-ordered versions;
- a monotonic store rule;
- a write log kept in the database;
- checksums, a store identity and a commit nonce;
- one live window per list instead of cached pages;
- one-shot warm reads with batched loops;
- endpoint epochs;
- per-page snapshot versions with idempotent replay;
- per-row policy checks inside partitions;
- a leader tab on the web;
- one store-authority concept (ballot B3);
- backup exclusion;
- device migrations;
- per-account files;
- an upload ordered by dependencies, with request keys.

It also narrows S1 honestly and states the offline limits of the blob store. That is real progress. What remains falls into four groups:

- correctness holes in the new version machinery;
- a statement that a race is closed when the code shows it is still open;
- two new mechanisms that contradict other cards (seed IDs, partition replay);
- a set of new failure modes created by the new components.

## Measured facts

| # | Read | Observed |
|---|---|---|
| F1 | `git merge-base --is-ancestor 5fa1fd1c85 9ae575e506`; `Core/Db/Db.jet:815-863` at dev | The fix is on dev. Inside an open transaction, `__scope_execute` records the table in `written`. `__db_commit` and `__db_rollback` mark every written table after the engine's commit or rollback. Outside a transaction a write marks at once. The proof case `tests/cases/db_policy/db_scope_live_marks_writes_stale_only_after_commit` exists on dev. Tower still shows #6868 c8 as open, and the note still calls it "patch submitted". |
| F2 | `Core/App/App.jet:404-416` (`state`) and `566-584` (`live_store`) at dev | `state` runs the binding (`binding.run()`), then calls `live_store`, which sets `dirty = false` without any condition. Nothing records whether an invalidation arrived while the run was in progress. A re-run that reads the old row just before a commit, and stores after the commit's invalidation, therefore leaves the old value marked current, and nothing marks it stale again. The fix in F1 shrinks the window from "statement to commit" to "one re-run's duration". It does not close it. The note's 3.2 says the race "is closed by that patch". That is not what the code shows. This was found by reading the code, not by running it. |
| F3 | `Compiler/JetRuntime/Source/WsRoutes.jet:470-602` at dev; search of `Core/App/App.jet` for `watch` | The live hub is unchanged. Every joined connection gets every topic's event, and a joining connection is replayed every topic's latest. No `app.watch` exists. #6868 c9 (the GoalsB-5-20 patch) has not landed, and its proof case `web_app/live_hub_events_reach_only_admitted_subscribers` does not exist on dev. |
| F4 | Tower | #6867, #6868 and #6869 are in phase `ready`, and every one of their criteria is open. #6554 (D-INVALIDATE1), #6558 (D-WIRESKEW1), #6827 and #6811 are in `planning`. **#6811 is "Web metadata and SEO: typed page head, Open Graph, sitemap.xml and robots.txt from the screen graph"**: no criterion of it records endpoint reads per screen, yet the note cites it for the warm-coverage check. #3709 c21, c23 and c26 are cited by the note as live criteria, but their substance moved to #6868 c5, #6867 c6 and #6867 c7. |
| F5 | Tower #6867 c6 | "An ID type's decoder accepts only UUIDv7 text (422 otherwise)." The note's 9.7 gives seeded rows UUIDv5 IDs. |
| F6 | Tower #6867 c2 | The server keeps (account, request key) → result "for 24 hours". The note's 9.7 relies on request keys to resume an interrupted upload, with no time limit. |
| F7 | MDN `createSyncAccessHandle` [MDN-CSAH] | A sync access handle is "only usable inside dedicated Web Workers". `"readwrite"` mode takes an exclusive lock. `"read-only"` mode allows several readers. `"readwrite-unsafe"` mode allows several writers. |
| F8 | SQLite WASM persistence guide [SQLITE-WASM] | SQLite WASM ships three OPFS VFSes. `opfs-sahpool` is fast and has no multi-tab concurrency. `opfs` needs the COOP and COEP headers. `opfs-wl` uses Web Locks. Clients that need multi-tab concurrency should use `opfs` or `opfs-wl`. "Reading locks OPFS files", and testing in 2026-03 sustained 8 to 10 concurrent workers when `SQLITE_BUSY` was handled. |
| F9 | Android Auto Backup [ANDROID-BACKUP] | Files under `getDatabasePath` and `getFilesDir` are backed up by default. Data is restored at install, before first launch. Cloud backup and device-to-device (D2D) transfer are configured separately: `dataExtractionRules` for API 31 and later, `fullBackupContent` for API 30 and earlier. On some manufacturers' devices, `allowBackup="false"` does not stop D2D transfer. |
| F10 | `Core/Sys/Sys.jet:316` | `core.sys.data_dir` exists and `native.data_path` does not. The note now says both. |

## Peer practice that matters for this round

The first review's peer survey stands (Replicache and Zero, Electric, PowerSync, CouchDB, Automerge and Yjs, Firestore, Realm). The points that decide this round:

- **Electric** joins a snapshot to the stream with the snapshot's own `xmin`, `xmax` and `xip_list`, and never with an LSN watermark that it derives itself [ELEC-HTTP]. The second draft derives a watermark W from commit LSNs and snapshot visibility. That is a different and less proven construction (N1).
- **PowerSync** does not advance a checkpoint while local uploads are pending [PS-CONS]. Local predicted state is therefore never overwritten by server state that does not yet include it. The second draft's replay rule "apply only if newer" has no such guard (S8).
- **Zero** keeps a limited, sorted query as one incrementally maintained view and sends row changes, not a re-run result [ZERO-Q]. The second draft adopts the one-window idea but re-runs and resends the whole window (S2).
- **SQLite WASM and PowerSync's web SDK** solve multi-tab access with a VFS (`opfs`, `opfs-wl`) or a worker that coordinates the tabs. The second draft builds its own leader tab (N4).
- **Android and iOS** back up app databases by default. The cost of excluding them is the user's data when the phone is replaced, which matters for an app where all data has device authority (S10).

## S1. First paint does not grow with the workspace

**Second draft.** The claim is narrowed to first paint. After first paint, a read is local only if its exact key is cached. Ballot B2 offers a row store as the alternative.

**Attack.** The narrowing is honest and matches the first review's finding. One residue: the note still says nothing about a cold native start. A phone with no cache waits for the network on every read, because the server-renders-first argument is web-only. That is a limit, not a hole.

**Verdict: holds only within stated limits** (first screen; on the web, server-rendered).

**Remaining gaps.** None new.

## S2. Bounded reads: one live window per list, typed oversize, limit after policy

**Second draft.** Cached pages are replaced by one live window that grows (50, 100, 150 rows) as one query over one snapshot, so rows appear exactly once. An oversized result fails as `CallError.TooLarge`, is stored, and is not re-run on each read. The encoder stops at the cap. `jet inspect choices` flags list endpoints with no `limit`. Limit after policy waits for #6827 c3 and #6869.

**What is engineered away.** Page inconsistency (G7). The per-read re-run of oversized results (G8). The dependency on unbuilt read sets for paging (G2), because a window needs no per-range invalidation.

**Attack.**

- **The cost is named but not measured or mitigated.** The note says "an invalidation re-runs the whole window", but nothing quantifies it. Under table-level invalidation, which is all the code has (F1), any task write re-runs every open window over `tasks` for every subscriber. Caller-dependent windows are not shared (#6868 c6). The device is then told to re-fetch, and the whole window travels again: up to 4 MiB per write per subscriber. Zero's best case sends the changed rows only. Relay and Apollo merge mutation results into a normalized cache by row identity, without refetching pages. Neither mitigation appears.
- **Past the cap, the paging problem returns.** "Reading past the cap uses a non-live paged read, which the device does not cache as current." Those rows are not consistent with the window, and they are not available offline. The note does not say so in its offline limits (5.4).
- **The unbounded-list rule is name-based.** "No parameter named `limit` with type `Int`" flags a list that is bounded by its predicate (for example, "my five pinned boards") and misses a list whose limit parameter has another name. AGENTS.md's choose, explain, pin rule asks for a pin to silence a false positive, and the note has none.
- **Window keys fragment the warm set.** A scrolled screen reads `board(id, limit: 100)`, while the warm set reads `board(id)` with the default limit. These are different keys, so the coverage check (S3) must treat window sizes as one family.

**Verdict: holds only within stated limits.** Consistency is fixed. The re-run and transfer cost per write is stated without a number or a mitigation, and the past-cap pages fall outside both the live window and offline reach.

**Remaining gaps.** G44, G45.

## S3. The warm set

**Second draft.** Warm reads are one-shot reads, checked at reconnect, with no server subscription. A `loop` is one batched request in one read transaction. Where the network type is unknown, warming runs in the foreground up to 16 MiB per launch. A compile warning compares warm calls with the screen graph (#6811). The offline limits are stated, and ballot B2 offers a row store.

**What is engineered away.** Server subscription cost (G11). N+1 serial requests. Fallback without a network type. Stated offline reach (G12).

**Attack.**

- **The coverage check has no owner.** #6811 is the SEO card (F4), and none of its criteria record which endpoints a screen reads. The fallback ("devtools logs a warm key no screen read") catches drift only at run time, in a session that happens to visit the screen.
- **Warm entries are not re-checked while online.** Catch-up runs "at each reconnect". A device that stays online for eight hours never re-checks its warm entries, because they hold no subscription. If the network then drops, those entries are hours old. The age label keeps this honest, but the note should re-check warm entries at screen open and periodically, or say that it does not.
- **A batched loop can exceed every cap.** Fifty boards in one response at up to 4 MiB each is 200 MiB, far past the phone budget, and the note gives no per-item cap or streaming.

**Verdict: holds only within stated limits** (enumerated keys only; coverage not checked at compile time until an owner exists).

**Remaining gaps.** G10 (still open, wrong owner card), G42, G43.

## S4. Budget, eviction, `QueueFull`

**Second draft.** One SQLite file per account with per-row access. One byte budget, with a 256-byte overhead per entry. Least-recently-used eviction, with entries a mounted screen reads pinned. The defaults are named as unmeasured, and the measurement is named. `QueueFull` is reserved before prediction, uploads included.

**What is engineered away.** G13, G16, and the account part of G27.

**Attack.** The measurement (G14) is still a promise, but the note makes it a precondition ("replaced by the measured ones before the store is built"). That is acceptable as a criterion, and it stays open until the numbers exist. "Pinned entries alone pass the budget, so the store goes over budget" is an honest bound. On the web, `estimate()` in incognito or guest mode can be tiny or not persistent [SQLITE-WASM]. The failure table has no row for this (G58).

**Verdict: holds only within stated limits** (numbers unmeasured).

**Remaining gaps.** G14 (open), G58.

## S5. Catch-up by version: never keep an older value as current

**Second draft.** SQLite uses a version counter written in each write transaction and read in the reader's own read transaction. Postgres uses commit LSNs stamped by a logical-replication reader, with a read's watermark W derived from `pg_current_snapshot()`. Invalidation is published after commit (F1). The monotonic store rule tracks the highest invalidation version per key. The log is a table in the application database, and `LISTEN/NOTIFY` is only a hint, backed by a poll every second. Read-set summaries are carried by the device. Results have 128-bit checksums. A store id, an epoch and a commit nonce detect a restore.

**What is engineered away.** Most of G3, G4, G5 (design), G6 and G35. This is the largest improvement in the draft, and its overall shape matches the best practice of PowerSync and Electric.

**Attack.** Three holes remain, each a concrete wrong-value path.

1. **The server-side registry still has the race (F2).** The note's 3.2 says the race is closed. In the code, a re-run that started before the commit and stores after the commit's invalidation clears `dirty` and keeps the old value. The device-side rule of 3.3 then makes it worse. The device has seen invalidation version v_new. The server's cached result carries v_old, so the device re-reads. The server answers from its registry entry, which is clean, with v_old again. The device re-reads in a loop, or gives up stale. The server registry needs the same generation rule as the device.
2. **Writes that Jet does not make bypass the versions.** The SQLite counter and the Postgres log row are written by Jet's write path. Admin SQL, `db.migrate` data migrations, another service and a manual fix all change rows without advancing anything, and every cached result over those rows stays "current" for good. Electric and PowerSync read the WAL or replication stream, so they see every writer. The note's own log reader already follows a logical slot, so it could derive write sets from the decoded changes instead of trusting an inserted row.
3. **The log reader can lag behind commits (N1).** At catch-up, a write that has committed but is not yet stamped has no LSN. The question "any log row after version v that meets the read set" misses it, so the server answers "current" wrongly. Catch-up must wait until the reader has passed the WAL position current when catch-up began, or treat unstamped rows as newer than everything.

Smaller points:

- **The stamping can feed itself.** The stamping UPDATE is itself a write that the logical slot decodes. It must be filtered out, or kept in a table the reader ignores, or it feeds itself.
- **Read sets come from the device.** Under D-INVALIDATE1, a summary of key ranges and predicates per key has no size bound, and it is trusted device input. That is harmless for the device's own freshness. It must not feed any server-side decision, such as an audit that a revoked device purged data.
- **The device needs versions on every event.** The hub events of #6868 c9 "carry no source or key". The note does not say they carry the commit version that 3.3 needs.

**The note's question, whether the log is justified before #6554.** No. Under table-level invalidation a read set is a set of tables, so the server needs only each table's highest write version to answer catch-up. That is a map from table to version, updated in the write transaction. It answers exactly as the 30-day log does, with no log scan, no retention and no growth. The store id and nonce check (3.7) needs only the nonce of each version the device holds, and the server can keep those for a bounded window. The log earns its cost only with D-INVALIDATE1 ranges.

**Verdict: fails, narrowly.** The central rule is now stated correctly. It is broken by the server registry race that the note says is closed, by writers outside Jet, and by catch-up running ahead of the log reader. Each is fixable with one rule and one case (G36 to G39, G41).

**Remaining gaps.** G5 (measurements, open), G36, G37, G38, G39, G40, G41, G60.

## S6. Revocation

**Second draft.** Endpoint epochs (logic, policies, argument encoding). `SessionRevoked` on the first request deletes the account's store. Membership tables join the read set. A token refresh with changed claims advances a policy epoch. Ballot B5 offers a maximum offline age. Platform file protection is used, and the window is stated.

**What is engineered away.** G17 and G18 in design.

**Attack.**

- **"Before it shows anything" is inconsistent with offline-first.** The device shows cached results at launch, before any request. The deletion happens at the first connection, after the data has been on screen. The text should say so.
- **The window is unbounded by default.** B5 option A (none by default) leaves the revocation window open forever on a device that never reconnects. Firestore and PowerSync have the same window, so this is parity, not a regression. But the note's own G18 answer only becomes protective under option B, or with a required choice at the entry.
- **Membership read sets depend on #6869.** The membership rule needs policies that read tables. Today's policies (`true`, `owner == user`) read none (#6869, open). The note lists that precondition.

**Verdict: holds only within stated limits** (same window as the peers; unbounded unless B5 B is chosen).

**Remaining gaps.** G47.

## S7 and N3. Wire versions and endpoint epochs in the key

**Second draft.** The key is (account, store id, wire name, endpoint epoch, canonical arguments). The epoch is a hash of compiled logic, policies and argument encoding. Arguments are encoded canonically, with defaults omitted.

**Attack.** This is a new claim (N3), and it breaks the property S7 promised, keeping unchanged entries across an update.

- **The epoch's source is not stated.** If the client computes it from its own build, a device that installs an app update while offline (common on a plane: the store updates overnight, the person opens the app in flight) computes new keys and finds an empty cache, although every entry is still valid at its age. If the server supplies it, the device cannot form keys offline. Either way, an epoch in the lookup key costs offline reads. It belongs in the entry, compared at catch-up.
- **"A hash of compiled logic" changes with the compiler.** A compiler or Core upgrade that changes code generation changes every endpoint's hash, so every device re-reads everything after every toolchain bump. The hash should cover the endpoint's typed source call graph and the policies it reads, not the generated code.
- **Canonical encoding rests on #6558**, which is in planning (F4). The note says so.

**Verdict: N3 fails as specified.** An epoch in the lookup key, from an unstated source, makes the cache unreachable offline after an app update. **S7 holds only within stated limits** once the epoch moves out of the key (G46).

**Remaining gaps.** G46.

## S8. Partitions: per-page versions, replay, moves, per-row policy

**Second draft.** Each snapshot page is its own read transaction with its own v_i. After the last page, the server replays every change after min(v_i), and the device applies a change only if its row version is newer. The partition stays hidden until a checkpoint is complete and checksummed, then is applied in one transaction. A move is a delete and an insert at one version. Resume restarts from the first page after log expiry. The row policy is checked per subscriber and per row.

**What is engineered away.** The torn snapshot (G20), exposure before completion, moves between partitions, restart after log expiry, and over-sending refused rows (G21). The structure now matches Electric's and PowerSync's.

**Attack.**

- **Replay by version contradicts collection merge.** Collections are D-SYNC2 data, merged as CRDTs. A device with an unsynced local edit to a row, receiving a server change with a newer version, replaces the row under "apply only if that version is newer than the one it holds". That loses the local edit, or rolls it back until rebase. PowerSync avoids this by not advancing a checkpoint while uploads are pending. A CRDT collection should merge, not replace. Either rule must be stated, with a case.
- **Snapshot rows have no version source on Postgres.** A row in page i was read at watermark v_i, but the note does not say that this is the row's version. The commit LSN of the transaction that last wrote the row is not stored with the row. A workable rule exists and should be written down: a snapshot row's version is its page's watermark, and a replayed change applies if its version is greater.
- **Rows leaving the policy are not covered.** 7.4 says a refused row is "never sent". It does not say that a row the subscriber *could* read and now cannot (a task made private, a member demoted) is sent as a delete. Without that, a revoked row stays on the device, online, until the partition is dropped. This is the policy form of move-out, which Replicache's CVR and Electric's move-out both handle (from knowledge for Replicache).
- **Membership changes re-evaluate whole partitions.** A role change alters which rows of a partition a subscriber may read. That is a re-snapshot per affected subscriber, and its cost is not stated.
- **Per-subscriber checksums are costly.** Policy filtering makes each subscriber's row set different, so a checkpoint checksum is per subscriber. Computed directly, it costs time in proportion to the partition at every checkpoint. PowerSync uses additive checksums over bucket operations. The note gives no method.

**Verdict: fails.** The snapshot mechanics are now sound. The replay rule as written loses local edits to a merged collection, and rows that leave a subscriber's policy are never retracted.

**Remaining gaps.** G48, G49, G50.

## S9. Local-only app via in-process mount

**Second draft.** `.mount(crew, authority: .Device)`. Jobs and schedules run in-process while the app runs. Mail, webhooks, push sends, signing keys and account flows are compile errors in a client build. `.serve()` opens no listening socket. Device migrations copy, migrate and restore, and refuse downgrades.

**What is engineered away.** G23 and G24 in design.

**Attack.**

- **A mixed service is never split.** In a synced app with one `.Device` collection, the service's methods that touch the `.Device` store must run on the device and the rest on the server. The note does not say how a service is divided, what happens to a method that touches both stores in one transaction, or whether a server row may reference a device row (it would dangle on the server).
- **Missed schedules are undefined.** A schedule that fell due while the app was closed either runs once at launch or is skipped, and the note picks neither.
- **Migration is wasteful and fragile for caches.** Copying a 256 MiB cache to migrate it doubles disk use for data that can be thrown away. Caches should be dropped and rebuilt, and only `.Device` data copied. The copy must use the SQLite backup API or `VACUUM INTO` after a WAL checkpoint, because a raw file copy of a WAL database is not consistent. A full disk must refuse the migration and keep the old store readable.

**Verdict: holds only within stated limits** (whole-app `.Device` mounts; mixed services unspecified).

**Remaining gaps.** G54, G63, G58 (missed schedules).

## S10. Device-authority data stays on the device

**Second draft.** One authority concept (B3). `.Device` stores are excluded from OS cloud backup by default (iOS `isExcludedFromBackup`, Android backup rules), with a `backup: .Include` option. Telemetry carries no argument values read from `.Device` stores. Ballot B4 offers a type marker, or no check, documented.

**What is engineered away.** G26 and G34 in design. G25 becomes an honest ballot.

**Attack.**

- **Android has three switches.** Excluding cloud backup is not enough (F9). API 31 and later need `dataExtractionRules` covering both `cloud-backup` and `device-transfer`. API 30 and earlier need `fullBackupContent`. `allowBackup="false"` alone does not stop D2D on some devices. The note names "Android backup rules" without the three.
- **Desktop backups are unstated.** Time Machine, Windows File History and backup tools on Linux all copy the app data directory. The limit should be stated.
- **The default loses data for local-only apps.** For an app where every store is `.Device`, excluding from backup by default means a person who replaces their phone loses all their notes. The first review asked for exclusion by default (G26), and that was too blunt. Privacy argues for exclusion. Durability argues for inclusion, since the backup goes to the person's own end-to-end encrypted cloud (F9: "end-to-end encrypted on devices running Android 9 or higher"). The note should state this cost and ballot it, or include whole-app `.Device` data by default and exclude caches.
- **The telemetry rule cannot be enforced under B4 B.** Without a marker, the runtime cannot know that a value came from a `.Device` store. The rule must be "spans carry no argument values at all unless declared", or it must depend on B4 A.

**Verdict: holds only within stated limits.** It holds for Jet's sync channel and iOS backup. Android D2D, desktop backup and telemetry enforcement under B4 B are open. The new default trades privacy for data loss in local-only apps without saying so.

**Remaining gaps.** G55, G56.

## S11 and N5. Switching sync on later; deterministic seed IDs

**Second draft.** The person picks the account. Rows go up in dependency order, and a cycle is created then updated. Request keys combine the row ID and the device ID. Batches of 100, with progress recorded per row. A per-field report. Refused rows wait locally with their dependants. Seeded rows get deterministic UUIDv5 IDs (app ID, seed name), so two devices joining one account share them.

**What is engineered away.** Most of G29 and G30.

**Attack.**

- **N5: seed IDs collide across accounts and break #6867 c6.** A UUIDv5 of (app ID, seed name) is the same for every user of the app. Person A's "Inbox" and person B's "Inbox" have one ID. B's upload meets `Exists` for a row that A owns, or overwrites it. It is also not UUIDv7, and #6867 c6 makes the ID decoder refuse anything but UUIDv7 (F5). The ID must include the account, which is unknown while the data is local, so seeds must be re-keyed at join time (with their references renumbered) or created lazily per account.
- **Resume after 24 hours fails.** Request-key results live 24 hours (F6). An upload interrupted for longer (a crash, then a week offline) resends keys the server has forgotten, and `Exists` is ambiguous again. Per-row progress on the device helps only if the server can confirm by row ID and owner.
- **The ending is still undefined.** After the upload, the note does not say what the local store becomes: an O4 A cache, a retired file, or a `.Device` store kept beside the synced data.

**Verdict: S11 holds only within stated limits. N5 fails.**

**Remaining gaps.** G52, G53.

## S12. Sync off for a synced app

**Second draft.** Reads and pushes continue online. Only writes are held. Held writes are flagged after 7 days, and the age can be changed. Sign-out asks before discarding. Release rebases (#6867).

**Attack.** The behavior is well defined. Rebase depends on #6867, which is listed as a precondition.

**Verdict: holds.**

## S13. Sign-out and shared devices

**Second draft.** One file per account for every device store. A local-only app uses one device account per OS user. `.Device` drafts belong to the writing account. Sign-out checkpoints with truncate, then deletes. `secure_delete` is on while the store lives. The flash limit is stated.

**What is engineered away.** G27 and G28.

**Attack.** Drafts written while no one is signed in, in a mixed app, have no owner, and the note does not say where they go. Deleting the whole file is not covered by `secure_delete`. The note relies on platform disk encryption there and says so, which is honest.

**Verdict: holds only within stated limits.**

**Remaining gaps.** G62.

## S14. The failure table

**Second draft.** Eighteen rows, covering every row G32 named, plus tooling.

**Attack.**

- **The corruption row destroys local-only data.** "A store that fails SQLite's integrity check is deleted and rebuilt, keeping the queue file". For a cache, that is right. For a `.Device` store it destroys the person's only copy. A corrupt `.Device` store must be kept, refused with a typed notice, and offered for export or recovery. "The queue file" also contradicts 2.1 (one SQLite file per account) and #6867 c4 (the web queue lives in IndexedDB).
- **The new components bring failure modes the table omits:**
  - the logical replication reader down, while its slot retains WAL on the primary until the disk fills (bounded by `max_slot_wal_keep_size`, from knowledge);
  - the log reader lagging behind commits at catch-up (S5);
  - writes made outside Jet (S5);
  - an app updated while offline (N3);
  - a web deployment without COOP and COEP headers, where the SQLite `opfs` VFS will not load (F8);
  - incognito or guest storage;
  - a leader tab that is frozen or backgrounded (N4);
  - schedules missed while the app was closed (S9);
  - the device clock behind or ahead when showing ages.
- **The age display uses the device clock.** "Ages shown use the server time of the last read" still computes "now minus then" with the device's clock, so a skewed clock shows wrong or negative ages. The device should record the moment of receipt and show elapsed time.

**Verdict: fails.** One row destroys user data, and the redesign's own components add failure modes the table does not list.

**Remaining gaps.** G57, G58, G61.

## N1. The Postgres watermark rule

**Claim (3.1).** W is the largest commit LSN such that every logged write at or below W is visible in the read's snapshot. A visible write above W only causes an extra re-read.

**Attack.** The rule is safe only if every write with a commit LSN at or below W is already stamped when W is computed. In Postgres, the order of commit records in the WAL and the order in which commits become visible to snapshots can differ: a transaction can be visible to a snapshot while one with an earlier commit LSN is not yet (from knowledge; this is why Electric and logical-replication consumers carry `xmin` and `xip_list` rather than an LSN watermark). Suppose such a write is not stamped yet because the log reader lags. W is computed above its LSN. It is later stamped below W and is invisible to the read, so catch-up from W never reports it. The fix is one rule: W is at most the reader's confirmed position at the time of the read, and catch-up waits for the reader. Read replicas are unaddressed. A snapshot on a replica is not comparable with the primary's LSNs without the replica's replay position. Long transactions and subtransactions do not break the rule, because `pg_current_xact_id()` names the top-level transaction, and a long transaction only keeps W low (extra re-reads, not wrong values).

**Verdict: fails as written. It holds once W and catch-up are bounded by the log reader's position** (G37).

## N2. Catch-up without per-key server state

**Claim (3.5).** The device carries each key's read-set summary, so the server needs no per-key state.

**Attack.** It is sound for freshness. The summary is trusted device input, its size under D-INVALIDATE1 is unbounded, and under table-level invalidation it reduces to a table list that a per-table version map answers more cheaply (S5).

**Verdict: holds only within stated limits.**

**Remaining gaps.** G39, G40.

## N4. The leader tab on the web

**Claim (8.1).** One tab holds a Web Lock, opens the OPFS file and serves other tabs over `BroadcastChannel`.

**Best existing.** SQLite WASM's `opfs-wl` and `opfs` VFSes give multi-tab access directly (F8). Chromium's `"readwrite-unsafe"` mode allows several writers (F7). PowerSync's web SDK coordinates tabs through a shared worker (from knowledge).

**Attack.**

- **The leader must be a dedicated worker**, because sync access handles exist only there (F7). The note says "tab".
- **Background tabs are throttled and frozen** (Page Lifecycle, from knowledge). A frozen leader holds the lock, and every other tab hangs. The note has no detection and no `steal`.
- **Requests in flight during a handover are lost or doubled** unless they carry IDs and are idempotent.
- **The queue and the cache are not written atomically.** #6867 c4 puts the queue in IndexedDB under its own Web Lock, while the cache lives in OPFS. A predicted write updates both stores with no atomic commit across them, and the crash order is unstated.
- **The note does not compare its leader tab with the VFS modes**, which its own section 14 asks for.

**Verdict: holds only within stated limits** (one live leader; no handover or freeze rules).

**Remaining gaps.** G51.

## The note's section 14 questions, answered

1. **Does W never let a device keep an older value?** Not as written: see N1. It holds when W is at most the log reader's confirmed position and catch-up waits for the reader.
2. **Does table-level catch-up justify the log before #6554?** No. A per-table version map gives the same answers. Build the log with #6554 (G39).
3. **Window against Relay-style merged pages, in re-run cost per write.** The window re-runs and resends up to the whole window, up to 4 MiB, per write per subscriber. Merged pages or row diffs send only changed rows. Keep the window for consistency, and deliver row diffs keyed by ID, as Zero does (G44).
4. **The leader tab against SQLite WASM's OPFS modes.** Prefer `opfs-wl`, or a worker that coordinates the tabs, over a hand-built leader. If the leader stays, it must be a dedicated worker with freeze detection and idempotent requests (G51).
5. **Is B2 A enough once offline search is required?** No. Offline search over B2 A finds only exact cached queries. Search needs B2 B or a local full-text index over cached rows. If offline search becomes a requirement, B2 A is ruled out.

## Verdicts

| Claim | First review | Second review |
|---|---|---|
| S1 First paint | holds within limits | holds only within stated limits |
| S2 Bounded reads (one window) | fails | holds only within stated limits |
| S3 Warm set | holds within limits | holds only within stated limits |
| S4 Budget, eviction, `QueueFull` | holds within limits | holds only within stated limits |
| S5 Catch-up by version | fails | fails, narrowly (server registry race, writes outside Jet, log-reader lag) |
| S6 Revocation | holds within limits | holds only within stated limits |
| S7 Wire versions | holds within limits | holds only within stated limits, once the epoch leaves the key |
| S8 Partitions | fails | fails (replay overwrites merged local edits; no retraction on policy loss) |
| S9 Local-only mount | holds within limits | holds only within stated limits |
| S10 Device data stays on device | fails | holds only within stated limits |
| S11 Sync on later | holds within limits | holds only within stated limits |
| S12 Sync off | holds within limits | holds |
| S13 Sign-out, shared devices | fails | holds only within stated limits |
| S14 Failure table | fails | fails (corrupt `.Device` store deleted; new components' failures missing) |
| N1 Postgres watermark (new) | | fails as written |
| N2 Device-carried read sets (new) | | holds only within stated limits |
| N3 Endpoint epoch in the key (new) | | fails |
| N4 Leader tab (new) | | holds only within stated limits |
| N5 Deterministic seed IDs (new) | | fails |

Of the six claims that failed before, three now hold within stated limits (S2, S10, S13), and three still fail (S5, S8, S14). Three of the five new claims fail (N1, N3, N5). Under AGENTS.md the note is not yet a reviewed design, and #3709 c1 stays open.

## Remaining gaps, as criterion text

Gaps G1 to G35 of the first review are answered by the second draft, except those listed here as carried. New gaps continue the numbering.

| Gap | Proposed criterion text |
|---|---|
| G5 (carried) | Before the write log is built, the note records two measured numbers: log bytes per day for Crewboard with 200,000 tasks under its seeded write load, and server time to catch up a device holding 1,000 keys after 30 days. |
| G10 (carried) | The warm-coverage check has an owner card that records, per screen, the endpoints and argument shapes it reads, and treats window sizes of one list endpoint as one family. #6811 (SEO) is not that card. Until it lands, the note states that coverage is checked only at run time. |
| G14 (carried) | The device budgets are replaced by numbers measured on Crewboard with 200,000 tasks (bytes per board, per warm set and per row duplicated across results, on desktop, phone and web) before the store is built. |
| G22 (carried) | #6868 c9 lands: the live hub (`Compiler/JetRuntime/Source/WsRoutes.jet:470-602`, unchanged on dev `9ae575e506`) sends an event only to connections watching an affected binding whose caller passed its policy, and `web_app/live_hub_events_reach_only_admitted_subscribers` passes. |
| G36 | The server's live registry stores a re-run's result as current only if no invalidation of that binding arrived after the re-run began (a generation counter per binding, checked in `live_store`); otherwise the result is stored stale. Proof: a case pauses a re-run after its read, commits a write and lets the invalidation land, then lets the store finish, and shows the binding is still stale and re-runs to the new row. The note's 3.2 stops calling the race closed until this lands. |
| G37 | On Postgres, a read's watermark W is at most the log reader's confirmed commit position at the time of the read; catch-up answers only after the log reader has passed the WAL position current when catch-up began; the stamping write is not itself decoded as an application write; reads on a replica are either refused for versioned results or carry the replica's replay position. Proof: a case delays the log reader and commits two writes whose visibility order differs from their commit order, and shows no device keeps an older value as current. |
| G38 | Every write to a synced table advances versions, including writes not made through Jet (admin SQL, data migrations, other services): write sets come from the decoded WAL on Postgres and from triggers on SQLite, or such writes are detected and force a store epoch. Proof: a case updates a row with plain SQL outside Jet and shows the cached result is re-read. |
| G39 | Under table-level invalidation, catch-up answers from a map from each table to its highest write version, updated in the write transaction, and the 30-day write log is built only with D-INVALIDATE1 read sets (#6554). Or the note justifies the log before #6554 with the G5 measurements. |
| G40 | A read-set summary has a byte cap per key, past which it falls back to table-level; it is used only for the device's own freshness, and no server decision about another party (audit, purge, revocation proof) relies on device-supplied summaries. |
| G41 | Every hub event and every catch-up answer carries the commit version the device's monotonic rule (3.3) compares; #6868 c9's event format includes it. |
| G42 | Warm entries are re-checked by catch-up while online, at least when a screen reading the key opens and at a stated interval, not only at reconnect. Proof: a case changes a warmed board while the device stays online and shows the entry is re-read before it is shown as current. |
| G43 | A batched warm request caps each item at `LIVE_MAX_PAYLOAD` and the batch at the remaining budget, and streams items so one oversized item fails alone with `TooLarge`. |
| G44 | The note states, measured, the re-runs and bytes sent per task write for a 1,000-row window with 200 subscribers under table-level invalidation, and delivers window updates as row changes keyed by ID rather than the whole window, or states a row cap per window with that cost. Pages read past the cap are listed in 5.4 as not available offline and not consistent with the window. |
| G45 | The unbounded-list rule of `jet inspect choices` follows choose, explain, pin: it shows the fact that flagged the endpoint, and a pin on the endpoint (for example, a declared maximum row count) silences it and fails compilation if the endpoint can exceed it. |
| G46 | The endpoint epoch is a stored field of each entry, compared at catch-up, not part of the lookup key; the device states where the epoch comes from; the epoch hashes the endpoint's typed source call graph and the policies it reads, not generated code, so a toolchain upgrade alone does not change it. Proof: a case updates the app while offline and shows cached entries still render with their age, then are re-read at reconnect only where the epoch changed. |
| G47 | Ballot B5 either defaults to a maximum offline age or requires the entry to choose one; the docs state that with no age the revocation window is unbounded on a device that never reconnects. |
| G48 | A snapshot row's version is its page's watermark v_i, and a replayed change applies only if its commit version is greater; the note states this for Postgres and SQLite. Proof: a case commits a write between two page reads and shows the row once, at the newer value. |
| G49 | Replay into a collection merges with CRDT semantics (D-SYNC2) and never discards a local edit not yet acknowledged, or the device does not apply a new checkpoint while its uploads for that partition are pending. Proof: a case edits a row offline, receives a newer server change to the same row, and shows both effects merged after reconnect. |
| G50 | A row a subscriber could read and no longer can (made private, member demoted, membership removed) is sent to that subscriber as a delete; a membership or role change re-evaluates the subscriber's partition with a stated cost; checkpoint checksums are computed incrementally per subscriber. Proof: a case makes a task private and shows it leaves a member's device while online. |
| G51 | The web store's leader is a dedicated worker; a frozen or backgrounded leader is detected and its lock stolen after a stated timeout; requests in flight during a handover carry IDs and are idempotent; the write order between the IndexedDB queue (#6867 c4) and the OPFS cache on a crash is stated; the note compares this with SQLite WASM's `opfs-wl` and `opfs` VFSes and states why it is preferred. Proof: a three-tab case closes and freezes the leader mid-write. |
| G52 | Seeded rows have UUIDv7 IDs (#6867 c6). Seeds are created per account at join, with references renumbered, or created lazily under the joined account, so two accounts never share a seed ID and two devices of one account do. Proof: a case joins two accounts and two devices of one account. |
| G53 | An upload interrupted longer than the request-key window (24 hours, #6867 c2) resumes correctly: the server confirms an `Exists` by row ID and owning account. The note states what the local store becomes after a complete join. Proof: a case resumes after a simulated 48 hours. |
| G54 | A service with mixed authorities states which methods run on the device; a method that touches a `.Server` and a `.Device` store in one transaction is a compile error; a `.Server` row cannot reference a `.Device` row. Proof: a diagnostic snapshot per rule. |
| G55 | `.Device` backup rules cover Android `dataExtractionRules` (both `cloud-backup` and `device-transfer`) and `fullBackupContent` for API 30 and earlier, iOS `isExcludedFromBackup`, and state desktop behavior. The default for an app whose every store is `.Device` is a ballot that states its cost (loss on device replacement against privacy), or includes such data by default and excludes only caches. |
| G56 | The telemetry rule is enforceable under the chosen B4 option: under B4 B, spans and crash reports carry no argument values unless the endpoint declares them safe. |
| G57 | A `.Device` store that fails its integrity check is never deleted: it is kept, opened read-only if possible, refused with a typed notice, and offered for export. Only cache stores are deleted and rebuilt. The failure table names where the queue lives on each platform. |
| G58 | The failure table adds rows, each with a case: the log reader down with its slot retaining WAL (`max_slot_wal_keep_size` stated); catch-up ahead of the log reader; writes outside Jet; an app updated while offline; a web deployment without COOP and COEP; incognito or guest storage; a frozen leader tab; schedules missed while the app was closed (run once at launch or skipped, stated). |
| G59 | The note cites live criteria: #3709 c21, c23 and c26 become #6868 c5, #6867 c6 and #6867 c7; #6868 c8 is recorded as landed on dev in `5fa1fd1c85`, with its proof case. |
| G60 | Section 3.2 is corrected: the commit-time marking of `5fa1fd1c85` narrows the race to one re-run's duration, and G36 closes it. |
| G61 | Ages shown offline are the device's elapsed time since it received the result, measured on the device, never server time subtracted from the device clock. Proof: a case with the device clock set two hours behind. |
| G62 | In a mixed app, `.Device` data written while no one is signed in has a stated owner (a device account, or refused). Proof: a case. |
| G63 | Device migrations drop and rebuild cache stores rather than migrating them; `.Device` stores are copied with the SQLite backup API or `VACUUM INTO` after a WAL checkpoint; a free-space check runs first, and a full disk refuses the migration with a notice and keeps the old store readable. |
| G64 | The two test-host abilities of section 11 (disconnect for a simulated duration; restore the server to an earlier version) have an owner card and a case each. |

## Sources

| Key | Source |
|---|---|
| MDN-CSAH | [MDN: FileSystemFileHandle.createSyncAccessHandle() (dedicated workers only; lock modes)](https://developer.mozilla.org/en-US/docs/Web/API/FileSystemFileHandle/createSyncAccessHandle) |
| SQLITE-WASM | [SQLite WASM: Persistent storage options (opfs, opfs-wl, opfs-sahpool; concurrency)](https://sqlite.org/wasm/doc/trunk/persistence.md) |
| ANDROID-BACKUP | [Android: Back up user data with Auto Backup](https://developer.android.com/identity/data/autobackup) |
| ELEC-HTTP | [Electric: HTTP API](https://electric-sql.com/docs/api/http) (opened in the first round) |
| PS-CONS | [PowerSync: Consistency](https://docs.powersync.com/architecture/consistency) (opened in the first round) |
| ZERO-Q | [Zero: Queries](https://zero.rocicorp.dev/docs/queries) (opened in the first round) |

These facts come from knowledge and were not opened this round. The redesign should confirm each against its documentation:

- Postgres commit-order versus visibility-order behavior, and `max_slot_wal_keep_size`;
- the Page Lifecycle freezing of background tabs;
- PowerSync's shared-worker tab coordination;
- Replicache CVR retraction.
