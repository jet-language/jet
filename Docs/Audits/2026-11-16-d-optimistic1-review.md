# D-OPTIMISTIC1 adversarial design review (2026-11-16)

Card #6555 c1. D-OPTIMISTIC1 was ratified A on 2026-09-28: "Automatic when
safe, opt out per action". An action is eligible "when every write goes to
data the client holds and it has no other effects. The client runs it at
once. The server result replaces the guess when it commits. If the server
rejects it, the guess is undone. `optimistic: false` opts one action out."
Its technical line says: "Eligibility comes from the action's effect row and
write set, the same sets D-INVALIDATE1 uses. The browser build contains the
action body." The web app shape (Docs/Proposals/2026-11-13-web-app-framework-shape.md)
carries the decision as §1 item 6, the §3.7 walk-through, the §4.6 subset rule,
§4.8 and claim CL4. The decision itself comes from
`memory-model-foundations-2026-09-27.md`, in the section "Using the language
level to replace stack layers".

This review attacks every strength the ballot and the shape claim. It sets
them against TanStack Query, React `useOptimistic`, React Router (Remix)
fetchers, Replicache, Zero and Meteor. It checks the claims against the code
on dev `6d616e7873` and against one probe. Each claim gets a verdict: holds,
holds within stated limits, or fails. A redesign follows (c5). Then the
redesign is reviewed again. Every gap goes to a card, a criterion or a
ballot.

## Measured facts

Code reads are against `6d616e7873`. Probe P1 ran `jet run -A main.jet`
(`jet a0.0.45-dev`, with `JET_TOOLCHAIN_ROOT` set to a checkout of
`6d616e7873`). Its output is quoted verbatim.

