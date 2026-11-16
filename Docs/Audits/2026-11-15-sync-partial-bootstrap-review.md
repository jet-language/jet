# Adversarial review: Sync partial bootstrap and local-only mode (2026-11-15)

This review covers the design note `Docs/proposals/2026-11-15-sync-partial-bootstrap-and-local-only.md`, the answer to Tower card #3709 c1. The note is still a patch (lane GoalsB-5-17). It is not on dev. The review follows AGENTS.md, "Adversarial design review": for each claim it names the strongest existing systems and their best case, attacks the claim, checks it against the code where it can, and gives a verdict. Every gap ends as proposed criterion text in the gap table at the end.

The repository was read at dev `1f32878176` (the read-only mirror at `$HOME/Projects/Github/jet`). Nothing was built and no test suite was run. Every check is a read of the code or the Tower board, recorded as R1 to R14 below. The note's design is proposed and not built, so no runnable probe of it exists. Where a check rests on reading code rather than running it, the text says so.

## What the note claims

The note adds two things to the shape (`Docs/proposals/2026-11-13-web-app-framework-shape.md`, O4 A: the server is the authority, and the device keeps the last live results and a write queue):

- **Partial bootstrap** (its sections 1 to 4). The reads decide what reaches the device. Live reads are bounded and paged. An entry can declare a warm set of reads to fetch for offline use. A device has a byte budget with eviction. Catching up after a long absence sends cached keys with versions, and the server answers only the keys that changed. Revocation removes entries. Entries are keyed by wire version. Collections are loaded as partitions, from a resumable snapshot followed by changes.
- **Local-only mode** (its section 5). A whole app runs with the service mounted in-process. One collection can be set to `sync: .Off`. Sync can be switched on later or off for a synced app. Sign-out and shared devices are handled.

The review splits these into fourteen claims (S1 to S14).

## Repository facts