| # | Read or probe | Observed |
|---|---|---|
| R1 | Tower `D-OPTIMISTIC1` | As quoted above. The rationale: "The server action is the only code, so the instant update cannot drift from it. Actions with outside effects are never guessed." The trade-off: "A brief possible correction buys instant interfaces with no extra code." Its code uses `app.action("pay", pay, optimistic: false)` and shows `jet inspect choices` lines such as `add: optimistic (writes only cart.items)`. |
| R2 | Shape §4.6, lines 3152-3164 | "Prediction is a subset of the server's run. The client runs the method on its copy of the rows, but it skips what needs the server: the row policies; the version check; `check_assignee`; `app.enqueue`, and so every job, push and mail; the server's clock." Only typed edits of rows already on the client, and creates with a client-made ID, are predicted. |
| R3 | Shape lines 1286-1300, 1534-1570, 1575-1585 | `move_task` reads the row, compares `task.version != version`, and calls `can_move(self.role_in(task.project), ...)`. `role_in` reads `members` keyed by `app.caller()`. `check_assignee` is an ordinary private method that reads `members`. `after_assign` calls `app.enqueue`. `open_crew` attaches every row policy with `.guard(...)` and is called "Only [by] the server entry". |
| R4 | Shape lines 1195-1215, 653-668 | `board(project)` returns `Board{project: ProjectCard{.., open_tasks}, tasks, members, role}`, and `task(id)` returns `TaskPage`. These are results that embed `Task` and `Member` values next to derived values (`open_tasks`, `role`). |
| R5 | Shape, the Crew listing | There are 12 write methods a client can call: `create_task`, `import_tasks`, `update_task`, `move_task`, `attach` (`FS.Write`), `create_project` (makes its ID on the server with `app.new_id()`), `new_webhook_key` (`Rand`), `invite` (`Time`, enqueues), `accept_invite` (`Time`), `set_role`, `remove_member` (enqueues) and `leave` (calls `remove_member`). `import_tasks` makes IDs inside the method (`new_task_id()`, line 1258), and `new_task_id` is declared `fn() -> TaskID Never!` (line 732), with no effect. |
| R6 | `Core/App/App.jet:919`, `:1282` | `enqueue: fn(call: Int, job: &&CallJob) Never!` and `__enqueue(...) Never!` carry no effect. Nothing in a method's effect row shows that it queues a job. |
| R7 | `Core/App/App.jet:663` | `caller()` reads the acting account from task-local state, with no effect. |
| R8 | `Core/App/App.jet:2077-2098`, `:2207` | The client's live cache stores encoded results (`"ok:{encoded}"`) per binding, keyed by `live_key(args)`. There is no store of rows by table and key. |
| R9 | `Core/DB/DB.jet:991-1018`, `:1278` | `guard` takes a run-time policy value, `fn(T, Op, Who, Store) -[DB.Read]> Bool DBError!`. `get` is `-[DB.Read]>`, and `add` and `put` are `-[DB.Read, DB.Write]>`. `open_memory` exists. `core.db` is a Jet port of the SQLite engine: 45 source files, 2,349,525 bytes. |
| R10 | `git grep write_set\|read_set` in Compiler and Core; `Core/App/App.jet:546-566`; `Core/DB/DB.jet:971-973`; `Compiler/JetSema/Source/Sema/Footprint.jet` | The compiler has no static read or write set. At run time a live binding records each table its source reads (`live_reads.record`, a table's `core.db.live:<table>`), and a write re-runs the bindings that read that table. That is a set of tables, collected while the code runs, not a fact the compiler knows before the method runs. `Footprint.jet` solves each method's field footprint: the stored fields of `self` it reaches, sibling calls included (E0231). It does not split reads from writes and does not know rows. |
| R11 | `Core/Web/Store.jet:151-184` | Prediction today is manual: `optimistic(store, label, values, apply)` keeps a backup, and `patch_rollback` restores it. |
| R12 | `Core/Time/Time.jet:2622`, `Core/Crypto/Random.jet:19` | `now_utc -[Time]>` and `random.bytes -[Rand]>`: the clock and randomness are visible in effect rows. |
| R13 | Tower #6554, #6867, #3709 | D-INVALIDATE1 (#6554) is in planning, with c1 open. The durable queue and request keys (#6867) are open, except c8. |
| R14 | Shape, `optimistic` | The shape never spells an opt-out. Its remote methods are `#App.SignedIn` methods of a service, not `app.action(...)` registrations, so the ballot's `optimistic: false` has nothing to attach to. `jet inspect choices` (`Compiler/JetCLI/Source/CLI/InspectChoices.jet`) has no prediction kind. |
| P1 | `main.jet`, below | The same method run on a client copy and on the server store. `1 predicted: ok status=doing version=2`, `1 committed: ok status=doing version=2`, `1 same stamp: false`; `2 predicted: ok status=done version=3`, `2 committed: refused Storage(the table's row policy refused this write)`; `3 predicted: ok status=review version=3`, `3 committed: refused Conflict(3)`; `4 predicted: refused NotFound`, `4 committed: ok status=doing version=2`. |

P1:

```jet
// P1: one write method run twice, once on a client copy of the rows (the
// prediction) and once on the server's store, under D-OPTIMISTIC1's model.
use {
    db: core.db
    time: core.time
}

Task: struct {
    id: Int
    status: String
    version: Int
    stamped: Int
}

#Error
MoveError: enum {
    NotFound
    Conflict(Int)
    Storage(String)
}

impl db.DBError -> MoveError {
    MoveError.Storage(self.message())
}

tasks_of: fn(store: db.Store) -> db.Table<Int, Task> {
    store.table("tasks", key: fn(row: Task) { row.id })
}

// The server method, as written once.
move_task: fn(tasks: db.Table<Int, Task>, id: Int, version: Int, to: String) -[DB.Read, DB.Write, Time]> Task MoveError! {
    found: tasks.get(id) ?? return Err(MoveError.NotFound)
    if found.version != version -> return Err(MoveError.Conflict(found.version))
    saved: tasks.put(Task{id: found.id, status: to, version: found.version + 1, stamped: time.now_utc().to_timestamp()})
    Ok(saved)
}

show: fn(label: String, result: Task MoveError!) -> Int {
    if result == {
        .Ok(task) -> {
            print("{label}: ok status={task.status} version={task.version}")
            task.stamped
        }
        .Err(failure) -> {
            print("{label}: refused {failure}")
            0 - 1
        }
    }
}

run: fn() {
    server: db.open_memory()
    client: db.open_memory()
    seed: Task{id: 1, status: "todo", version: 1, stamped: 0}
    _: tasks_of(server).put(seed) ?? panic("seed server")
    _: tasks_of(client).put(seed) ?? panic("seed client")

    // 1. A pure edit: the prediction and the server agree, but the stamp
    // each run reads from its own clock differs.
    predicted: show("1 predicted", move_task(tasks_of(client), 1, 1, "doing"))
    // The call reaches the server a second and a half later.
    time.sleep(1500ms)
    committed: show("1 committed", move_task(tasks_of(server), 1, 1, "doing"))
    print("1 same stamp: {predicted == committed}")

    // 2. A row policy the client copy does not run: the server refuses a
    // move to done, the prediction shows it.
    guarded: tasks_of(server).guard(fn(row: Task, op: db.Op, who: db.Who, store: db.Store) { Ok(!(op == .Update && row.status == "done")) })
    _: show("2 predicted", move_task(tasks_of(client), 1, 2, "done"))
    _: show("2 committed", move_task(guarded, 1, 2, "done"))

    // The server's answer replaces the prediction.
    _: tasks_of(client).put(Task{id: 1, status: "doing", version: 2, stamped: 0}) ?? panic("reconcile")

    // 3. Another writer moved the row first: the prediction succeeds on
    // the stale copy, the server answers Conflict.
    _: tasks_of(server).put(Task{id: 1, status: "review", version: 3, stamped: 0}) ?? panic("other writer")
    _: show("3 predicted", move_task(tasks_of(client), 1, 2, "review"))
    _: show("3 committed", move_task(tasks_of(server), 1, 2, "review"))

    // 4. A row the client does not hold: the client copy answers NotFound,
    // a refusal the server never gives.
    _: tasks_of(server).put(Task{id: 2, status: "todo", version: 1, stamped: 0}) ?? panic("server only row")
    _: show("4 predicted", move_task(tasks_of(client), 2, 1, "doing"))
    _: show("4 committed", move_task(tasks_of(server), 2, 1, "doing"))
}
```

## The strongest existing models

- **TanStack Query.** `onMutate` cancels in-flight refetches of the keys it
  touches, so a stale response cannot overwrite the guess. It snapshots the
  cache, writes the guess, and returns the snapshot for `onError` to restore;
  `onSettled` invalidates and refetches. The lighter form renders
  `variables` while `isPending` and needs no rollback. `useMutationState`
  lists concurrent pending mutations. `scope.id` runs mutations in one
  scope in order. Paused offline mutations persist and resume in order,
  provided `setMutationDefaults` names a default `mutationFn`. The cost is
  that the guess is hand-written next to the server code, which is the drift
  the ballot names. Sources:
  tanstack.com/query/latest/docs/framework/react/guides/optimistic-updates,
  .../guides/mutations.
- **React `useOptimistic`.** The optimistic state exists only while an
  Action is pending. If the base value changes during the Action, the
  reducer runs again on the new base, so the guess stays on top of fresh
  data. When the Action ends, the real value renders in the same commit,
  and a failed Action leaves the old value, so a revert needs no code. The
  reducer must be pure. The guess is still hand-written, and it lives in one
  component. Source: react.dev/reference/react/useOptimistic.
- **React Router (Remix) fetchers.** `fetcher.formData` is the pending
  submission, and the UI derives the guess from it. After an action, every
  loader revalidates by itself. Concurrent fetchers commit each
  revalidation as it lands, and drop a revalidation that started earlier
  than one already committed. The docs name the rare stale case where a
  cancelled request still reaches the server, and suggest submission
  timestamps. There is no offline queue. Sources:
  reactrouter.com/how-to/fetchers, reactrouter.com/explanation/concurrency.
- **Replicache.** Mutators run on the client against a persistent
  key-value Client View and again on the server, which is authoritative.
  The server need not compute the same result. Each pending mutation gets a
  per-client sequential mutation ID, is persisted, and is pushed in order.
  A pull returns a patch and `lastMutationIDChanges`. The client rewinds to
  the last server state, applies the patch, replays the mutations still
  pending, and reveals the result atomically. Conflict resolution is the
  mutator's own code, run again during rebase. Source:
  doc.replicache.dev/concepts/how-it-works.
- **Zero.** A mutator runs on the client against rows already synced and
  then on the server. The two implementations "don't have to be the same,
  or even compute the same result". Server-only work branches on
  `tx.location`. Client reads "only return data already cached on the
  client". Credentials come from a server `context` (`ctx.userID`), not from
  arguments. Client-made random IDs are recommended, and server-assigned
  values (`shortID`) come back only from the server run. Mail goes through
  an outbox table. Source: zero.rocicorp.dev/docs/mutators.
- **Meteor.** A method can have a client stub that runs in parallel with
  the server call against the client's document cache (Minimongo).
  `this.isSimulation` tells the stub where it is. Meteor tracks each
  method's writes and holds the callback until the server's writes replace
  the stub's. Source: docs.meteor.com/api/meteor.html (Meteor.methods,
  Meteor.call).

All six run the guess against data the client holds as data. In the three
sync engines (Replicache, Zero, Meteor) that data is rows or documents, not
query results, and the client reruns queries over it.

## Claims and attacks

### 1. "The server action is the only code, so the instant update cannot drift from it" (R1; CL4)

**Fails as written.** The shape's subset rule (R2) makes the client run a
different program from the server. That program has the version check and
`check_assignee` removed. The `if task.version != version` line (R3) is user
code, not a framework step, so dropping it means the compiler rewrites the
method body. `check_assignee` is named by its method name, so the rule is a
list of one app's calls, not a rule. The next app's
`check_quota` is not on the list. Each skipped line is a place where the
guess and the server differ, which is the drift the ballot set out to
remove. Replicache and Zero are honest about this: the server program may
differ, and the code says so (`tx.location`). Here the skipping happens
implicitly, through a list in a document.

### 2. "Eligibility comes from the effect row and write set, the same sets D-INVALIDATE1 uses" (R1)

**Fails today.** The compiler has no write set (R10). The run-time read set
names tables, and only after a source has run, so it cannot decide before a
call whether that call may be predicted. The nearest compile-time fact is the
field footprint, which names the `self` fields a method touches without
saying which it writes or which rows. The static sets the ballot leans on
are #6554's work, which is still in planning (R13). The effect row cannot
carry the decision by itself either:

- `app.enqueue` is `Never!` with no effect (R6), so "has no other
  effects" cannot see a queued job.
- `new_task_id` is declared with no effect (R5), yet a UUIDv7 needs a clock
  and randomness. So `import_tasks`, which makes IDs inside the method,
  looks pure. Its prediction would mint IDs that differ from the server's,
  and every predicted task would be replaced by a task with another ID.
- `app.caller()` has no effect (R7). That is fine in itself, since the
  client knows its own session, but it shows that "pure over client data"
  is a property of calls the compiler must know by name, not of the row.

### 3. "Prediction is a subset of the server's run" (§4.6; CL4 "matches Meteor and Zero")

Each skipped item, attacked on its own:

- **Row policies: holds within stated limits.** Policies are run-time
  values attached in `open_crew`, which runs only on the server (R3, R9),
  so the client cannot run them. P1 case 2 shows the result: the prediction
  says `done`, and the server refuses. Zero and Meteor have the same limit,
  and the shape states it ("a predicted card can be refused and move back").
  What is missing is any measure of how often this happens (claim 13).
- **The version check: fails.** Skipping it removes the one check the
  client can run correctly. P1 case 3 shows a stale copy predicting success
  where the server says `Conflict`. When the client's copy is already newer
  than the version the handler passes (a live push arrived after the
  screen read the row), the check on the client finds the conflict at once.
  Skipping it guarantees a wrong guess there. The §3.7 rebase story also
  needs the check: a second queued edit passes its own version, and the
  client can check it against the predicted row.
- **`check_assignee`: fails.** It reads `members`, which `board` already
  sends (R4). Nothing about it needs the server. Naming it is the
  enumeration problem of claim 1.