| # | Read | Observed |
|---|---|---|
| R1 | `Core/App/App.jet:375-392, 467-552` | The live registry is one process-wide in-memory cell (`LIVE_CELL: Shared{LiveCell{...}}`). It holds every binding as length-prefixed text. Every `live_current`, `live_store`, `live_register` and `live_invalidate` decodes the whole cell and re-encodes it, so each read of one entry costs time in proportion to the bytes of all entries. Nothing writes it to disk. |
| R2 | `Core/App/App.jet:503-541` | At `LIVE_MAX_QUERIES` (1024) the registry drops the first non-sync record in insertion order. That is first-in, first-out, not least recently read. A binding that a mounted screen is reading can be the one dropped. |
| R3 | `Core/App/App.jet:404-417, 566-569` | `live_store` returns without storing a value larger than `LIVE_MAX_PAYLOAD` (4 MiB). `state` then finds no stored value on its next call and runs the source again, so an oversized result is re-run on every `state` read, not only "silently not stored" as the note says. |
| R4 | `Core/App/App.jet:501, 436-458` | An endpoint binding's key is `live_key(args)`, the encoded arguments, under a source name. The key does not include the caller or the account. |
| R5 | `Core/App/App.jet:586-614` | Invalidation marks bindings of a source, or one exact key, as dirty. There are no versions. The WebSocket event carries `invalidations`, a per-process counter, not a commit version. |
| R6 | `Core/Db/Db.jet:842-852, 812-836` | `__scope_execute` publishes the table's invalidation right after the statement runs, inside the open transaction. A successful `__db_commit` publishes nothing more. A rollback invalidates again. So a re-run that reads on another connection between the statement and the commit sees the old row, stores it as clean, and no later event marks it stale. |
| R7 | `Core/Db/Db.jet:736-749, 861-885` | `DBScope.live` keys a binding by user, policy-applied SQL and values. Its invalidation source is the table (`core.db.live:<table>`). Invalidation is per table. No read set, range or predicate is recorded anywhere in Core or Compiler: a search for `read_set`, `write_set`, `ReadSet` and "read set" finds none. |
| R8 | `Core/Db/Db.jet:303-330, 625-690` | Row policies have two forms only, `true` and `owner == user`, applied by rewriting SQL text. Joins and subqueries that name the policy's table are refused. `Table.guard`, `db.Who` and `db.Op` (#3709 c22) do not exist in Core. |
| R9 | `Compiler/JetRuntime/Source/WsRoutes.jet:470-602` | Every WebSocket connection joins the live hub once its upgrade is written. Each published event goes to every joined connection, and a joining connection receives the latest event of every topic. Nothing checks the caller or filters topics. |
| R10 | `Core/App/App.jet:695-749`, `Core/Sync/Sync.jet` | `app.sync` merges CRDT documents (`core.sync`) into a session record of the same process-wide registry. It has no partitions, no policy, no version log and no persistence. `app.collection` does not exist. |
| R11 | `Core/App/App.jet:236, 241` | `app.remote(service)` (run in process) and `app.remote_at(address)` (POST to a server) both exist, as the note says. |
| R12 | Tower `#3709` c14–c26, and `#6867`, `#6868`, `#6869` | #3709 c14 to c26 are not met with proofs. Each one's evidence reads "Moved to #6867 cN / #6868 cN / #6869 c1": the offline queue went to #6867, live-query scaling to #6868 and row policies to #6869. All three cards are in phase `ready`, and every criterion on them is open. Their named proofs do not exist yet: a search of `tests/` and `Compiler/` finds none of `db/row_policy_judges_each_operation`, `web_app/offline_create_with_client_id`, `web_app/caller_dependent_live_query_is_not_shared`, `web_app/presence_counts_only_admitted_readers`, `web_app/refused_queued_write_without_a_screen_becomes_a_kept_notice`, `sync_two_clients` or `app_live_write_query`. `tests/cases/sync_crdt/` is empty, and `tests/cases/web_app/` holds 37 files, none about offline or sync. |
| R13 | Tower `#6554` (D-INVALIDATE1 build), `#6827`, `#6558` | All three are in phase `planning` with every criterion open, including #6554 c1, D-INVALIDATE1's own adversarial review. The note's "D-INVALIDATE1 already keys by read set" rests on unbuilt and unreviewed work. "Limit counts rows after the row policy" rests on #6827 c3, which is open. |
| R14 | `Core` search | `native.data_path`, `app.new_id`, UUIDv7, `navigator.storage`, OPFS and IndexedDB appear nowhere in Core. `core.sys.data_dir` exists (`Core/Sys/Sys.jet:316`). `Core/Web/Query.jet` still exports `live`, `subscribe` and `invalidate` (#3709 c8 open). |

**Review-wide finding.** The note relies on the offline queue, live-query scaling and row-policy criteria (#3709 c14–c26, now open on #6867, #6868 and #6869, R12) as if they were built, and on mechanisms the shape describes as decided (R7, R8, R13). The code has none of them: invalidation is per table, policies have two forms, the registry is in memory and keyed without the caller, and invalidation fan-out is unauthenticated. The rule is that missing context counts as a failed claim, not a pass. Each claim below is judged against what the code shows, and where a claim depends on unbuilt work, that dependency is named as a gap.

## The strongest existing systems, at their best

- **Replicache and Zero** (Rocicorp). Replicache pulls with a cookie and gets a patch computed against a per-client view record (CVR). The patch includes deletes of rows the client may no longer see, so a change of filter or permission is the same code path as a data change. Zero is query-driven. The client names queries and arguments, and the server resolves them to ZQL under a server-side `context`, which enforces permissions. zero-cache maintains the results incrementally from Postgres logical replication. The client stores **rows, not results**, in IndexedDB or SQLite: "Zero syncs the union of all active queries' results", so overlapping queries do not duplicate rows, and any query can run locally over the rows on the device. `zero.preload()` is Zero's warm set. Zero states its own consistency gap: local results are trusted only when they are a prefix of the server's, and a consistency model to fix this is "in the future". Zero query TTLs are at most ten minutes and tick only while the app runs. Sources: [ZERO-Q], [ZERO-LLMS], [REP-CVR].
- **ElectricSQL shapes.** A shape is a table, a `where` clause and columns, synced as a log. The initial sync pages through the log with an `offset` and a shape `handle`. An `up-to-date` control message marks the point where the client holds everything the server knew. `must-refetch` tells the client to throw its copy away and sync again. Subset snapshots return Postgres snapshot metadata (`xmin`, `xmax`, `xip_list`) in a `snapshot-end` message, so the client can skip changes already in the snapshot. That is the exact mechanism the note leaves unstated for joining a snapshot to a live stream. Authorization is delegated: Electric recommends that production apps proxy shape requests through the app's backend, which sets the shape parameters. Source: [ELEC-HTTP].
- **PowerSync sync rules and Sync Streams.** Server-side declarations put rows into buckets per user, from JWT claims and parameter queries. The client gets every row of its buckets into SQLite and queries it locally, including full-text search and infinite scroll. Consistency is by **checkpoint**: "The client only updates its local state when it has all the data matching a checkpoint", with no intermediate state, across all buckets, and Jepsen-tested causal+ consistency. Checkpoints carry checksums. While the upload queue holds writes, the client does not advance to a new checkpoint. PowerSync also ships **local-only tables** (`localOnly: true`), which are never queued for upload, with a documented copy-to-synced-table migration and a `viewName` swap. Its docs warn that turning an existing local-only table into a synced one deletes the table's data. Sources: [PS-CONS], [PS-LOCAL], [PS-LLMS].
- **CouchDB and PouchDB filtered replication.** Replication resumes from a checkpoint (`_local/<replication-id>`) on the changes feed. The replication ID includes the filter function's code and the changes-feed query parameters, so **changing the filter starts a new replication from the beginning**. `_local` documents are never replicated, which is prior art for local-only data. Filtered replication is not a security boundary: read access is per database, so the idiom is a database per user. Source: [COUCH-PROTO].
- **Automerge and Yjs.** Yjs syncs by exchanging state vectors, and each side sends only the updates the other lacks. Its subdocuments load lazily, which is partial sync at document granularity. Automerge's sync protocol exchanges heads with Bloom filters, and Automerge Repo's `sharePolicy` decides which peer gets which document. Neither has built-in authorization or partial sync inside one document. Their strength is convergence without a server authority, which O4 A does not try for. Sources: [YJS-SUB], [AM-SYNC].
- **Firebase and Firestore offline persistence.** The cache holds every document received. The default threshold is 100 MB on Apple and Android, after which unused older documents are cleaned up. The cache can be set unlimited. Snapshots carry `fromCache`. The web supports multi-tab persistence. The docs warn that the web cache "isn't automatically cleared between sessions", so a web app with sensitive data should ask whether the device is trusted. Security rules apply on the server. Cached documents stay readable offline. Source: [FS-OFF].
- **Realm (Atlas Device) Flexible Sync.** Subscriptions are queries over queryable fields. Permissions are server rules. A write outside permissions gets a compensating write. Local realms and synced realms are configured separately. When the server's history no longer reaches a client (client maximum offline time), the client gets a **client reset** with modes such as recover or discard. MongoDB deprecated Atlas Device Sync in September 2024, so this is prior art and not a live competitor. Source: [REALM-FS].
- **Linear.** Per the talk behind [LINEAR-SCALE], large workspaces forced partial bootstrap and lazy loading on a client-side object graph that is then read and written locally. The cited page is a video page with no text, so the note's summary of it cannot be checked from the source as cited.

Against all of these, the design has one real strength: **a synced read is an ordinary endpoint call, checked by the same marker and the same row policy as an online read.** No second authorization language exists to keep in step, which PowerSync's sync rules and Electric's proxy both require. The review credits this where it applies.

## S1. Query-driven partial bootstrap: first paint does not grow with the workspace

**Claim (note section 1).** A device never downloads a workspace. The first screen costs that screen's live reads, so 200,000 tasks cost the same first paint as 200, as long as no single read returns them all. This matches Linear's partial bootstrap and Zero's query-driven sync.

**Best existing.** Zero has the same property for first paint. It also keeps a local row store, so after first paint other queries can be answered locally from rows already present. Linear's partial bootstrap loads an object graph that later screens read locally. PowerSync's prioritized sync loads the important buckets first and the rest in the background.

**Attack.**

- The claim is about first paint, but "partial bootstrap" in Linear's sense is about what the device can do *after* first paint. Here every later screen is a network read unless its exact key is cached (S3). The comparison with Linear and Zero holds for the first screen only.
- Results are stored as opaque blobs per endpoint and arguments (R1, R4). One task appears in the board, in "Mine", in search and in its own screen, and is stored four times. Zero and PowerSync store it once. That costs budget (S4) and makes prediction expensive: D-OPTIMISTIC1 must patch every cached blob that holds the row.
- "As long as no single read returns all of them" moves the hard part into S2. Under today's policies (R8) a membership-based policy cannot be expressed, so a bounded read with "limit after policy" cannot be written yet.
- On native targets the server does not "run the reads before HTML". A cold phone with no cache waits for the network for every read. The note's first-paint argument is the web's.

**Check.** R1 and R4 confirm keying by endpoint and encoded arguments, and blob storage. R8 confirms the policy limits.

**Verdict: holds only within stated limits.** It holds for the first screen and the web. It is not Linear-style partial bootstrap.

**Gaps.** G12, G13, G14.

## S2. Bounded reads: a typed failure for oversize, cursor pages cached per page, and only touched pages invalidated

**Claim (note section 2).** An oversized result becomes a typed failure. List endpoints take a limit and a cursor. Each page is cached under its own key. A write invalidates only the pages whose read set it meets ("D-INVALIDATE1 already keys by read set; a page's read set is its range"). Limits count rows after the row policy.

**Best existing.** Zero maintains a limited, sorted query incrementally as one view. Relay and Apollo merge connection pages into one normalized list keyed by row identity, so a row moved between pages appears once. PowerSync's infinite scroll pages a local SQLite query, so pages are consistent with each other by construction. Electric subset snapshots take `limit`, `offset` and `order_by`, with snapshot metadata.

**Attack.**

- **The invalidation is not there.** Today a write to a table invalidates every binding over that table (R6, R7). No read set, range or predicate is recorded anywhere (R7). D-INVALIDATE1's build card #6554 is in planning with its own review open (R13). Under the real fallback, table-level invalidation (#3709 c21), every page of every board re-runs on any task write. Per-page invalidation is unbuilt.
- **Pages are not a consistent snapshot.** Each page is read at a different time and refreshed on its own. A task moved from page 3 to page 1 between the two reads shows twice or not at all. With keyset cursors, page 2 is keyed by page 1's last row. When page 1 changes, the cached page 2 is still keyed by the old cursor, so a stale page can be stitched to a fresh one. The note has no rule for assembling a list from pages read at different versions.
- **A keyset page's read set is not a simple range.** "The first N rows after cursor c" depends on every insert or delete before its last row, and on rows entering or leaving the policy. Deriving that read set needs the predicate and the limit together, which is #3709 c21's hard case.
- **The typed oversize failure turns data growth into an outage.** A board that crosses 4 MiB as a project grows changes from working to `Failed` in production, with no warning before the limit. The server pays for encoding the whole result before it can measure it. Today the oversize case re-runs on every `state` read (R3), which is worse than the note says. The note does not say how the compiler decides that a return type is an "unbounded list" for the `jet inspect choices` row.
- **Limit after policy** depends on #6827 c3 (open) and on policies that can be pushed into SQL. Today's policy rewriter refuses joins (R8).

**Check.** R3, R6, R7, R8 and R13.

**Verdict: fails.** Per-page invalidation depends on unbuilt, unreviewed read sets. Assembling a list from pages has no consistency rule. The oversize behaviour and limit-after-policy are not specified enough to build.

**Gaps.** G2, G7, G8, G9.

## S3. The warm set makes a device usable offline beyond what it has seen

**Claim (note section 3).** An entry declares `warm: fn(crew) { ... }` with one endpoint call per statement, or a `loop` over one call's result. After first paint, on an unmetered connection, the device runs the warm reads in order, one at a time, until the budget is spent. Warm entries refresh like any live query.

**Best existing.** PowerSync needs no warm list: sync rules put a user's rows on the device, and any local query works offline, search and filters included. Zero's `preload()` is an explicit warm set like this one, but its rows serve every other query. Linear loads lazily. Firestore caches only what was read, the same as here.

**Attack.**

- **Coverage is not checked.** A warm call is useful only if its key equals the key a screen will read, meaning the same endpoint and byte-identical encoded arguments (R4). If a screen calls `board(id, filter: .All)` and the warm set calls `board(id)`, every lookup misses, and nothing reports it. The warm set restates screen logic in the entry, so the two can drift apart.
- **Offline reach stays narrow.** Offline, only enumerated keys work. Search, a new filter, sorting by another column, and any read whose arguments come from input fail with "Not available offline". PowerSync and Zero can answer these from local rows. The note's own question 1 points at this. The answer is that the warm set is not "works offline" in PowerSync's sense.
- **Server cost is hidden.** "Warm entries refresh like any live query" makes each warm entry a server subscription. Caller-dependent queries are not shared (#3709 c24), so a phone that warms 50 boards holds 50 subscriptions, each re-run on every write that meets its read set. Under per-table invalidation (R7) that means every task write. The per-server caps of #3709 c19 then refuse or starve them. The note does not say whether warm entries are subscriptions or one-shot reads that are checked at reconnect.
- **The loop is N+1, run serially.** `loop project in crew.my_projects() -> crew.board(project.id)` makes one request per project, one at a time, so 50 projects cost 50 round trips. No batched form exists.
- **"Metered" is not knowable everywhere.** The Network Information API is missing from Safari and Firefox. iOS Low Data Mode and Android data saver differ. The note gives no fallback.
- **It only runs while the app is open.** Warming runs "after the first paint". The shape's `native.background` (5.1.3) could warm in the background, but the note does not connect the two.

**Check.** R4 confirms that keys are the encoded arguments, so equivalent calls get different keys. The rest is read from the note and the shape. No code exists for the warm set.

**Verdict: holds only within stated limits.** It warms enumerated reads. It does not make an app work offline beyond those keys.

**Gaps.** G10, G11, G12.

## S4. A per-account device budget, least-recently-read eviction, and a queue that is never evicted

**Claim (note section 4).** The budget is 256 MiB on desktop, 64 MiB on phones, and on the web `navigator.storage.estimate()` capped at 64 MiB. Eviction is least recently read first, with warm entries last and in reverse warm order. Queued writes are never evicted: `QueueFull` refuses new writes.

**Best existing.** Firestore defaults to 100 MB on Apple and Android, with LRU collection of documents that no active listener uses. Rows are stored once, so the budget goes further. PowerSync has no eviction: a user's buckets are the set, bounded by per-user bucket limits.

**Attack.**

- **No store exists to budget.** The registry is in memory and decoded whole on every read (R1). A 64 to 256 MiB cache built on it would copy the whole cache for each `state` call. The design needs a new persistent store with per-entry access, and the note does not say so.
- **Two limits conflict.** `LIVE_MAX_QUERIES` (1024 entries) and a byte budget both apply, with different eviction orders. Today's order is first-in, first-out (R2), not the least-recently-read order the note states. "Least recently read first" and "warm entries last, in reverse warm order" are two orderings with no rule for combining them. Neither protects an entry that a mounted screen is reading (R2).
- **The arithmetic is thin.** 64 MiB divided by the 4 MiB result cap is 16 results. Twenty boards near the cap overflow a phone budget. Duplication across results (S1) multiplies real usage. The note admits the numbers are guesses (its 7.3).
- **The web loses data on its own.** `estimate()` reports a quota, not a grant. Safari deletes script-writable storage of a site that has had no user interaction for seven days, unless the site is installed to the home screen. Quota errors can strike mid-write. OPFS synchronous access handles are exclusive per file, so a second tab cannot open the same database file. The note's one-queue-per-origin (#3709 c16) and its local-only web store (S9) both meet this.
- **`QueueFull` arrives at the wrong moment.** The handler predicts and returns at once (shape 3.7). If `QueueFull` is decided after prediction, the person sees the change and then a rollback. Uploads (shape 5.2.3) are not counted.
- **The budget key is wrong.** The budget is "per signed-in account", but endpoint keys carry no account (R4). Two accounts on one device would share entries for the same endpoint and arguments, which is a cross-account leak as well as an accounting error.

**Check.** R1, R2 and R4.

**Verdict: holds only within stated limits.** The policy is reasonable in outline. The store, the eviction order, the account scoping and the numbers are all unbuilt or unmeasured.

**Gaps.** G13, G14, G15, G16, G27.

## S5. Catching up by version: re-read only what changed, never a wrong value

**Claim (note section 4).** Each cached result keeps "the server's invalidation version at the time it was read". On reconnect the device sends keys with versions, in pages of 500, and the server answers only the keys whose read sets met a write since. The write log is kept 30 days, and older keys get "re-read", which "costs one read, never a wrong value".

**Best existing.** Replicache's CVR strategy keeps, per client, the version of each row the client has. A pull returns puts and deletes, so filter or permission changes come out as deletes. PowerSync uses commit-consistent checkpoints with per-bucket checksums, and the client applies a checkpoint atomically. Electric uses shape log offsets and Postgres snapshot metadata, plus `must-refetch`. CouchDB checkpoints a sequence on the changes feed. Realm detects history loss and runs a client reset. Every one of these ties versions to the commit order and has a defined answer for "server history lost".

**Attack.** This is the note's central correctness claim, and it is underspecified in every place where correctness is decided.

- **What is "the version at the time it was read"?** If the server stamps the version outside the read's snapshot, a write that commits after the read's snapshot and before the stamp is never reported, and the cached value is wrong for good. The version must be the read transaction's own snapshot, and the note does not say so. Today's code already has this class of bug (R6): invalidation is published before commit, so a re-run on another connection in that gap stores the pre-commit value as clean, and nothing marks it stale again.
- **Commit order across servers.** With Postgres, transaction IDs are assigned at start, not in commit order. The version must be a commit LSN or a sequence advanced at commit. The broker the note relies on, LISTEN/NOTIFY (#3709 c20), is not durable: a server that is not listening misses notifications, and payloads are limited to 8000 bytes. A 30-day write log is a new durable server component with an unstated size per day.
- **How does the server know a cached key's read set?** It was recorded when the device read it, possibly on another server, possibly months ago. The server must either store each device's read sets, which costs storage proportional to devices times keys, or re-run each query to recover the read set, which costs the read the protocol claims to save. The saving is then bandwidth only, not server work.
- **The fallback makes the protocol pointless.** Under table-level invalidation (R7, #3709 c21), any write to `tasks` since version v means every board key changed. A month away on an active workspace re-reads everything, and the protocol only adds a round trip.
- **Out-of-order results.** A push for version v2 can arrive while a re-read at v1 is in flight. `live_store` has no version (R1, R5), so the older result can overwrite the newer one. "Never a wrong value" needs a monotonic store rule.
- **Divergence is never detected.** No checksum exists. A corrupted or mis-versioned entry stays wrong until a later write happens to touch it. PowerSync's checksums exist to catch exactly this.
- **Server restore.** If the server is restored from a backup, its versions go backwards, and devices hold versions "from the future". They would be told nothing changed. The protocol needs a server epoch.
- **Policy-dependent read sets.** A membership change changes which rows the query reads, not only their values. The read set must include the policy's reads (shape 3.2 says it does). No code records it (R7, R8).

**Check.** R5, R6 and R7 show that versions, commit-ordered publication and read sets are all absent, and that a pre-commit invalidation race exists today. The race was found by reading the code, not by running it. A case that pauses between `__scope_execute` and `__db_commit` would demonstrate it.

**Verdict: fails.** "Never a wrong value" depends on a version and snapshot rule the note does not state, on a durable log and read-set store it does not describe, and on read sets that are unbuilt. Today's code shows the race the rule must prevent.

**Gaps.** G2, G3, G4, G5, G6, G35.

## S6. Revocation: losing access removes cached data on the next connection

**Claim (note section 4, failure table).** A re-read that the row policy now refuses removes the entry. A person removed from a project loses its boards on the next connection. Data stays readable offline until then, and the note states this window.

**Best existing.** Firestore has the same window: cached documents stay readable offline, and rules apply on the server. PowerSync keeps local SQLite rows until a checkpoint removes the bucket. Replicache's CVR pull returns deletes. Realm removes objects outside the new permissions on sync, and older versions ran a client reset on a permission change. None closes the offline window, so the note is at parity on the window itself.

**Attack.**

- **Revocations without a write are missed.** Catch-up reports only keys whose read sets met a write (S5). These revocations involve no write to any read set:
  - a deploy that tightens a row policy or changes an endpoint's logic;
  - a session revoked from the device list (shape 4.7);
  - an account disabled in the account store;
  - a role change that lives outside the database.

  In each case the device keeps the data indefinitely, even while online, until some unrelated write happens to touch the key.
- **A refused re-read is often not an error.** For a list endpoint, the policy filters rows, so the re-read returns fewer rows and removal works. For a single-row endpoint, the result may be `NotAllowed` or an empty value, and the note does not say which one removes the entry.
- **The note's question 7.2: does per-session encryption close the window?** No. To read offline after a restart, the key must be on the device, so encryption cannot stop an offline reader who holds the device. Encryption does help with sign-out, device loss after the session is revoked, and backups (S10). The window can only be bounded by a maximum offline age per entry, past which cached results are not shown. Neither Firestore nor PowerSync offers that bound built in, and it would be a real improvement over both.

**Check.** No revocation code exists. R7 shows policy reads are not recorded.

**Verdict: holds only within stated limits.** It holds for revocation by a database write that meets a recorded read set. It misses revocations made without such a write.

**Gaps.** G17, G18, G35.

## S7. Wire-version keying keeps unchanged endpoints' entries across an update

**Claim.** Entries are keyed by their endpoint's wire version (shape 4.12). After an update, entries of changed endpoints are dropped and the rest are kept.

**Best existing.** PowerSync supports multiple client versions with different output schemas. Electric rotates a shape and answers `must-refetch`.

**Attack.**

- A change in behaviour without a change in type keeps stale results with the old meaning. Examples are a new sort order, a different policy, or a bug fix in the query. Only a later write (S5) would refresh them. A deploy epoch per endpoint is needed (the same fix as G17).
- Keys are encoded arguments (R4). Any change to how arguments encode, such as a new optional field with a default, changes every key, so an app update silently empties the warm cache.
- What happens to queued writes for an endpoint whose wire version changed is left to #6558. The note should name that dependency.

**Verdict: holds only within stated limits.** It handles type changes. It misses changes in meaning and changes in argument encoding.

**Gaps.** G17, G19.

## S8. Collections: partitions, a resumable paged snapshot, then changes by version

**Claim.** An `app.collection` is partitioned by a key its policy reads. A device opens partitions. A partition's first load is "a snapshot at a version, sent in pages with a checkpoint", which resumes after an interruption. "Changes made while the snapshot streams are applied after it, by version." Losing access removes the partition.

**Best existing.** Electric joins a snapshot to a stream with Postgres snapshot metadata (`xmin`, `xmax`, `xip_list`) and an `up-to-date` marker. PowerSync applies a checkpoint atomically and shows no intermediate state. CouchDB resumes from a checkpointed sequence. Realm bootstraps a new subscription and removes objects that no longer match.

**Attack.** This is the snapshot-versus-live-stream problem the task names. The note states its goal, not its rule.

- **Are all pages read at the same version v?** If each page is a separate request after an interruption, the server must either hold one snapshot open across requests, or read later pages at later versions. Holding a snapshot open means a long transaction, which in Postgres holds back vacuum and expires with the exporting transaction. Reading at later versions makes the snapshot torn unless every change after the *smallest* page version is replayed idempotently. The note says neither.
- **Rows that move.** A row whose sort key changes during the snapshot can move from the unsent range into the sent range and be missed, or the reverse and be sent twice. A row whose partition key changes (a task moved between projects) must be a delete in one partition's stream and an insert in the other's. PowerSync models this as bucket REMOVE operations, and Electric names it move-in and move-out. The note does not cover it.
- **Exposure before completion.** The note does not say whether queries can see a partition before it is complete. PowerSync does not allow it, and Electric waits for `up-to-date`.
- **Partitions are coarser than policies.** A partition is "a key its server policy reads". A policy is per row: a private task inside a shared project is refused to some members. The partition stream must then be filtered per subscriber and per change, which breaks "snapshot plus changes by version" as a shared log. Otherwise the stream over-sends rows the policy forbids.
- **Resuming after the log expires.** An interrupted snapshot resumed after more than 30 days has no log back to v. The note's rule for keys ("re-read") is not stated for partitions.
- **Nothing to extend.** `app.sync` today is one process-wide session record with no partition, policy or log (R10), and `app.collection` does not exist.

**Check.** R10.

**Verdict: fails.** The consistency rule for joining the snapshot to the stream, move-in and move-out, hiding a partition until it is complete, and per-row authorization inside a partition are all unstated.

**Gaps.** G20, G21, G22.

## S9. A whole local-only app: mount the service in-process, and the screens are unchanged

**Claim (note 5.1).** A desktop or phone entry opens the database in the app's data directory and calls `.mount(crew)`. The screens still call `app.Remote<Crew>`. Live queries work in one process. Row policies run with a local account. On the web the database lives in OPFS or IndexedDB, with `navigator.storage.persist()`.

**Best existing.** PowerSync's local-only usage puts the same SQLite and the same queries behind a switch, with no server. Realm's local realms are configured separately from synced ones. Linear lets engineers switch sync off to prototype. RxDB and PouchDB run fully on the device.

**Attack.**

- **The real strength holds.** `app.remote(service)` runs in process today (R11), so "the screens do not change" is cheap and true at the call boundary.
- **Server-only features are not addressed.** The mounted service carries jobs (`#App.Job`, `app.enqueue`), schedules (`#App.Every`), mail, webhooks, push sends, `#App.SigningKey` and `app.random_key`, and account flows such as email confirmation. On a device, jobs have nowhere to run while the app is closed, mail cannot be sent, and webhooks cannot arrive. The note does not say which of these is a compile error, which runs locally and which is dropped.
- **Server code ends up in the client binary.** Mounting the service links server code, the database engine and secret-holding methods into the phone binary. The shape's CL1 and D-SECRETEDGE1 rest on server-only code never reaching a client. A local-only build needs its own rule for this.
- **`.serve()` may open a socket.** If `.serve()` on desktop starts the HTTP and WebSocket host (R9 shows the hub accepts any connection), a local-only app may listen on a port that other local processes, or a browser through DNS rebinding, can reach. The note must say that no listening socket opens, or that it is loopback-only behind a per-launch token.
- **Migration is missing.** A device database needs `db.migrate` on every app update, with a backup, rollback on failure and refusal to downgrade. The note says nothing about schema evolution on the device.
- **The web needs an engine nobody has shown.** A local-only web app needs Jet's database engine compiled for the browser over OPFS or IndexedDB. Nothing in Core shows this exists (R14). OPFS exclusive access handles (S4) block a second tab.

**Check.** R9, R11 and R14.

**Verdict: holds only within stated limits.** It holds for desktop and phone apps whose service uses no server-only features. Web and migration are unaddressed.

**Gaps.** G15, G23, G24.

## S10. `sync: .Off` on a collection: its documents stay on the device and are never pushed

**Claim (note 5.2, with section 5's definition "nothing is sent to a server").**

**Best existing.** PowerSync's `localOnly: true` tables are a schema-level marker, and writes to them never enter the upload queue. CouchDB's `_local` documents are never replicated. Realm keeps local and synced realms in separate configurations, so one cannot be synced by accident.

**Attack.** The task asks how local-only data could leak into sync. Paths the note leaves open:

1. **OS backups.** By default iOS backs up the app's Application Support directory to iCloud, and Android Auto Backup copies app data (up to 25 MB) to Google Drive unless the app opts out. A local-only database in `native.data_path(...)` therefore reaches a server by default. Section 5's "nothing is sent to a server" is false on phones unless the store is excluded from backup.
2. **Values copied into synced calls.** The type system does not mark local data. A draft's text passed as an argument to a synced `crew.new_task(...)`, a synced row that references a local document's ID, or a predicted write that captures local values all send local data. Some of this is intended, but nothing makes it explicit.
3. **Telemetry.** Shape 4.11 makes every endpoint call a span with the method and outcome, exported over OpenTelemetry. Arguments in spans or in crash reports leak local values.
4. **Invalidation events.** `live_invalidate` publishes the topic and, for keyed invalidations, the encoded key to the hub (R5, R9). In-process this reaches nobody, unless `.serve()` opened a socket (S9).
5. **A flipped flag.** One argument separates local from synced, and the default is not stated. A refactor that drops `sync: .Off` turns drafts into synced data with no compile error.

**Check.** R5 and R9.

**Verdict: fails as stated.** "Never pushed" holds for Jet's own sync channel. "Nothing is sent to a server" does not hold under default OS backup, and the note has no type-level marker that stops local data flowing into synced calls, logs or spans.

**Gaps.** G25, G26, G34.

## S11. Switching sync on later: local rows join a chosen account through the normal create methods

**Claim (note 5.3).** UUIDv7 IDs avoid renumbering. The person picks the account. Each row is sent as one call of its normal create method, in creation order, carrying the row's current value, so every policy runs. Refused rows stay local and are marked "Not synced".

**Best existing.** PowerSync documents the copy from a local-only table to a synced table in one transaction, with the row going through the upload queue. Its warning that converting a table in place deletes the table's data is the honest statement of the hazard.

**Attack.**

- **Creation order is not dependency order.** If row B references row A and A is refused, B is uploaded referring to a row the server does not have. Creation order by UUIDv7 time also depends on the device clock.
- **It is not resumable.** A crash halfway through needs a request key per row (#3709 c14). If a resend meets `Exists` for the device's own earlier upload, that must count as success, but `Exists` also means "someone else has this ID". The note does not separate the two.
- **Fields are lost silently.** A create method rarely accepts every field. Status, assignee, `created_at` and other server-set fields are dropped or replaced by the server's clock. "Carrying its current value" is true only for fields the create method takes. History is dropped, as the note says.
- **It does not scale.** One call per row for 10,000 rows has no batching.
- **Two devices, one account.** Two local-only devices that join one account each upload their own seeded rows (an "Inbox" project, for example), and duplicates follow. The note does not cover it.
- **The ending is undefined.** After the upload, the local database must become an O4 A cache. The note does not say what happens to the local database then.

**Verdict: holds only within stated limits.** It holds for a single device with independent rows whose fields the create method accepts.

**Gaps.** G29, G30.

## S12. Switching sync off for a synced app holds the queue

**Claim (note 5.4).** The device keeps reading its cache and queues writes. The queue is held until sync is on again, and sign-out asks before discarding held writes.

**Attack.** The note does not say whether reads still go to the server and pushes still arrive while sync is off. "Keeps reading its cache" suggests reads stay local even online, which is the opposite of the shape's live model. Held writes grow stale. Their rebased versions (#3709 c15) produce conflicts at scale when the queue is released, and the note gives no limit on hold time.

**Verdict: holds only within stated limits.** Holding the queue is well defined. The read behaviour is missing context.

**Gaps.** G31.

## S13. Sign-out and shared devices

**Claim (note 5.5).** Sign-out deletes the account's cached live results and partitions. Local-only data belongs to the device profile and is deleted only by an explicit "Remove local data".

**Attack.**

- **Mixed apps leak across accounts.** Section 5.2 allows a synced app with a local `drafts` collection. Under 5.5 the drafts belong to the device profile, not to the account. Person A writes drafts, signs out, and person B signs in on the same device and sees them. The local account of 5.1 is single, so the row policy cannot separate A from B.
- **The cache is not scoped to the account.** Endpoint keys carry no account (R4). A sign-out that deletes "the account's" entries cannot find them, and a second account can hit the first account's entries.
- **Deletion is not erasure.** Deleting rows in SQLite leaves bytes in free pages and in the WAL unless `secure_delete` is on and the WAL is checkpointed and truncated. Flash wear-levelling limits what any delete can promise.

**Check.** R4.

**Verdict: fails.** Device-profile data in a mixed app is visible to every account on the device, and the cache cannot be scoped by account as keyed today.

**Gaps.** G27, G28.

## S14. The failure table is complete

**Claim (note section 6).** Seven failures with defined behaviour.

**Attack.** The table has no row for:

- disk full or quota exceeded during a write, a snapshot or an upload;
- a corrupted device store;
- device clock skew (UUIDv7 ordering, and "Updated 2 h ago");
- server restore with versions going backwards;
- resuming a partition after its log has expired;
- a crash during the 5.3 upload;
- a failed migration of the device database;
- a second tab of a local-only web app (OPFS exclusive handles);
- Safari's seven-day eviction;
- an endpoint whose result grows past the cap in production;
- an invalidation broker outage that loses notifications (LISTEN/NOTIFY is not durable);
- the out-of-order results race and the pre-commit invalidation race (R6).

Tooling is missing as well. The device cache has no inspector (keys, bytes, versions, age, warm rank). The last catch-up has no report. The test host cannot simulate thirty days offline or a server restore. `live_stats` (`Core/App/App.jet:421-424`) counts queries and evictions but not bytes or versions.

**Verdict: fails.** The table omits failure modes that the design's own mechanisms create.

**Gaps.** G32, G33.

## The note's four questions, answered

1. **Is the warm set enough for "works offline" on a phone?** No. It is enough for enumerated screens. PowerSync and Linear give offline search and new filters over rows on the device. Zero's `preload()` is the closest equivalent, and even Zero serves other queries from the preloaded rows. See S3 and G12.
2. **Do Firestore and PowerSync have the same revocation window, and does per-session encryption close it?** Both have the window. Encryption does not close it for a device that stays offline, because the key must be on the device to read. Bound the window with a maximum offline age per entry (G18), and use encryption for sign-out, device loss and backups.
3. **Fixed budgets.** They are unmeasured. 64 MiB holds 16 results at the 4 MiB cap, and blob-per-result storage duplicates rows. See G14.
4. **Are `sync: .Off` and the in-process service one concept or two?** They are two mechanisms that answer one question: where the authority for a store lives. The review recommends one concept, an authority per store (`.Server` or `.Device`) applied the same way to a whole app, a collection or a table, with a type-level marker (G25, G34). That is how PowerSync (`localOnly` per table) and Realm (local versus synced configuration) present it.

## Verdicts

| Claim | Verdict |
|---|---|
| S1 Query-driven bootstrap; first paint independent of workspace size | holds only within stated limits (first screen, web) |
| S2 Bounded reads, cursor pages, per-page invalidation, limit after policy | fails |
| S3 Warm set for offline reads | holds only within stated limits (enumerated keys only) |
| S4 Budget, eviction, `QueueFull` | holds only within stated limits (unbuilt store, unmeasured numbers) |
| S5 Catch-up by version; "never a wrong value" | fails |
| S6 Revocation on next connection | holds only within stated limits (write-driven revocation only) |
| S7 Wire-version keying | holds only within stated limits (type changes only) |
| S8 Partition snapshot plus changes by version | fails |
| S9 Whole app local-only via in-process mount | holds only within stated limits (native, no server-only features) |
| S10 `sync: .Off` collection never leaves the device | fails as stated |
| S11 Switching sync on later | holds only within stated limits (one device, independent rows) |
| S12 Switching sync off for a synced app | holds only within stated limits (read behaviour unstated) |
| S13 Sign-out and shared devices | fails |
| S14 Failure table complete | fails |

Under AGENTS.md, a design with a failed claim or an open gap stays unbuilt until it is redesigned and reviewed again. #3709 c1 asks for "a reviewed design note". This review does not pass the note.

## Gaps, as proposed criterion text

Each row is written so that Main can file it on #3709, or on the named card, as a criterion.

| Gap | Proposed criterion text |
|---|---|
| G1 | The note names as unbuilt preconditions the criteria it relies on as if built: #3709 c14–c26, moved to #6867 (offline queue), #6868 (live-query scaling) and #6869 (row policies). Their proofs (`db/row_policy_judges_each_operation`, `web_app/offline_create_with_client_id`, `web_app/caller_dependent_live_query_is_not_shared`, `web_app/presence_counts_only_admitted_readers`, `web_app/refused_queued_write_without_a_screen_becomes_a_kept_notice`, `sync_two_clients` and the others those cards name) do not exist yet and stay open on #6867, #6868 and #6869. No part of the bootstrap design that depends on one of them is built before that criterion is met with its proof. |
| G2 | The bootstrap design states which parts depend on D-INVALIDATE1 read sets (#6554, review open) and #3709 c21, and states the behaviour and cost of per-page invalidation and catch-up under table-level invalidation, today's only mechanism (`Core/Db/Db.jet:842-852`). |
| G3 | Every committed write gets a version from one commit-ordered sequence (a counter advanced inside the write transaction on SQLite; the commit LSN, never a transaction ID, on Postgres); invalidations are published only after commit; every live result records the version of the snapshot it read. Proof: a case pauses a write between its statement and its commit, runs a re-read on another connection in that gap, and shows the device does not keep the pre-commit result as current after the commit. |
| G4 | The device stores a live result only when its version is at least the stored one. Proof: a case delivers a re-read at version v1 after a push at v2 > v1 and shows v2's result stays. |
| G5 | The server write log is specified and measured: what it stores (write sets and versions), on which backends, how several servers share it durably (LISTEN/NOTIFY alone is not durable), how a cached key's read set is recovered at catch-up (stored per key or re-derived by re-running), its bytes per day for Crewboard at 200,000 tasks, and the server cost of catching up a device holding 1,000 keys after 30 days. |
| G6 | Catch-up detects divergence: each key or partition carries a checksum the device verifies, and a mismatch re-reads. Proof: a case corrupts one cached entry and shows it is repaired on the next connection. |
| G7 | A list the device assembles from pages shows each row exactly once: either all cached pages of one list are re-read at one version when any is invalidated, or rows are merged by key. Proof: a case moves a row from page 3 to page 1 between page reads and shows it once. |
| G8 | A live result over `LIVE_MAX_PAYLOAD` fails as `Loadable.Failed` with a typed error naming the endpoint and the size; the server stops encoding at the cap; the result is not re-run on every `state` read (today it is, `Core/App/App.jet:404-417, 566-569`); `jet inspect choices` lists each live endpoint whose return type is a list with no limit parameter, by a rule the docs state; `jet inspect` warns when an endpoint's measured result size passes half the cap. |
| G9 | Bounded reads with "limit after policy" build only after #6827 c3 and row policies beyond `true` and `owner == user` (`Core/Db/Db.jet:311-323`) exist; the note names both as preconditions. |
| G10 | Each warm call's key equals a key some screen reads (same endpoint, same canonical encoded arguments); a warm call no screen makes is a compile warning naming the call; `jet inspect` lists the warm set with each entry's estimated bytes. |
| G11 | Warm reads are one-shot reads checked at reconnect, not server subscriptions, unless a mounted screen reads the same key; a `loop` warms through one batched request per loop; the warm order and the unmetered rule have a fallback where the platform reports no network type. Proof: a case warms 50 boards and reports requests sent and server re-runs per task write. |
| G12 | The docs state that offline search, new filters and any read whose key is not cached fail with "Not available offline", and compare this with PowerSync and Zero local queries; or a ballot offers a normalized device row store with local queries as an alternative to blob-per-result caching. |
| G13 | The device cache is a persistent store with per-entry reads and writes (not the process-wide registry cell that is decoded whole on each read, `Core/App/App.jet:486-552`); `LIVE_MAX_QUERIES` and the byte budget are one limit with one eviction order; eviction is least recently read (today it is first-in, first-out, `Core/App/App.jet:523-535`); an entry a mounted screen reads is never evicted. Proof: a case fills the budget while a screen is open and shows its entry survives. |
| G14 | Each device budget names the measurement it came from: Crewboard with 200,000 tasks, bytes per board, per warm set and per duplicated row across results, on desktop, phone and web. |
| G15 | The web store handles, by a case or a stated limit each: refusal of `navigator.storage.persist()`, Safari's seven-day eviction of script-writable storage for sites not installed, quota errors mid-write, and OPFS exclusive access handles with two tabs of one origin. |
| G16 | `QueueFull` is decided before the handler predicts, so the handler fails with the notice and nothing is predicted then rolled back; the queue's byte count includes queued uploads. Proof: a case fills the queue offline and shows the next write is refused without a predicted change. |
| G17 | A deploy that changes an endpoint's logic or a row policy it reads advances a per-endpoint epoch, and every cached key of that endpoint is answered "re-read" at the next connection; session revocation, account disabling and sign-out from another device delete the device's cached results for that account at the next connection. Proof: one case per path. |
| G18 | The entry can set a maximum offline age (for example `.offline(.Queue, max_offline_age: 14d)`), past which a cached result is shown as `Failed` with "Not available offline"; the docs state that per-session encryption does not close the offline window and use it for sign-out, device loss and backups; cached data uses the platform's file protection class. |
| G19 | Argument encoding for live keys is canonical and stable across app versions for endpoints whose wire version is unchanged. Proof: a case updates the app with an unrelated endpoint change and shows the warm cache is kept. |
| G20 | A partition load reads every page at one snapshot version v, or records each page's version and replays changes after the smallest one idempotently; the device exposes a partition to queries only once it holds a complete checkpoint; a row whose sort key or partition key changes during or after the snapshot appears exactly once, in the right partition (move-in and move-out operations); resuming after the log no longer reaches v restarts the snapshot. Proof: one case per rule, including a write committed while the snapshot streams. |
| G21 | A row inside a partition that the row policy refuses to a member is never sent to that member. Proof: a case with a private task in a shared project. |
| G22 | The WebSocket live hub sends an event only to connections with a subscription it affects, and only after the connection's caller passed the source's policy; no event carries another caller's encoded key. Today every connection receives every topic (`Compiler/JetRuntime/Source/WsRoutes.jet:470-602`). Proof: a two-account case shows no cross-account event. |
| G23 | A local-only entry that mounts a service states, as compile errors or local behaviour, what happens to jobs, schedules, mail, webhooks, push sends, signing keys and account flows; the D-SECRETEDGE1 cases cover a service mounted in a client build; `.serve()` on desktop or phone opens no listening socket, or only loopback with a per-launch token. Proof: a case per rule. |
| G24 | Device databases (local-only stores and caches) have a versioned schema; an app update runs `db.migrate` on the device with a backup first and rollback on failure; a downgrade is refused with a typed notice. Proof: a case migrates a local-only database across an update and survives a crash mid-migration. |
| G25 | Device-authority data has a type-level marker, and the compiler refuses passing it to an `app.Remote` call, a span or log attribute, or a sync push without an explicit conversion that names the destination; or the note states that this is unchecked and why. |
| G26 | Local-only stores are excluded from OS cloud backup by default (iOS `isExcludedFromBackup`, Android backup rules), with an entry option to include them; telemetry spans and crash reports carry no argument values from device-authority data. Proof: a case or a documented decision for each platform. |
| G27 | Device cache entries are keyed by account as well as endpoint and arguments; in an app that mixes synced data and device-authority collections, device-authority data is scoped to the person who created it, or the docs state that it is visible to every account on the device. Proof: a case signs in as A, writes a draft, switches to B, and shows the chosen behaviour, and shows B never reads A's cached results. |
| G28 | Sign-out and "Remove local data" delete with the store's secure delete (SQLite `secure_delete`, WAL checkpoint and truncate, or file deletion), and the docs state the limits on flash storage. |
| G29 | Switching sync on uploads rows in dependency order, references before referrers; each row carries a request key, and an upload is resumable after a crash; `Exists` for the device's own earlier upload counts as success and is distinguished from another account's ID; uploads are batched; fields the create method cannot carry are reported per row, never dropped silently. Proof: a case with 10,000 rows, dependent rows, and a crash halfway. |
| G30 | Two local-only devices joining one account have stated behaviour for duplicate seeded rows and for the same row edited on both. Proof: a case. |
| G31 | While sync is off for a synced app, the note states whether reads go to the server and pushes arrive, and the maximum hold time before held writes are flagged as stale. Proof: a case. |
| G32 | The failure table has a defined behaviour and a case for: disk full or quota exceeded mid-write, mid-snapshot and mid-upload; a corrupted device store; device clock skew; server restore with versions going backwards; partition resume after log expiry; a crash during the 5.3 upload; a failed device migration; a second tab of a local-only web app; Safari eviction; a result that grows past the cap; a broker outage that loses invalidations. |
| G33 | `jet inspect` or the devtools show each device cache entry's key, account, bytes, version, age and warm rank, and the result of the last catch-up; the test host can disconnect a device for a simulated duration and can restore the server to an earlier version; `live_stats` adds bytes and versions. |
| G34 | Owner choices 8.2 and 8.3 are replaced by one ballot: each store (whole app, collection or table) names its authority, `.Server` or `.Device`, with the same marker and the same rules for backup, telemetry and account scope. |
| G35 | The server sends a store identity and epoch with every version; a device whose cached versions come from another identity or a later epoch re-reads every key. Proof: a case restores the server from a backup and shows no device keeps a result newer than the server. |

## Sources

| Key | Source |
|---|---|
| ZERO-Q | [Zero: Queries (preload, TTLs, cached results, local consistency)](https://zero.rocicorp.dev/docs/queries) |
| ZERO-LLMS | [Zero documentation index (architecture summary)](https://zero.rocicorp.dev/llms.txt) |
| REP-CVR | [Replicache: backend strategies, row version (client view records)](https://doc.replicache.dev/strategies/row-version) [not opened in this review] |
| ELEC-HTTP | [Electric: HTTP API (shape log, up-to-date, must-refetch, snapshot-end metadata, proxy auth)](https://electric-sql.com/docs/api/http), now served at electric.ax/docs/sync/api/http |
| PS-CONS | [PowerSync: Consistency (checkpoints, causal+)](https://docs.powersync.com/architecture/consistency) |
| PS-LOCAL | [PowerSync: Local-only usage](https://docs.powersync.com/client-sdks/advanced/local-only-usage.md) |
| PS-LLMS | [PowerSync documentation index (Sync Streams, sync rules now legacy, bucket limits, prioritized sync)](https://docs.powersync.com/llms.txt) |
| COUCH-PROTO | [CouchDB Replication Protocol (checkpoints, replication ID includes filter code)](https://docs.couchdb.org/en/stable/replication/protocol.html) |
| YJS-SUB | [Yjs: subdocuments](https://docs.yjs.dev/api/subdocuments) [not opened in this review] |
| AM-SYNC | [Automerge: sync protocol](https://automerge.org/docs/under-the-hood/sync/) [not opened in this review] |
| FS-OFF | [Firestore: Access data offline (cache size, fromCache, multi-tab)](https://firebase.google.com/docs/firestore/manage-data/enable-offline) |
| REALM-FS | [Atlas Device Sync: Flexible Sync and client resets](https://www.mongodb.com/docs/atlas/app-services/sync/) [not opened in this review; the product is deprecated] |
| LINEAR-SCALE | [Linear: Scaling the Linear Sync Engine](https://linear.app/now/scaling-the-linear-sync-engine) (the note's URL redirects here; the page is a video with no text) |

The note's PowerSync link (`docs.powersync.com/usage/sync-rules`) points at what PowerSync now calls legacy Sync Rules, which Sync Streams replace. The redesign should cite Sync Streams.

Facts not checked against an opened source in this review are marked "not opened". These are Replicache CVR details, Yjs and Automerge protocol details, Realm client resets, Safari's seven-day storage eviction, the default OS backup behaviour on iOS and Android, OPFS exclusive access handles and Postgres LISTEN/NOTIFY limits. They come from the reviewer's knowledge of those systems, and the redesign should confirm each against its documentation.