- **`app.enqueue`: holds within stated limits.** A queued job runs on the
  server after the commit, and its effects reach the client only through
  live results. Skipping it on the client loses nothing the client could
  show. It does widen the ballot, though, which says eligible actions have
  "no other effects", and the ballot's own example refuses `pay` because of
  its `Net` effect. A method that queues a mail is an action with an
  outside effect in the plain sense. The widening needs the owner's
  confirmation (see Verdict).
- **The server's clock: fails.** A method that reads the clock has `Time`
  in its effect row (R12), so the ballot makes it ineligible. The shape
  instead predicts it with the client's clock. P1 case 1 shows that the
  stamp differs (`1 same stamp: false`). The shape and the ballot disagree,
  and no Crewboard write that the shape predicts reads the clock
  (`invite` and `accept_invite` are excluded for other reasons, R5). The
  line buys nothing and contradicts the ratified text.

The "matches Meteor and Zero" verdict (§4.6) holds for running one body in
two places. It fails for the data that body runs on (claim 4).

### 4. "A typed edit of rows the client already holds is predicted" (§1 item 6, §4.6)

**Fails.** The client holds no rows (R8). It holds encoded results per
binding: a `Board` embeds `Task` and `Member` values next to derived values
(R4). For `self.tasks.get(id)` to run on the client, something must build a
table of tasks from those results. Nothing says which result values are
stored rows and which are values a query computed, nor how two results that
hold the same task stay one row. P1 case 4 shows the failure when a row is
missing: the client copy answers `NotFound`, a refusal the server never
gives. A missing row on the client means "unknown", but the method body reads
it as "absent". The flagship move also reads the caller's `Member` row
through `role_in` (R3). That row arrives only inside `Board.members` and
`TaskPage.members`, so a move made from another screen (search results,
`mine`) has no role to check.

Replicache, Zero and Meteor all keep a normalized row or document store for
exactly this reason, and Zero states the read rule: "ZQL reads only return
data already cached on the client".

Derived values have the same problem. After a predicted move to Done,
`ProjectCard.open_tasks` in `my_projects` and in `Board.project` is stale
until the server pushes. The board can be recomputed only by running
`board` again on the client, which needs every task of the project. The
`search` results cannot be recomputed at all: full-text search runs in the
SQLite engine, which the client does not carry (R9).

### 5. "A create with a client-made ID is predicted; a predicted timestamp is replaced by the server's" (§4.6, §4.8, #6555 c7)

**Holds within stated limits.** An ID passed as an argument (`create_task`)
gives the predicted row its final identity, as Zero recommends, and a link
to it works offline (§4.8). The limits:

- An ID made inside the method (`import_tasks`, `create_project`) cannot be
  predicted, and the effect row does not show it (claim 2).
- A server-assigned value such as a counter or a short ID (Zero's
  `shortID`) has no place in the shape. Today Crewboard has none, so this is
  a future limit, not a defect.
- Timestamps: claim 3.

### 6. "The server result replaces the guess; a refusal undoes it, with its notice" (R1; §3.7 steps 7 and 9; §4.8)

**Holds within stated limits.** Replacing the guess with the committed row
is the right rule. P1 shows the server's answer is the one that counts in
all four cases. Rollback is visible: the card moves back and the notice
appears beside it, or in the app's notice list when it has no screen
(§3.7 step 10, #6445 c7). TanStack's `variables` form and React's
`useOptimistic` revert with no code, and so does this design. The limits:

- The replacement is defined for the returned value, not for every live
  result that showed the guess. The board, the project card and the task
  page each hold their own copy (claim 4), and the shape does not say how a
  guess reaches all of them, or how it leaves all of them.
- Meteor holds the callback "until all of the server's writes replace the
  stub's writes". The shape has no matching rule tying the end of
  "Waiting to send" to the push that carries the commit, rather than to
  the reply.

### 7. Concurrent and reordered mutations (§3.7 step 3, §4.8 "Rebase per device")

**Fails.** The shape covers two edits of one row from one device: the
second queues behind the first. It does not cover a live push that arrives
while a guess is pending. With result-level storage the push replaces the
`Board` and erases the guess, so the card jumps back and then forward
again when the reply lands. TanStack's `onMutate` cancels refetches for
exactly this reason. React reruns the reducer on the new base. Replicache
rewinds, applies the patch, and replays what is still pending. The shape
does none of these. It also does not say how the client learns that a
push already contains its own write, which is Replicache's
`lastMutationIDChanges`. Without that, replaying could apply the write
twice, and not replaying flickers. Two tabs share one queue (§4.8, Web
Locks), but they do not share guesses: the second tab sees the edit only
when the push arrives. That is acceptable, but it is not stated.

### 8. Offline queues (§4.8, §3.7)

**Holds within stated limits.** The queue design (request keys, ordered
send, sign-in again when the session expires, nothing dropped in silence)
meets the TanStack and Replicache bar on paper. None of it is built
(#6867, R13). TanStack needs a default `mutationFn` to resume after a
reload. Here the call is method plus codable arguments (#6445 c8), so a
resumed call needs no closure, and that part goes further. After a
restart, though, the guesses must be rebuilt by replaying the queue on the
stored results, and that hits claims 4 and 7 again. The web build sets no
queue, and IndexedDB storage is still to be built (#6867 c1).

### 9. Partial failure

**Holds within stated limits.** A predicted call runs as one client
transaction, and a server method is one serializable transaction (§9). So
either every write shows or none does, and a refusal undoes all of them.
The one exception is `import_tasks`, which fails claim 2's ID problem
before partial failure arises. A lost race retries the whole method on the
server (§9), which is invisible to the client because the guess waits for
one answer. A queued job that fails later is not a failure of the write, and
it reaches the notice list (CL7).

### 10. Secrets and server-only effects inside the function (R1 "the browser build contains the action body")

**Fails as written.** Shipping the body ships its logic: `can_move`'s role
rules and every check in an eligible method become readable in the client
bundle. The ballot does not mention this cost, and §9 counts only bytes.
Nothing stops an eligible body from reading a secret through a call the
effect row does not show. Today `webhook_secret` is a `#App.SigningKey`
method returning `#Secret String?`, and nothing marks a caller of it
ineligible. The secret edge is D-SECRETEDGE1 (#6557), which is not built
(CL5). Policies stay on the server (R3), which is good. Zero handles the
same problem with `tx.location` branches and a server `context`; Meteor
puts server-only code in server files.

### 11. "`optimistic: false` opts one action out"; choose, explain, pin (R1, #6555 c3)

**Fails.** The shape has no spelling for the opt-out (R14). The ballot's
spelling belongs to an `app.action(...)` registration that the shape
replaced with marked service methods. `jet inspect choices` has no
prediction kind, and nothing exists to pin a method as predicted so that
the build fails when it stops qualifying.

### 12. "Instant interfaces with no extra code"; "a method that is not eligible costs nothing" (R1, §9)

**Holds within stated limits.** No extra code is true for eligible methods.
The second sentence holds for bytes. But the client must also carry the
code that runs the bodies: a client table with the `get`, `put`, `add` and
`query` surface. Today that surface is the SQLite port (R9, 2.3 MB of
source), which no web budget (#6556) will take. So the cost of prediction
is a second, client-side table engine, not just the method bodies.

### 13. The measure §9 assigns: the share of Crewboard's writes that are eligible, and wrong guesses per thousand

**Measured in part.** Of the 12 client-callable write methods (R5):

- Under the ballot as ratified (only client data written, no other
  effects), 2 are eligible: `move_task`, and `set_role`. `set_role` qualifies
  only while the client holds every member row that `keep_an_owner`
  counts.
- Under the shape's subset rule, 4 are eligible: those two, plus
  `create_task` and `update_task`. If a delete counts as a "typed edit",
  `remove_member` and `leave` make it 6. The shape does not say.

Wrong guesses per thousand cannot be measured: no prediction is built, and
no counter is specified. P1 shows three ways to guess wrong (a policy, a
stale row, a missing row). That list is not a rate.

## Verdict

**Fails.** Claims 1, 2, 4, 7, 10 and 11 fail. Claims 3 and 13 are mixed
(3: policies and enqueue hold within limits; the version-check, assignee
and clock items fail). Claims 5, 6, 8, 9 and 12 hold within stated limits.
The direction the ballot authorizes stands. As written, the design must not
be built or presented to the owner. The redesign below goes back through
review.

Gaps and where they go:

| Gap | Where |
|---|---|
| A client row view built from what the server read, with "not held" distinct from "absent" (claims 4, 6, 7) | needs a card under #6555 (new), depends on #3709 live queries |
| Read and write sets per method at table and key level (claim 2) | #6554 (D-INVALIDATE1) c1 |
| `app.new_id` and `new_task_id` carry their effects (claims 2, 5) | #3709 c23 (client IDs) |
| A push that names the request keys it applied (claim 7) | #6867 (queue and request keys) |
| Eligibility refuses a body that reaches a `#Secret`, signing-key, webhook or job method; the shipped bodies are listed (claim 10) | #6557 (D-SECRETEDGE1), #6555 c3 |
| A spelling for the opt-out and the pin on a marked method (claim 11) | ballot needed (syntax registry, I7) |
| A prediction kind in `jet inspect choices` (claim 11) | #6555 c3 |
| Wrong-guess counters in devtools (claim 13) | needs a criterion on #6555 |
| Owner confirmation that queuing a job does not make a method ineligible (claim 3) | owner, through Main |
| A client table engine within the web budget (claim 12) | #6556 |

## Redesign

The redesign keeps the ballot's rule: one body, the server's answer wins,
and an opt-out. It changes what the client runs the body on and what it
skips.

**D1. A client row view, filled from what the server read.**

- Each live result arrives with the typed-table rows its query read after
  the policies ran: the table, the key, and the row.
- The client keeps those rows in an in-memory view per table, keyed by the
  table's key function and shared by every result.
- A result's row-typed values are references into that view, so one task
  shown on three screens is one row.
- The view implements the typed-table surface the method bodies call:
  `get`, `add`, `put`, `delete`, and `query().filter/count/collect` over
  held rows. It is not the SQLite engine. Full-text `search` has no client
  form.

**D2. A read the view cannot answer stops the guess; it is never a
refusal.**

- `get` of a key the view does not hold ends the prediction as "cannot
  predict", and the call waits for the server with the busy state.
- A `query()` is answered only when the view holds the complete answer:
  the same table, filter closure and captured values as a scan the server
  recorded for a live result. Captured values are codable, by #6445 c8.
  Any other scan stops the guess.
- A guess that ends in `Err` (a check failed on the client's copy) is not
  shown as a refusal. The call goes to the server unpredicted, and the
  server's answer decides. The client never invents a refusal (P1 case
  4). It also never shows success for an edit it already knows is stale
  (P1 case 3, when the view is newer).

**D3. The client runs the whole body.**

- Every user-written line runs, the version check and `check_assignee`
  included. The shape's list goes away.
- Two things are skipped, both compiler-known:
  - row policies, which are attached only in server code and so are never
    in the client view;
  - `app.enqueue`, whose job runs on the server after the commit.
- `app.caller()` is the client's session account.

**D4. Eligibility from facts the compiler already has.**

- A remote write method is eligible when its effect row is within
  `DB.Read, DB.Write`. `Time`, `Rand`, `Net` and `FS` make it ineligible, as
  the ballot says.
- `app.new_id()` is `-[Rand, Time]>`. A method that makes an ID inside
  itself is therefore ineligible, while a create whose ID comes as an
  argument stays eligible.
- The body must not reach, through its calls, a `#Secret` value, an
  `#App.SigningKey`, an `#App.Webhook` or an `#App.Job` method, or a
  foreign call.
- The tables it touches come from the field footprint (`Footprint.jet`)
  until #6554's read and write sets exist. A method is not refused because
  the client might not hold a row; D2 handles that at run time.

**D5. Pending guesses are a layer, replayed over every push.**

- The client shows the server rows with the pending calls replayed on
  top, in queue order. This is Replicache's rewind and replay, over the D1
  view.
- Each push names the request keys whose commits it contains. A pending
  call leaves the layer when a push names its key, not when its reply
  arrives. So a push that lands first does not erase the guess, and a
  reply that lands first does not drop the "Waiting to send" mark before
  the screen shows the commit.
- Live results whose scans the view holds completely (D2) are recomputed
  on the client after each replay, so `Board.project.open_tasks` moves with
  the card. A result the client cannot recompute (`search`) keeps its
  server copy until the push.

**D6. Choose, explain, pin.**

- `jet inspect choices` gets a Prediction kind. Each remote write method
  is listed as predicted or waits, with the fact that decided it: an
  effect, a call to a server-only method, or an ID made inside. It also
  lists the bodies that ship to the client.
- An opt-out and a pin on the marked method, spelled by the ballot below.
  The pin fails the build when the method stops being eligible.

**D7. Measured.**

- Devtools count the guesses made, the guesses that matched the committed
  row, and the guesses that differed, by reason: policy, conflict, cannot
  predict.
- The Crewboard test run reports wrong guesses per thousand.
- `jet inspect choices` gives the eligible share.

## Re-review

The claims, taken again against D1-D7:

1. **Drift: removed.** The client runs the same body. The only skips are
   compiler-known (policies, enqueue), and inspect lists them.
2. **Eligibility from facts: holds within stated limits.** It uses the effect
   row, compiler-known calls and the field footprint, all of which exist.
   Row-level sets are not needed for eligibility. With `app.new_id` taking
   its effects, `import_tasks` and `create_project` correctly wait. The
   limit is that D4 depends on `app.new_id` declaring `Rand, Time`
   (#3709 c23).
3. **The subset.**
   - Version check, assignee check and clock: removed, by D3 and D4.
   - Policies: a named cost. A guess the server's policy refuses shows
     and then moves back, with its notice. The mitigations: the refusal
     notice, the counter of D7, and the opt-out for a method whose
     policies often refuse.
   - Enqueue: holds, pending the owner's confirmation of the widening.
4. **Rows: removed** by D1 and D2. "Not held" is not "absent", and P1 case
   4 becomes a wait. Named cost: each push carries the rows its query read.
   Rows are de-duplicated by key across results, so a board sends each task
   once.
5. **Creates: holds within stated limits.** IDs made by the client are
   predicted. Values assigned on the server wait for the commit.
6. **Replace and undo: removed.** One row in the view feeds every screen
   that shows it. The mark clears when the push that contains the commit
   arrives (D5).
7. **Concurrency: removed.** The layer is replayed over each push, and
   applied keys end each guess (D5). Named cost: the replay runs once per
   push for each pending call, bounded by the queue length. Calls that touch
   none of the pushed rows are not replayed.
8. **Offline: holds within stated limits.** After a restart, the queue
   replays on the stored view. This waits on #6867, which builds the
   durable queue and its web storage.
9. **Partial failure: holds.** The same all-or-nothing rule, now over one
   view.
10. **Secrets and disclosure.** Secret reads are removed by D4's
    reachability rule; the full guarantee waits on #6557. Disclosure of
    business logic is a named cost: an eligible body is readable in the
    client. The mitigations: inspect lists every shipped body (D6), and
    the opt-out keeps a method's logic on the server.
11. **Opt-out, explain, pin: removed** by D6, once the spelling ballot is
    ratified.
12. **Cost: a named cost.** The client carries the D1 view (a typed-table
    surface over in-memory rows, no SQL engine), the eligible bodies, and
    the bodies of live queries it recomputes. Mitigations: the #6556 budget
    decides; a method that is not eligible ships nothing; and a live query
    whose scans are never complete on the client ships nothing.
13. **Measures: removed** by D7, which makes both measures reportable.
    Under D3 and D4, Crewboard's eligible share is 6 of 12: `move_task`,
    `update_task`, `create_task`, `set_role`, `remove_member` and `leave`.
    The last two are deletes, which the D1 view supports. The decision on
    enqueue is the owner's. If the owner rules that queuing a job is an
    outside effect, the share is 2 of 12 (`move_task` and `set_role`).

**The redesign passes the review within these named costs:**

- policy refusals the client cannot foresee;
- rows sent with each push;
- the replay per push;
- logic disclosure for eligible methods;
- the client code for the view.

Each cost has a mitigation above. Before implementation, it still needs:

- the owner's confirmation on enqueue;
- the opt-out and pin spelling ballot;
- the cards and criteria in the gap table.
