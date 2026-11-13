# Web and app framework shape: business logic once, on the web, the desktop and phones (2026-11-13, revision 5)

**Status: proposed, not built.** This document proposes the shape that ballot
**D-WEBAPP-SHAPE1** would approve. No program in it builds into a working app
today, because the `app` runtime, the `kit` views and the `native` plugins do
not exist yet. Every Jet program here is a real file, checked by the seed
compiler; Appendix A says how. Revision 5 answers the eight gaps of the
round-four review: a policy's reads see the transaction's earlier writes;
the creator rule is tied to the project row; the members policy enforces an
invitation's expiry; each push and mail a job sends has its own key; an
upload's policy check runs before the body is read and spools only after
it; the text names which policies join; a replayed edit keeps its version;
and a failed background run reaches the app's notice list (section 0).
Revision 4 answered the 28 items of round three.

- Card: #6792. The proposal answers criteria c1 and c2, c6 (native and web
  needs), c7 (styling) and c8 (Crewboard as a probe, with a comparison
  matrix). c3 is the adversarial review. Round one failed C3, C5 and C6;
  round two failed C3 and c8 and found c7 partly met; round three failed C3
  and found c8 partly met; round four failed C3 narrowly, on eight gaps.
  Section 0 answers each round-four gap, then each earlier item.
- Base: dev `2fcd2d5247`.
- Builds on these ratified decisions: D-ONEAPP-MODEL1, D-ONEAPP-RENDER1,
  D-ONEAPP-WEBOUT1, D-VIEW-SET1, D-PORTABLE-WEB1, D-ROUTE-ONE1,
  D-WEB-RUNTIME-API1 (=A: `app` stays in Core, `core.web.browser` becomes
  `core.browser`), D-CONTEXT-EFFECT1 (=F: context is an implicit input, not
  an effect), D-THEME2, D-LAYOUT2, D-I18N1, D-A11Y-EXPORT1, D-FRAME-THEME1,
  D-FRAME-STYLE1, D-PLATFORM-OVERRIDE1, D-ONE-LIFECYCLE1, D-UI-WINDOW-LIFE1,
  D-NATIVEUI3, D-SECRET-CT1, D-SYNC2 and D-VALIDATE1.
- Uses the designs of the owner-paused cards #6553–#6558: D-REACT-REF1,
  D-INVALIDATE1, D-OPTIMISTIC1, D-RESUME1, D-SECRETEDGE1 and D-WIRESKEW1.
  Those cards stay paused. This proposal uses their ratified direction. It does
  not claim their advantages hold; section 9 names what each must measure.
- Still open and not decided here: D-HTTP-HANDLER-ENTRY1 decides how the `app`
  package reaches an HTTP port. Every option it offers leaves the programs
  below unchanged.

## Contents

- [How to read the code](#how-to-read-the-code)
- [0. What changed after the round-four review](#0-what-changed-after-the-round-four-review)
- [1. The shape in one page](#1-the-shape-in-one-page)
- [2. Today's code versus the shape](#2-todays-code-versus-the-shape)
- [3. Crewboard: the business app, used as a probe](#3-crewboard-the-business-app-used-as-a-probe)
- [4. The areas, one at a time](#4-the-areas-one-at-a-time)
- [5. What a native app and a web app need (card c6)](#5-what-a-native-app-and-a-web-app-need-card-c6)
- [6. Styling (card c7)](#6-styling-card-c7)
- [7. Open choices, and the ballot](#7-open-choices-and-the-ballot)
- [8. How the shape compares (card c8)](#8-how-the-shape-compares-card-c8)
- [9. Costs and what is not built](#9-costs-and-what-is-not-built)
- [10. The proposed surface](#10-the-proposed-surface)
- [11. Sources](#11-sources)
- [Appendix A. How the code was checked](#appendix-a-how-the-code-was-checked)

## How to read the code

There are two kinds of code block.

1. **Jet blocks.** Each one is copied from a file under `crewboard/` or
   `examples/`. The files were checked with the seed compiler (seed-b15,
   `jet check`) against stub packages that declare the proposed surface:
   `app`, `ui`, `kit`, `native`, `auth` and a typed-table `db`. The stubs are
   the proposal for that surface (section 10). Each file's header says
   which card would build what it uses. Appendix A lists every file with its
   result. One example keeps a known error, and the error is a gap with a
   card (A.4). The seeded style bugs of section 6.5 and one claim probe fail
   on purpose.
2. **Other blocks.** HTML, CSS, Tailwind, SwiftUI, Flutter and shell blocks
   show other systems or commands. They are not checked. Option blocks in
   section 7 that are not the recommended form are marked "not checked" with
   their card.

Every name the proposal adds is listed in section 10. Each one is
owner-gated, and the ballot approves them together with the shape.

## 0. What changed after the round-four review

### 0.1 Round four

Round four (`Docs/audits/2026-11-13-web-app-framework-shape-review.md`,
R4-5) found 23 of the 28 round-three items closed and listed eight gaps.
This revision closes each one in the files and the text. Each criterion it
needs is in 0.2.

| # | Gap | Revision 5 | Where |
|---|---|---|---|
| 1 | What a policy's reads see | They see the transaction's earlier writes but not the write being judged, as Postgres evaluates `WITH CHECK`. `create_project`'s member insert sees the project row the same transaction inserted; a write cannot vouch for itself. | 3.2, 4.2, 10 (`db.Op`) |
| 2 | The creator rule was not tied to the project | `Project` gains `created_by`. The projects policy admits an insert only when `created_by` is the caller, and an update only when it keeps `created_by`. `first_owner` admits an Owner insert only when the project row exists, names the caller as its creator, and has no members. Test: "an outsider can't claim a project with no members". | 3.2 |
| 3 | Invitation expiry outside the policy (item 4) | `invited` checks `expires >= time.today()`, with `Time` in the members policy's effects, so the policy refuses an insert from a lapsed invitation; `accept_invite` keeps its own check to answer `NotFound`. | 3.2 |
| 4 | One key for every push of a run (item 11) | Each push and mail has its own key, made from the run's key (a queued call's, or a schedule's job name and due time), the recipient and the `open:` or `link:` value. `remind_due_today`'s pushes about two tasks stay two, and a re-run still replaces each. A key made from the send's position would shift when a re-run's query returns other rows. | 3.2, 4.14, 10 |
| 5 | Uploads spooled before the policy (item 12) | `attach` is marked `#App.UploadCheck(method: "may_attach")`. The framework runs `may_attach` as the caller after the session check and the size cap and before it reads the body (403, body never read), as #6813 c3 requires; only then does it spool, before the transaction, as c5 requires, and the transaction checks again. A staged file lives no longer than its request, failed or not; one left by a crash is deleted at the next start. Test: "an outsider's upload is refused before it is read". | 3.2, 5.2.3, 10 |
| 6 | Which policies join (item 13) | The task, attachment and project policies and the members policy's read are key lookups and join. The invites policy (`auth.verified_email`) and the members policy's insert (a count and an invitation read) run per row, memoized. | 3.2, 4.2 |
| 7 | A replayed rebased edit (item 15) | It keeps the row version it was made with, so after Ben's change it gets `Conflict` and its own notice. | 3.7 |
| 8 | The batch path (item 9), nested calls, the background run (R4-3), A.3 | Decoding a list runs each element's `validate` and names the element (`tasks[3].title`); `import_tasks` gives `check_assignee`'s refusal the same path through `in_row` (test: "an import names the refused row"). A method called from another method, an endpoint included, joins its transaction. A failed `native.background` run joins the app's notice list as well as the log, so CL7 stands as written. A.3 and 3.8 count every file as written, from a fresh run; A.3 says why the checked `model.jet` is 16 lines shorter (TR4 drops its marker lines), which is the 224 against 240 that round four found. | 3.2, 4.2, 4.3, 4.10, 8.3, A.3 |

### 0.2 Criteria revision 5 asks for

Main has filed the first two. The rest are edits this revision asks Main to
make; each row gives the text.

| Card | New or changed criterion |
|---|---|
| #3709 c26 (filed) | A replayed edit keeps the row version it was made with, so one whose row changed answers `Conflict`, each with its own notice. |
| #6825 c1 (filed, edit asked) | Each push or mail a job sends gets its own key. Filed as "the queued call key and that send position in the job"; this revision asks for "the run's key, the recipient and the `open:` or `link:` value", because a re-run whose query returns other rows shifts positions and would give a push another push's key. |
| #3709 c22 (edit) | The policy's reads see the transaction's earlier writes but not the write being judged (was "the store before the write"). The creator rule admits an Owner insert only for a project whose row names the caller as `created_by` and has no members. |
| #6813 c3 (edit) | The check before the body is the method `#App.UploadCheck(method:)` names, run as the caller after the session check; a refusal is 403 and the body is never read. |
| #6813 c5 (edit) | Add: a staged file lives no longer than its request, and one left by a crash is deleted when the server next starts. |
| #6826 c5 (edit) | Add: the members policy refuses a member insert from a lapsed invitation. |
| #6445 c7 (edit) | `native.background`'s failed run joins the app's notice list, kept until dismissed, as well as the log (was "native.background logs it"). |

### 0.3 Round three

Round three (`Docs/audits/2026-11-13-web-app-framework-shape-review.md`,
R3-6) listed 28 items. Revision 4 answered each one in the files and the
text; none is left as a promise. Each "(card)" item is a criterion that Main
files or edits (0.4); the row names it. Round four found items 4, 11, 12, 13
and 15 partly closed; 0.1 finishes them.

| # | Item | Revision 4 | Where |
|---|---|---|---|
| 1 | Policy write semantics | A policy takes a `db.Op` and returns a `Bool`. A read is judged on the stored row, an insert on the new row, a delete on the stored row, an update on both; the policy's reads see the store before the write (Postgres `USING` and `WITH CHECK`). A creator rule admits `create_project`'s two inserts. Tests: a Member's `set_role` to Owner is refused; `create_project` makes the caller its owner. Criterion edit on #3709 c22. | 3.2, 4.2, 10 |
| 2 | Project rows writable by members | `project_policy`: members read; only an owner updates or deletes. | 3.2 |
| 3 | Webhook key | `new_webhook_key` stores `app.random_key()`, 256 bits from the operating system, and returns it once to the owner with the path (`WebhookSetup`). A project starts with no key. `None` answers 401 and never skips the check. Criterion #6820 c4. | 3.2, 4.7 |
| 4 | Invitations | `accept_invite` deletes the invitation; invitations lapse after 14 days, `my_invites` hides lapsed ones and a nightly job deletes them. `auth.verified_email` returns the lowercased address. Criterion #6826 c5. | 3.2, 4.7 |
| 5 | Member removal and roles | `set_role`, `remove_member` and `leave`. The last owner is kept (`TaskError.LastOwner`); open live queries re-run and are refused; the `unassign` job clears assignments; `tell_assignee` and `remind_due_today` check membership before they push. Criterion #6826 c5. | 3.2 |
| 6 | Presence counted refused readers | `app.presence` counts only accounts whose last read of that screen passed. Criterion #3709 c25. | 3.2, 4.9, 10 |
| 7 | Client IDs | Each ID type's decoder accepts only UUIDv7 text (422 otherwise); a test sends `"t1"`. `add` meeting a key the caller cannot read answers `Denied`, as a refusal; a test shows an outsider gets `NotAllowed`. Criterion edit on #3709 c23. | 3.2, 4.8 |
| 8 | Drafts in Screens | `ProjectDraft`, `InviteDraft` and the new `TaskBatch` live in `Logic/model.jet`, and the endpoints take them, so the decoder runs their `validate` blocks. The hand copy is gone. `InviteDraft` refuses an Owner invitation. | 3.2, 3.3 |
| 9 | `field_error` at arguments | Both calls are gone: `ProjectDraft` and `TaskBatch` check in their `validate` blocks. The remaining call names `assignee`, a field of the decoded `draft`. | 3.2 |
| 10 | What `app.enqueue` accepts | One call of an `#App.Job` of the same service with codable arguments, evaluated and encoded at the call; anything else is a compile error. Criterion #6825 c5. | 3.2, 4.14, 10 |
| 11 | "Exactly once" | At least once. Each queued call has a key that `app.notify` and `app.mail` pass to APNs, FCM and mail as a collapse ID or Message-ID. Criterion edit on #6825 c1. | 4.14, 10 |
| 12 | Uploads in the transaction | The framework spools the body before the transaction starts; a retry reads the staged bytes; a kept blob belongs to the transaction and a rollback deletes it. Criterion #6813 c5. | 3.2, 5.2.3, 10 |
| 13 | Policies and query counts | A policy whose reads are key lookups compiles into the query as a join; others run per row, memoized per key in the transaction, and `jet inspect choices` names them. `limit` counts rows after the policy. Criterion edit on #6827 c2, c3. | 3.2, 4.2, 9 |
| 14 | Handlers with no destination | The tray and menu bar builders keep their last menu and add a disabled item with the notice; `native.background` logs; `on_resume` shows a notice in the window that came back; a drop shows it beside the drop target. The invitations read and the "also viewing" line show their failure. Criterion edit on #6445 c7. | 3.3, 4.10, 5.1 |
| 15 | A refused queued write with no card on screen | The notice joins the app's notice list, which persists until dismissed; edits rebased on the refused one are replayed, and each fails or passes its own checks with its own notice. Criterion #3709 c26. | 3.7 |
| 16 | Are creates predicted? | Yes: 4.6 now says creates with a client-made ID are predicted, as `new_task.jet` shows, and lists `app.enqueue` among what a prediction skips. Criterion #6555 c7. | 4.6 |
| 17 | No `native.share` example | `examples/tray.jet`'s card menu has "Share…". The 5.1 table and 5.1.7 point to it. | 5.1, 5.1.7 |
| 18 | 6.3's drag row | Names `ui.draggable`, the keyboard pick-up and the long-press menu, as 5.1.7 does. | 6.3 |
| 19 | CSV round trip | Stated as lossy. The import takes off one leading apostrophe before `=`, `+`, `-`, `@`, a tab or a carriage return; the cost is that a title that began with such an apostrophe loses it. | 4.15, 5.1 |
| 20 | `limit_sign_in` card | #6826 c1. | 4.7 |
| 21 | Policies card | #3709 c22. | 3.2 |
| 22 | "No method checks membership" | Now "no method needs a membership check to keep rows private"; role checks through `role_in` are named. | 3.2 |
| 23 | The `mine` test | "a member who leaves sees nothing of the project": after `leave`, `mine`, `search` and `my_projects` return nothing. | 3.2 |
| 24 | The kit in scratch; "clean" | A.7 says the scratch kit is transient and every checked file appears verbatim in this document. A.3 says "clean" means no errors and counts the warnings. | A.3, A.7 |
| 25–28 | D-BOUND-METHOD1 | Dropped from this revision. The shape does not need bound method values: the closure is the target form for `app.live`, keyed by the one call it makes (the rule `app.enqueue` follows), and the webhook's `key:` string is checked at compile time. D-LIVEQUERY1 stands with the closure form, and #6824 is not a dependency of this proposal. | 4.6, 4.7, 7.12, 7.13 |

The claims that failed their own next tests are retested in 8.3: CL2 by
item 8, CL4 by item 16 and CL7 by item 14.

### 0.4 Criteria revision 4 asked for

Main has filed these on Tower; each row names the criterion. 0.2 edits
#3709 c22, #6813 c5, #6825 c1, #6826 c5 and #6445 c7 again.

| Card | New or changed criterion |
|---|---|
| #3709 c22 (edit) | A policy is asked with the operation. A read is judged on the stored row, an insert on the new row, a delete on the stored row and an update on both; the policy's reads see the store before the write. A Member's update of its own member row to Owner is refused. |
| #3709 c23 (edit) | An ID type's decoder accepts only UUIDv7 text. `add` meeting a taken key the caller cannot read answers `Denied`. |
| #3709 c25 | `app.presence` counts only accounts whose last read of that screen passed its policies. |
| #3709 c26 | A refused queued write whose screen is gone, or which is refused after a restart, adds a notice to the app's notice list, kept until dismissed; edits rebased on it replay and are judged each on its own. |
| #6555 c7 | A create with a client-made ID is predicted; a prediction skips `app.enqueue`, `app.notify`, `app.mail` and policies. |
| #6820 c4 | A signing-key method that returns `None` makes the delivery 401; it never skips the check. The `key:` string must name an `#App.SigningKey` method of the same service. |
| #6825 c1 (edit) | A job runs at least once, not exactly once. Each queued call has a key that `app.notify` and `app.mail` pass to the transport (APNs `apns-collapse-id`, FCM collapse key, mail Message-ID). |
| #6825 c5 | `app.enqueue`'s body must be one call of an `#App.Job` method of the same service with codable arguments, evaluated at the call; anything else is a compile error. |
| #6826 c5 | Accepting an invitation deletes it; an invitation has an expiry; `auth.verified_email` returns the lowercased address. Member removal keeps an owner, re-runs the removed member's live queries, and jobs that push check membership. |
| #6813 c5 | An upload is spooled before the transaction; a retry reads the staged body; a blob kept in a transaction that rolls back is deleted. |
| #6827 c2, c3 (edit) | The counts include the policies' statements. A policy whose reads are key lookups joins into the query; `limit` counts rows after the policy. |
| #6445 c7 (edit) | Add the destinations of the tray and menu bar builders, `native.background`'s `run`, `native.lifecycle`'s `on_resume` and `native.drop_files`'s `on_drop`. |
| #6445 c8 | `app.live`'s closure must be one call of an `app.Remote` endpoint with codable arguments, so the subscription is keyed by method and arguments. |

### 0.5 Round two

Round one found 32 gaps. Round two found 25 of them closed and 7 partly
closed, and folded every residual into its own list of 28 items, which
revision 3 answered. Each row says what revision 3 did and where. "Card"
means the item is a framework gap that a card now tracks; the app does not
work around it.

**A hole in revision 2's evidence.** In revision 2, every package that
depends on `Screens` stopped at `jet.DependencyCheckFailed` before the seed
checked its own code, because the checked copy of `Screens` had errors. So
the "clean" rows for `Web`, `Desktop`, `Phone` and every file under
`examples/` were not checked. Revision 3 adds two stub transforms (TR5 and
TR6, A.2), after which `Screens` checks without error. Every row in A.3 is
now a real result.

| # | Item | Revision 3 | Where |
|---|---|---|---|
| 1 | Language gaps behind the API rule | Three criteria on #6819 (c4 closure-parameter inference, c5 a closure ending in a call that returns nothing, c7 conversions from another package) and a language card, #6824, for bound method values with ballot D-BOUND-METHOD1. The public API no longer bends around them: styles are plain functions, `ui.drop_zone` infers its type from the closure, and the old amendment 2 to D-LIVEQUERY1 is withdrawn. Section 7.13 lists each interim form beside its target form. | 3.9, 7.12, 7.13, A.4 |
| 2 | CL1's evidence was false | The checked copy now replaces `app.Remote<Crew>` with a generated `CrewRemote` that has only the marked methods (TR2). The probe `probes/cl1_server_only.jet` calls the signing key and a job from a screen, and the seed refuses both. The verdict is "matches Convex". | 4.1, 8.3, A.2 |
| 3 | `#Secret` flows were cited as E0721 | Every such statement now says "proposed in #6557 (D-SECRETEDGE1)". The probe `probes/cl5_secret_print.jet` prints a secret, and seed-b14 accepts it. | 1, 4.7, 8.3 |
| 4 | Claim verdicts | CL2, CL3, CL4, CL7, CL8 and CL9 are "matches", each naming the system round two named. CL1 matches Convex. CL5 goes further only once #6557 is built. CL6 is narrowed to methods marked as webhooks and gains redelivery de-duplication. | 8.3 |
| 5 | CL3 failed its own test | All targets share one route set. Only the graph builder in `Screens` adds screens; a `.screen` call in an entry package is a compile error (#6445 c6). Mine is now a shared screen, and the phone only adds a tab for it. | 3.3, 4.4 |
| 6 | Matrix defects | 8.2's Qwik row says Jet resumes, as 4.13 does. CL7 is "matches" in both 8.1 and 8.2. 8.1 has new rows for performance, authorization, server jobs, email, rate limits and observability. | 8.1, 8.2 |
| 7 | Hand-written membership checks; the `mine` leak | Typed row policies (D-DBPOLICY1, #3709 c22) in `Logic/policy.jet`, attached with `.guard(...)`. The database applies them to every read and write, so `mine` returns only tasks in the caller's projects. A test proves it. | 3.2, 4.2 |
| 8 | Optional caller inside `#App.SignedIn` | `app.caller()` returns `auth.AccountID`, not an option. The compiler accepts it only on paths that start at an `#App.SignedIn` method or a test's `as_caller` (#6445 c4); `app.maybe_caller()` is the form for `#App.Public`. | 4.1, 4.7 |
| 9 | Writes not atomic, no version on moves | Each method that writes is one serializable transaction, retried when it loses a race (#6445 c5). That makes the version check and the write one step. `move_task` takes the version the board shows. The webhook's loop and the CSV import are each one transaction (`import_tasks`). | 3.2, 4.2 |
| 10 | Joins, N+1 queries, in-memory search | `my_projects` makes two queries whatever the number of projects (`count_by`). `search` uses the table's full-text index. Relation loading and the index are #6827. | 3.2, 4.2 |
| 11 | Offline creates need a server ID | IDs are UUIDv7 text made where the row is created (`app.new_id`). `new_task` makes the ID, so it opens the task at once, even offline. `Table.add` refuses a taken key (#3709 c23). | 3.2, 4.8 |
| 12 | No after-commit work, jobs or cron | `#App.Job` methods queued with `app.enqueue` run after the transaction commits; `#App.Every(cron:)` runs on a schedule (#6825). The assignee push and the invitation mail are jobs. The "due today" reminder is a server schedule that pushes. | 3.2, 4.14 |
| 13 | No account lifecycle | Crewboard creates projects, invites by confirmed email and accepts invitations. The server entry turns on email confirmation, password reset, passkeys, two-factor codes and a sign-in rate limit; `kit.account_flows()` adds their screens (#6826). | 3.2, 3.4, 4.7 |
| 14 | Handlers had no error destination | A handler's type is `fn() app.Notice!`: its error must convert to `app.Notice`, which the control shows beside itself and announces (#6445 c7). `_:` drops only the success value; the error still propagates. 3.7 now says what a queued write's button shows. | 3.7, 4.10 |
| 15 | c6 examples not wired in | A task has attachments (`Crew.attach`, the task screen); assigning a task queues the push job. What wiring found: send after commit (item 12), storage that names files (item 17), authorization before the body is read, and the missing download policy (#6813 c4). | 3.2, 3.9 |
| 16 | Webhook replay | The framework drops a delivery whose `X-GitHub-Delivery` it has run (#6820 c3). | 3.2, 4.7 |
| 17 | `uploads.jet` bugs | Deleted. `app.Blobs.keep` names each blob itself and checks the first bytes against an allow list (415); `Crew.attach` loads the task through its row policy and checks `can_edit` (403) before the body is read; `max_upload` gives 413 first (#6813). | 3.2, 5.2.3 |
| 18 | CSV formula injection | `core.encoding.csv` writers escape a cell that starts with `=`, `+`, `-`, `@`, a tab or a carriage return (#6828). | 3.5, 4.15 |
| 19 | Presence and the signing-key lookup | Presence is the method `Crew.viewers`, which reads the task first, so the task's row policy decides who learns who is looking. The webhook marker names its key: `key: "webhook_secret"` (#6820 c2). | 3.2, 4.9 |
| 20 | Assignee not checked | `check_assignee` refuses an assignee outside the project with `app.field_error(at: "assignee", ...)`, which the form shows beside the field (#6831). | 3.2, 4.3 |
| 21 | File paths on the web | `native.PickedFile` replaces path strings: a browser `File`, a content URI or a desktop path behind one type (#6802 c1). | 5.1.4 |
| 22 | Background on the web; single instance | 5.1 and #6801 agree: browsers run no background work, and `native.background` in a web graph is a compile error (#6801 c3). A second desktop launch hands its link to the running app (#6804 c3). | 5.1.3, 5.1.6 |
| 23 | `realtime.jet` comment | It now says the web entry sets no queue, so `app.pending()` is always 0 there. | 4.8 |
| 24 | Sharing rule versus #3709 | Stated: a caller-dependent live query, such as each of Crewboard's, is not shared, and costs one run per subscriber per write that meets its read set (#3709 c24). | 4.9 |
| 25 | Styling | The whole board, with both layouts, in every form; "no dead rules"; a focus ring in the SwiftUI form; S1 A without the wrapper; the theme type is a compile-time check; rows for contrast, forced colors, a dragging alternative, overlays, scrolling and view identity (#6817 c1–c3). Counts redone. | 6 |
| 26 | Criteria on #6799–#6817 | Main posted executable-example criteria on each card, taken from the 5.1 and 5.2 examples. | 5 |
| 27 | L0530 on `validate {` | Reported as A.6 finding 6 and #6819 c6. It fires on every struct with a `validate` block. | A.6 |
| 28 | Ratified syntax | Every struct and enum body has commas between members (D-BRACE-LAYOUT1=A). The two-part member key is the named struct `MemberKey`, because the unnamed record of D-RECORD-BRACE1=A waits on #6251 c12. | 3.2 |

## 1. The shape in one page

The owner's goal: write business logic once and run it in a native app
(desktop and phone) and in the browser as a web app. The shape has eight
rules.

1. **One client-safe app graph.** `app.new(...)` builds one checked graph:
   screens, the services they call, sign-in, error views and policies. Sema
   evaluates the builder chain (D-WEBAPP1). Every target runs the same graph
   (D-ONEAPP-MODEL1), so the graph holds no database handle and no secret.
   Only the graph adds screens, so every target has the same route set.
   Each target's entry file adds only what is specific to it: the server
   entry mounts the real services and sets accounts and mail; the desktop
   entry adds menus; the phone entry adds tabs and app links.
2. **One logic package with no UI.** Models, validation, permission rules,
   queries, writes and the screen states (the routes) live in an ordinary
   package. A service is a struct whose fields are its server resources
   (tables, file storage). Typed row policies on the tables decide who reads
   and writes each row (D-DBPOLICY1). Each method that writes is one
   transaction, and its side effects (pushes, mail) are jobs that run after
   the commit. A screen receives the service as `app.Remote<Crew>` and calls
   its methods like any method. On the server the call is direct; elsewhere
   it is a typed remote call.
3. **Safe by default.** A service method is an endpoint only when it is
   marked `#App.SignedIn` or `#App.Public`. Every other method is
   server-only, and a client call to it is a compile error. Signed-in
   screens are `noindex` and `Cache-Control: private, no-store`. A webhook
   must declare how its signature is checked. Secrets are a type tag
   (D-SECRET-CT1); the rule that a secret never reaches a client, a reply or
   a log is proposed in #6557 (D-SECRETEDGE1) and is not built.
4. **Placement comes from facts, and it is shown and can be pinned.** Code
   that touches a server resource runs only on the server. Code with the
   `Browser` effect runs only in the page (D-WASM1). Pure code runs wherever
   it is called. Reading the request context is not an effect
   (D-CONTEXT-EFFECT1=F), so `jet inspect choices` (#6823) lists each
   function that reads it, with every other placement fact (D-AUTOPIN1).
5. **One route identity, one route set.** A screen's state struct is its
   address. Path parameters fill fields of the same name, and `#App.Query`
   fields fill the query string. URLs, browser Back, deep links, pushes and
   mail links all carry the same value (D-ROUTE-ONE1), and every target
   knows every route.
6. **Data flows without keys.** A live query is a read-only method. A
   committed write whose write set meets its read set refreshes it
   everywhere (D-LIVEQUERY1, D-INVALIDATE1). A typed edit of rows the client
   already holds is predicted on the client (D-OPTIMISTIC1). Offline devices
   queue writes and keep the last results they read.
7. **Jet draws the views on native targets, and the web gets real HTML.**
   Screens are built from `ui` primitives and the `kit` standard views
   (D-VIEW-SET1). On desktop and phones Jet draws every control
   (D-ONEAPP-RENDER1). On the web it writes semantic HTML on the server and
   resumes handlers in the page (D-ONEAPP-WEBOUT1, D-RESUME1). `native.view`
   embeds a real system view where an expert asks for one
   (D-PLATFORM-OVERRIDE1).
8. **Platform features are plugins.** Menus, a tray, tabs, app links,
   push, background work and lifecycle hooks come from the `native` package
   as `app.Plugin` values added with `.with(...)`, the way Tauri 2 adds
   plugins [T2]. The graph and the screens do not change.

```text
                 ┌───────────── logic (package, no UI) ───────────────┐
                 │ models · rules · routes · row policies · Crew      │
                 └───────────────┬────────────────────────────────────┘
                                 │ used by
                 ┌───────────── screens (package) ────────────────────┐
                 │ client-safe graph · one route set · ui + kit views │
                 └──┬──────────────┬───────────────┬──────────────────┘
          Web/web.jet        Desktop/desktop.jet   Phone/phone.jet
          mounts Crew,       menus, windows,       tabs, app links, push
          accounts, mail     offline queue         offline queue
          (Service output)   (Executable output)   (Bundle output)
             ▲   HTML + resumed handlers, typed calls, live pushes    ▲
             └──────────── browser · desktop · phone call the server ─┘
```

### What runs where, at a glance

| Code | Server | Browser | Desktop | Phone |
|---|---|---|---|---|
| Pure logic (rules, `validate` blocks) | yes | yes (Wasm) | yes | yes |
| `#App.SignedIn` and `#App.Public` methods of `Crew` | runs the body | typed remote call | typed remote call | typed remote call |
| Unmarked and `priv` methods, webhooks, signing keys | runs the body | compile error | compile error | compile error |
| `#App.Job` and `#App.Every` methods | runs after commit, or on schedule | compile error | compile error | compile error |
| Row policies | run on every read and write | — | — | — |
| Live queries (`app.live`) | runs, tracks the read set | subscribes, caches | subscribes, caches on disk | subscribes, caches on disk |
| Screens (`ui`, `kit`) | renders the first HTML | resumes, then updates the DOM | drawn by Jet | drawn by Jet |
| `Browser` code (`core.browser`) | compile error | yes | compile error | compile error |
| `native` plugins | serves their web files (app-link files, manifest) | the web form where one exists | yes | yes |
| `#Secret` values | yes | proposed compile error (#6557) | proposed compile error | proposed compile error |

"Compile error" means the compiler names the path from the screen to the
offending call. Nothing is left out silently.

## 2. Today's code versus the shape

Today's code was read at dev `93380f43ef`. Core/Web is 7,143 lines:
Web.jet, Router.jet, Forms.jet, Query.jet, Store.jet, Table.jet, Virtual.jet,
Browser.jet and Storage. `Core/App/App.jet` declares the package `app`
(live-query, sync and auth wrappers). D-WEB-RUNTIME-API1=A keeps `app` in
Core and moves `core.web.browser` to `core.browser`. The web battery is
`Jetpack/Provider/Batteries/web/web.jet` (244 lines of route, form, database
policy, live-query and login helpers). Examples live in `Examples/Apps/Web`.

| Today | The shape | Decision or card |
|---|---|---|
| `web.app().route("/", home).action("save", save).serve()` (Examples/Apps/Web/AppHello.jet) | `app.new(title: "Crewboard").api<logic.Crew>("/api").screen("/", projects)`, then the server entry calls `.mount(crew)` | D-ONEAPP-MODEL1, D-ROUTE-ONE1, #6445 |
| A page returns `WebPage` built from strings: `web.page("People", form.render())` (FormsTable.jet) | A screen returns `ui.Node`. On the web it becomes server HTML; natively Jet draws it. | D-ONEAPP-WEBOUT1, #6447 |
| `web.form(Person, action: save)` renders an HTML form string | `kit.form(initial, submit:, on_submit:, on_error:, fields:)` builds a view. The struct's `validate` block runs in the form and again in the server's decoder. | D-VALIDATE1, D-VIEW-SET1, #6821 |
| `web.live(user_count, "admin")`, `web.invalidate(user_count, "admin")` (StateSurface.jet) | `app.live(fn() { crew.board(view.project) })`. A database write needs no invalidate call. | D-INVALIDATE1 |
| `query.mutate_with_invalidations(q, key, None, [keys], fn)` | Removed: read and write sets replace keys | D-INVALIDATE1 |
| `reactive.signal(100)` (Examples/Apps/UI/Reactive.jet) | `ui.local(value)` for screen-local state; the screen state struct for state that lives in the address | D-REACT-REF1 |
| Core/Ui has node, box, text, button, text_input and Window | The `ui` package has the primitives, and `kit` holds the standard views | D-VIEW-SET1, #6447 |
| `.csr()`, `.ssr()`, `.island()`, `.hydration_dev()` knobs | Resumable by default, with per-route code. Each knob becomes an inspectable choice with a pin. | D-RESUME1, D-AUTOPIN1 |
| `web.auth("session")` with string configuration | `.sign_in(SignInView{})` on the graph, `.accounts(...)` in the server entry; `app.caller()` in a method | D-AUTH1, #6818, #6826 |
| The battery's database policy helpers | typed row policies attached with `.guard(policy)` | D-DBPOLICY1, #3709 c22 |
| `jet build --target=web`, `--target=portable` and wasm builds stop with E2104 "is not built yet" | `jet build server`, `jet build desktop`, `jet build phone --target=android` | D-NATIVEUI3, D-TARGET-ONE1 |
| Native windows: `ui.application()` and `app.window(...)` give E1004 today | `.window(width:, height:)` and `.open_in_window<V>()` in the desktop entry | D-UI-WINDOW-LIFE1, #2999 |
| No style system: Core/Ui has `Color` and `SizeConstraint`; the web host positions every element absolutely with inline pixel styles (`Compiler/JetWeb/Source/Web/Host.jet:389–393`) | Typed styles from theme tokens, laid out by the browser on the web and by Jet natively (section 6) | D-THEME2, D-LAYOUT2, #6817 |

## 3. Crewboard: the business app, used as a probe

Crewboard is a task tracker for small teams. People create an account,
confirm their email, and create projects or accept invitations to them. A
project is a board with three columns: To do, Doing and Done. Owners invite
people by email and choose their role; owners and members create, edit, move
and attach files to tasks; viewers only read. An assigned task sends a push
to its assignee, and each morning the server reminds people of work due that
day. A closed GitHub issue closes the task with the same title. The web app,
a desktop app and a phone app share every rule, every route and every
screen, and all of them talk to one server.

Crewboard is also a probe (card c8). Every need it has is either met by the
proposed framework, or it is a framework gap with a card. The app never works
around a gap. Section 3.9 lists what the probe found.

### 3.1 The workspace

One workspace holds five packages and three outputs. The root manifest says
what the project makes (D-TARGET-ONE1).

`crewboard/package.jet`

```jet
// Proposed, not built (#6552 c8 targets; #6810 desktop install and update;
// #480 phone output). The seed parses this manifest but does not check
// output kinds yet.
// One workspace, five packages, three outputs. The server output is the only
// one that opens a database; the desktop and phone outputs call it at `server:`.
name: "crewboard"
version: "0.1.0"
edition: "2026"
members: [
    "Logic",
    "Screens",
    "Web",
    "Desktop",
    "Phone",
]
outputs: {
    server: .Service{ name: "crewboard", package: "Web" },
    desktop: .Executable{
        name: "Crewboard",
        package: "Desktop",
        server: "https://crew.example.com",
        // Signed updates: the app checks this feed and verifies each build
        // against the key before installing it.
        updates: .Feed{ url: "https://crew.example.com/updates", key: "keys/updates.pub" },
    },
    phone: .Bundle{
        name: "Crewboard",
        package: "Phone",
        id: "com.example.crewboard",
        server: "https://crew.example.com",
        // Why the app asks for each permission (#6806). The build writes the
        // Info.plist and AndroidManifest.xml entries from these lines, and a
        // call that needs a permission with no line here fails the build.
        // Crewboard has no location feature, so it asks for no location.
        purposes: {
            notifications: "Tell you when a task is assigned to you.",
            camera: "Attach photos to tasks.",
        },
    },
}
```

| Package | Holds | Depends on |
|---|---|---|
| `Logic` | types, `validate` blocks, rules, routes, row policies, notices, the `Crew` service, tests | `auth` |
| `Screens` | the client-safe graph, every screen, the theme, screen tests | `ui`, `kit`, `auth`, `Logic` |
| `Web` | the server entry: mounts `Crew`, the account store and mail | `auth`, `Logic`, `Screens` |
| `Desktop` | the desktop entry: menus, windows, CSV export | `native`, `Logic`, `Screens` |
| `Phone` | the phone entry: tabs, app links, push | `native`, `Logic`, `Screens` |

`app` is part of Core (D-WEB-RUNTIME-API1=A), so no manifest lists it. The
package manifests look like this one:

`crewboard/Phone/package.jet`

```jet
// Checked on seed-b15 as a manifest.
name: "phone_entry"
version: "0.1.0"
edition: "2026"
deps: {
    native: "0.1.0",
    logic: ../Logic,
    screens: ../Screens,
}
```

### 3.2 Logic: types, rules, routes, policies and the service

`model.jet` holds the types.

- **IDs.** `TaskID` and `ProjectID` are distinct `String` types holding a
  UUIDv7 made where the row is created (`app.new_id`). A task made offline
  has its final ID at once, and a link to it never changes.
- **Validation.** `TaskDraft` has a `validate` block in the one form dev
  accepts: `check(condition, at: field, "message")`. A failed check becomes
  a `FieldError` with `path` and `reason`. The derived decoder runs the same
  block (spec.md, "A derived decoder validates through the same list";
  D-VALIDATE-DECODE1=B), so a crafted request meets the same rules as the
  form.
- **Secrets.** `Project` is not `#Codable`, so no reply, screen or log can
  carry one. Its `webhook_secret` is `#Secret String`, the fixed-timing tag
  of dev today. The rule that a secret never reaches a client is proposed in
  #6557 (D-SECRETEDGE1); seed-b15 accepts printing one (A.4).
- **Errors.** `TaskError` has a case for each way a call can fail. The
  conversions from `app.CallError`, `db.DBError` and `app.BlobError` say how
  a transport, storage or file failure becomes one.
- **Syntax.** Struct and enum members are separated by commas
  (D-BRACE-LAYOUT1=A). The two-part member key is the named struct
  `MemberKey`: the unnamed record of D-RECORD-BRACE1=A waits on #6251 c12.

`crewboard/Logic/model.jet`

```jet
// Proposed, not built (#6819 codable distinct IDs; #6445 app.CallError).
use {
    app
    auth
    db: core.db
    time: core.time
}

// IDs are UUIDv7 text made where the row is created, so a task made offline
// has its final ID at once and a link to it never changes. Each ID type's
// derived decoder accepts only UUIDv7 text (#3709 c23): "t1" in a crafted
// request is refused with 422 before any method runs. An ID prints as its
// text, for paths.
#Printable
TaskID: distinct String
#Printable
ProjectID: distinct String

Status: enum {
    Todo,
    Doing,
    Done,
}

Role: enum {
    Owner,
    Member,
    Viewer,
}

#[Codable, PublishedSchema]
Task: struct {
    id: TaskID,
    project: ProjectID,
    title: String,
    notes: String,
    status: Status,
    assignee: auth.AccountID?,
    due: time.LocalDate?,
    version: Int,
}

// What a person types. Validation runs when a form submits and again when the
// server decodes the value, so a crafted request meets the same rules.
#Codable
TaskDraft: struct {
    title: String{""},
    notes: String{""},
    assignee: auth.AccountID?{None},
    due: time.LocalDate?{None},

    validate {
        check(title.trim().len() > 0, at: title, "Give the task a title.")
        check(title.len() <= 200, at: title, "Keep the title under 200 characters.")
        check(notes.len() <= 10000, at: notes, "Notes are limited to 10,000 characters.")
    }
}

// What the "New project" form holds. The server's decoder runs the same
// validate block, so create_project has no check of its own.
#Codable
ProjectDraft: struct {
    name: String{""},

    validate {
        check(name.trim().len() > 0, at: name, "Give the project a name.")
        check(name.len() <= 100, at: name, "Keep the name under 100 characters.")
    }
}

// What the invite form holds. Owners are made from the members list, never
// by invitation; the server's decoder refuses a crafted Owner invitation.
#Codable
InviteDraft: struct {
    email: String{""},
    role: Role{Role.Member},

    validate {
        check(email.contains("@"), at: email, "Enter an email address.")
        check(role != Role.Owner, at: role, "Invite as a member or a viewer.")
    }
}

// A CSV import: up to 500 tasks, all added or none. The decoder runs each
// TaskDraft's validate block too, and names the row: `tasks[3].title`.
#Codable
TaskBatch: struct {
    tasks: [TaskDraft],

    validate {
        check(tasks.len() <= 500, at: tasks, "Import at most 500 tasks at a time.")
    }
}

// Not #Codable: no response, screen or log can carry a Project. The tag on
// webhook_secret is the fixed-timing #Secret of dev today; the rule that a
// secret never reaches a reply, a log or a client is proposed in #6557
// (D-SECRETEDGE1). A project has no key until an owner makes one, and until
// then every delivery gets 401. `created_by` is the account that inserted
// the row; the creator rule (Logic/policy.jet) admits only that account as
// the first owner, and no update changes it.
Project: struct {
    id: ProjectID,
    name: String,
    webhook_secret: #Secret String?,
    created_by: auth.AccountID,
}

// What an owner pastes into GitHub's webhook settings. The key is plain text
// here on purpose: this reply, to an owner, is the one place it leaves the
// server, once, when it is made.
#[Codable, PublishedSchema]
WebhookSetup: struct {
    path: String,
    key: String,
}

// A table key with two parts. D-RECORD-BRACE1 would allow the unnamed
// {project:, account:}; that waits on #6251 c12, so the key is named.
#Codable
MemberKey: struct {
    project: ProjectID,
    account: auth.AccountID,
}

#[Codable, PublishedSchema]
Member: struct {
    project: ProjectID,
    account: auth.AccountID,
    name: String,
    role: Role,
}

// An invitation is to an email address, lowercased. Only an account that has
// confirmed that address can accept it, so no token travels in the mail.
// Accepting deletes it; past `expires` it cannot be accepted, and a nightly
// job deletes it.
#Codable
InviteKey: struct {
    project: ProjectID,
    email: String,
}

#[Codable, PublishedSchema]
Invite: struct {
    project: ProjectID,
    project_name: String,
    email: String,
    role: Role,
    expires: time.LocalDate,
}

// A file on a task. The storage names the blob; `name` is only shown.
#[Codable, PublishedSchema]
Attachment: struct {
    blob: app.BlobID,
    task: TaskID,
    project: ProjectID,
    name: String,
    media: app.Media,
    bytes: Int,
}

// What GitHub posts when an issue changes. Only the fields Crewboard reads.
#Codable
IssueEvent: struct {
    action: String,
    issue: IssueFields,
}

#Codable
IssueFields: struct {
    title: String,
}

#[Codable, PublishedSchema]
ProjectCard: struct {
    id: ProjectID,
    name: String,
    open_tasks: Int,
}

#[Codable, PublishedSchema]
Board: struct {
    project: ProjectCard,
    tasks: [Task],
    members: [Member],
    role: Role,
}

#[Codable, PublishedSchema]
TaskPage: struct {
    task: Task,
    members: [Member],
    attachments: [Attachment],
    role: Role,
}

#Error
TaskError: enum {
    SignedOut,
    NotFound,
    NotAllowed,
    Invalid([FieldError]),
    Conflict(Task),
    BadFile(String),
    LastOwner,
    Unreachable(String),
    UpdateRequired,
    Storage(String),
}

// A remote call can fail on the way. This conversion is what lets a screen
// call Crew through app.Remote: each transport failure has a case.
impl app.CallError -> TaskError {
    if self == {
        .Unreachable(why) -> TaskError.Unreachable(why)
        .Invalid(fields) -> TaskError.Invalid(fields)
        .SignedOut -> TaskError.SignedOut
        .UpdateRequired -> TaskError.UpdateRequired
    }
}

// A row policy's refusal is NotAllowed. A taken key the caller can read is a
// conflict the client resolves by making a new ID; one it cannot read is
// Denied, so it learns nothing about other projects.
impl db.DBError -> TaskError {
    if self == {
        .Denied -> TaskError.NotAllowed
        .Exists -> TaskError.Storage("that ID is taken")
        .Closed | .Constraint(_) -> TaskError.Storage(self.summary())
    }
}

impl app.BlobError -> TaskError {
    if self == {
        .WrongType -> TaskError.BadFile("Only images and PDF files can be attached.")
        .TooLarge -> TaskError.BadFile("That file is larger than 10 MB.")
        .Storage(why) -> TaskError.Storage(why)
    }
}

// Entry points and tests run in the default error family.
impl TaskError -> Err {
    Err(describe(self))
}

describe: fn(error: TaskError) -> String Never! {
    if error == {
        .SignedOut -> "signed out"
        .NotFound -> "not found"
        .NotAllowed -> "not allowed"
        .Invalid(fields) -> "{fields.len()} invalid fields"
        .Conflict(_) -> "edited by someone else"
        .BadFile(why) -> why
        .LastOwner -> "a project needs an owner"
        .Unreachable(why) -> "unreachable: {why}"
        .UpdateRequired -> "this version of Crewboard is too old"
        .Storage(why) -> "storage: {why}"
    }
}

new_task_id: fn() -> TaskID Never! {
    TaskID.from_string(app.new_id())
}

draft_of: fn(task: Task) -> TaskDraft Never! {
    TaskDraft{title: task.title, notes: task.notes, assignee: task.assignee, due: task.due}
}

status_label: fn(status: Status) -> String Never! {
    if status == {
        .Todo -> "To do"
        .Doing -> "Doing"
        .Done -> "Done"
    }
}
```

`rules.jet` is plain functions over plain values. They run on every target:
the board uses `can_move` to decide whether to show a button, and the server
uses it again to decide whether to allow the move.

`crewboard/Logic/rules.jet`

```jet
// Checked on seed-b15. Plain functions over plain values: no effects, no I/O.
can_edit: fn(role: Role) -> Bool Never! {
    role == Role.Owner || role == Role.Member
}

can_move: fn(role: Role, from: Status, to: Status) -> Bool Never! {
    if !can_edit(role) -> return false
    if from == to -> return false
    // Only an owner reopens finished work.
    if from == Status.Done -> return role == Role.Owner
    true
}

next_status: fn(status: Status) -> Status Never! {
    if status == {
        .Todo -> Status.Doing
        .Doing -> Status.Done
        .Done -> Status.Todo
    }
}

column: fn(tasks: [Task], status: Status) -> [Task] Never! {
    out := [Task]{}
    loop t in tasks {
        if t.status == status -> out.push(t.copy())
    }
    out
}

// Tasks whose title holds every word of the query, in the order given.
matching: fn(tasks: [Task], query: String) -> [Task] Never! {
    words := [String]{}
    loop w in query.lower().split(" ") {
        if w.len() > 0 -> words.push(w)
    }
    out := [Task]{}
    loop t in tasks {
        title: t.title.lower()
        hit := true
        loop w in words {
            if !title.contains(w) -> hit = false
        }
        if hit -> out.push(t.copy())
    }
    out
}
```

`routes.jet` holds the screen states. They live in `Logic`, not `Screens`,
because the server names them too: a push opens `TaskView{id}` and an
invitation mail links to `ProjectsView{}`.

`crewboard/Logic/routes.jet`

```jet
// Proposed, not built (#6445 app graph; #6552 c8 typed screens).
use {
    app
}

// The app's addresses. Each screen's state is its own struct, and the struct
// is the route: links, deep links, pushes, mails and the back stack carry it.
// The server needs them too (a push opens TaskView, an invitation mail links
// to ProjectsView), so they live here, beside the logic, and hold only IDs.
// Every target uses this one route set (section 4.4).
ProjectsView: struct {}

BoardView: struct {
    project: ProjectID,
    // The filter box. A #App.Query field lives in the address after `?`.
    #App.Query
    q: String{""},
}

TaskView: struct {
    id: TaskID,
}

NewTask: struct {
    project: ProjectID,
}

SearchView: struct {
    #App.Query
    q: String{""},
}

// Open tasks assigned to the caller. The phone shows it as a tab; the web and
// the desktop have it at /mine.
MineView: struct {}

SignInView: struct {}
```

`policy.jet` holds the row policies (D-DBPOLICY1, #3709 c22). A policy is a
named function from a row, the operation (`db.Op`: Read, Insert, Update or
Delete), the caller (`db.Who`) and the store to a `Bool`. `open_crew`
attaches one to each table with `.guard(...)`, and the database asks it
about every read and every write of that table: endpoints, live queries,
jobs and webhooks. The rules for a write follow Postgres's `USING` and
`WITH CHECK` [PG-POLICY]:

- a read is judged on the stored row, an insert on the new row and a
  delete on the stored row;
- an update is judged twice, on the stored row and on the new one, and
  both must pass;
- the policy's own reads see the transaction's earlier writes but not the
  write being judged, so a write cannot vouch for itself. A Member who
  rewrites its own member row as Owner is judged on the stored row, where
  it is a Member, and refused. `create_project`'s member insert sees the
  project row that the same transaction inserted just before.

A creator rule lets `create_project` start a project. Any account may insert
a project row that names it as `created_by`. It may then insert itself as
the owner of that project while the project has no members, and no other
account may: a project whose members are all gone cannot be claimed by the
next account that tries. No update changes `created_by`. Project rows are
owner-write, which covers the name and the webhook key; members read them.
So no method needs a membership check to keep rows private, and none can
leak a row by forgetting one. Roles are a different matter: `move_task` and
`attach` still check what the caller's role allows, through `role_in`.
Revision 2's `mine` returned tasks from projects the caller had left; now
the task policy hides them, and the test "a member who leaves sees nothing
of the project" proves it.

`crewboard/Logic/policy.jet`

```jet
// Proposed, not built (D-DBPOLICY1 typed row policies, #3709 c22).
use {
    auth
    db: core.db
    time: core.time
}

// Row policies. open_crew attaches one to each table, and the database asks
// it about every read and every write of that table: endpoints, live
// queries, jobs and webhooks alike. A read is judged on the stored row, an
// insert on the new row, a delete on the stored row, and an update twice, on
// the stored row and on the new one (Postgres's USING and WITH CHECK). The
// policy's own reads see the transaction's earlier writes but not the write
// being judged, so a write cannot vouch for itself. A row the caller may not
// read is never returned; a refused write is db.DBError.Denied, which Crew
// sees as TaskError.NotAllowed. A policy reads with System rights, so it can
// read the members table.
// Cost (section 4.2): task_policy, attachment_policy, project_policy and
// member_policy's read look up the caller's member row by key, so the
// database joins them into the query. invite_policy (it asks
// auth.verified_email) and member_policy's insert (first_owner counts the
// members, invited reads the invitation) run per row, memoized per key.
// `jet inspect choices` lists each policy, how it runs, and each place that
// runs as System.

// Members read a project's tasks and files; owners and members write them.
task_policy: fn(row: Task, op: db.Op, who: db.Who, store: db.Store) -[DB.Read]> Bool db.DBError! {
    member_may(store, row.project, op, who)
}

attachment_policy: fn(row: Attachment, op: db.Op, who: db.Who, store: db.Store) -[DB.Read]> Bool db.DBError! {
    member_may(store, row.project, op, who)
}

// Members read the project row; only an owner changes or deletes it, which
// covers its name and its webhook key. The creator rule: any signed-in
// account may insert a new project that names it as `created_by`, and
// member_policy then lets it insert itself as the project's first owner. An
// update keeps `created_by`: the stored row is the one before this write.
project_policy: fn(row: Project, op: db.Op, who: db.Who, store: db.Store) -[DB.Read]> Bool db.DBError! {
    account: account_of(who) ?? return Ok(true)
    if op == db.Op.Insert -> return Ok(row.created_by == account)
    role: role_of(store, row.id, account) ?? return Ok(false)
    if op == db.Op.Update {
        stored: projects_in(store).get(row.id) ?? return Ok(false)
        return Ok(role == Role.Owner && stored.created_by == row.created_by)
    }
    Ok(op == db.Op.Read || role == Role.Owner)
}

// Members see who else is in the project. Only an owner adds, changes or
// removes a member, except in three cases that name the caller's own row: it
// may delete it (leaving), insert it as the first owner of a project it
// created that has no members yet (the creator rule), or insert it with the
// role that an unexpired invitation to its confirmed address grants. A
// Member who rewrites its own row as Owner is refused on the stored row,
// where it is not an owner. Keeping one owner is a rule across rows; Crew
// checks it (keep_an_owner).
member_policy: fn(row: Member, op: db.Op, who: db.Who, store: db.Store) -[DB.Read, Time]> Bool db.DBError! {
    account: account_of(who) ?? return Ok(true)
    owner: role_of(store, row.project, account) == Val(Role.Owner)
    own_row: row.account == account
    if op == {
        .Read -> Ok(role_of(store, row.project, account) != None)
        .Update -> Ok(owner)
        .Delete -> Ok(owner || own_row)
        .Insert -> Ok(owner || (own_row && (first_owner(store, row, account) || invited(store, row, account))))
    }
}

// Owners send, change and withdraw a project's invitations. The invited
// account reads its own and deletes it when it accepts; it cannot change it.
invite_policy: fn(row: Invite, op: db.Op, who: db.Who, store: db.Store) -[DB.Read]> Bool db.DBError! {
    account: account_of(who) ?? return Ok(true)
    if role_of(store, row.project, account) == Val(Role.Owner) -> return Ok(true)
    mine: auth.verified_email(account) == Val(row.email)
    Ok(mine && (op == db.Op.Read || op == db.Op.Delete))
}

member_may: fn(store: db.Store, project: ProjectID, op: db.Op, who: db.Who) -[DB.Read]> Bool db.DBError! {
    account: account_of(who) ?? return Ok(true)
    role: role_of(store, project, account) ?? return Ok(false)
    Ok(op == db.Op.Read || can_edit(role))
}

// None for System, which may read and write every row.
account_of: fn(who: db.Who) -> auth.AccountID? Never! {
    if who == {
        .Account(account) -> Val(account)
        .System -> None
    }
}

role_of: fn(store: db.Store, project: ProjectID, account: auth.AccountID) -[DB.Read]> Role? db.DBError! {
    found: members_in(store).get(MemberKey{project: project, account: account}) ?? return Ok(None)
    Ok(Val(found.role))
}

// The creator rule, tied to the project row: an Owner insert for a project
// whose row names the caller as its creator and that has no members yet. In
// create_project the project row is the transaction's earlier write, so the
// policy sees it. No other account can claim a project whose members are
// gone.
first_owner: fn(store: db.Store, row: Member, account: auth.AccountID) -[DB.Read]> Bool db.DBError! {
    if row.role != Role.Owner -> return Ok(false)
    project: projects_in(store).get(row.project) ?? return Ok(false)
    if project.created_by != account -> return Ok(false)
    Ok(members_in(store).query().filter(fn(m: Member) { m.project == row.project }).count() == 0)
}

// An invitation to the caller's confirmed address that grants this role and
// has not lapsed. accept_invite checks the date too, to answer NotFound, but
// the policy is the check that no method can forget.
invited: fn(store: db.Store, row: Member, account: auth.AccountID) -[DB.Read, Time]> Bool db.DBError! {
    email: auth.verified_email(account) ?? return Ok(false)
    found: invites_in(store).get(InviteKey{project: row.project, email: email}) ?? return Ok(false)
    Ok(found.role == row.role && found.expires >= time.today())
}

projects_in: fn(store: db.Store) -> db.Table<ProjectID, Project> Never! {
    store.table("projects", key: project_key)
}

members_in: fn(store: db.Store) -> db.Table<MemberKey, Member> Never! {
    store.table("members", key: member_key)
}

invites_in: fn(store: db.Store) -> db.Table<InviteKey, Invite> Never! {
    store.table("invites", key: invite_key)
}
```

`notices.jet` says where a failed button goes. Every handler has the type
`fn() app.Notice!` (4.10), so a handler that calls `Crew` needs this
conversion. It sits in `Logic` because Jet allows a conversion only in the
package that defines one of its two types.

`crewboard/Logic/notices.jet`

```jet
// Proposed, not built (#6445 app.Notice).
use {
    app
}

// Where a failed button, menu item or swipe goes. Each handler has the type
// fn() app.Notice!, so this conversion must exist for a handler to call Crew.
// It lives beside TaskError: Jet allows a conversion only in the package that
// defines one of its two types.
// The frame shows the notice beside the control and announces it to screen
// readers. A refused write that the screen had predicted is rolled back first.
impl TaskError -> app.Notice {
    if self == {
        .Unreachable(_) -> app.Notice{text: "Can't reach Crewboard. Try again.", retry: true}
        .Conflict(_) -> app.Notice{text: "Someone else changed this task. It now shows their version."}
        .NotAllowed -> app.Notice{text: "You don't have permission to do that."}
        .NotFound -> app.Notice{text: "That task no longer exists."}
        .SignedOut -> app.Notice{text: "Sign in again to continue."}
        .UpdateRequired -> app.Notice{text: "Update Crewboard to continue."}
        .BadFile(why) -> app.Notice{text: why}
        .LastOwner -> app.Notice{text: "A project needs an owner. Make someone else an owner first."}
        .Invalid(_) | .Storage(_) -> app.Notice{text: "Couldn't save. Try again.", retry: true}
    }
}
```

`crew.jet` is the service. Its fields are its tables and its file storage.
Each table names its key type: `db.Table<TaskID, Task>` (#6822).

- **Endpoints.** `#App.SignedIn` methods need a session. Inside one,
  `app.caller()` is the caller's account, not an option: the compiler
  accepts the call only on paths that start at a signed-in method or a
  test's `as_caller` (#6445 c4).
- **Transactions.** Each method that writes is one serializable
  transaction, retried when it loses a race (#6445 c5). The version check in
  `update_task` and `move_task` and the write that follows are therefore one
  step. A method called from another method, an endpoint included, joins
  the caller's transaction. `import_tasks` takes a `TaskBatch` of up to 500
  tasks and adds them all or none: each `create_task` it calls runs in its
  transaction. The webhook closes every matching task, all or none.
- **Queries.** `my_projects` makes two queries whatever the number of
  projects: the projects, and one `count_by` over open tasks. `search` uses
  the tasks table's full-text index (#6827). The row policies narrow both to
  the caller's projects. The task and project policies look up the caller's
  member row by key, so the database joins them into the query and the
  count stays two; `limit(50)` counts only rows the policy passed. The
  invites policy and the members policy's insert are not key lookups; they
  run per row, memoized, on the few rows an invitation or a join touches
  (4.2).
- **Validation.** Each endpoint takes its form's type: `create_project`
  takes a `ProjectDraft`, `invite` an `InviteDraft`, `import_tasks` a
  `TaskBatch`, all in `model.jet`. The decoder runs each type's `validate`
  block, so a crafted call meets the same rules as the form, and no rule is
  written twice (CL2). Decoding a list runs each element's block, and a
  failure names its element: `tasks[3].title`.
- **Rules that need the database.** `check_assignee` refuses an assignee who
  is not in the project with `app.field_error(at: "assignee", ...)`, which
  the form shows beside the Assignee field (#6831). In `import_tasks` the
  same refusal names its row, `tasks[3].assignee` (`in_row`).
- **Files.** `attach` names its check, `#App.UploadCheck(method:
  "may_attach")`. The task ID travels in the URL, so after the session
  check and the size cap (413) the framework runs `may_attach` as the
  caller, under the row policies, before it reads the body: a caller who
  cannot edit the task gets 403 and the body is never read (#6813 c3). Only
  then does the framework spool the body to a staging file, before
  `attach`'s transaction starts, so the transaction never waits on the
  network and a retry reads the same bytes (#6813 c5). In the transaction
  `attach` checks the role again, since it may have changed meanwhile. Then
  `files.keep` checks the first bytes against the allow list (415), names
  the blob itself and moves it into storage, tied to the transaction: a
  rollback deletes it. A staged file lives no longer than its request, and
  one that a crash leaves behind is deleted when the server next starts.
  The client's file name is kept only to show.
- **Accounts.** `create_project` inserts the project, naming the caller as
  `created_by`, and makes the caller its first owner in the same
  transaction; the creator rule in the policies admits both inserts. `invite`
  records an invitation to a lowercased email address, which lapses in 14
  days; only an owner may, by the invites policy. `accept_invite` adds the
  member row, which the members policy admits only when an invitation to the
  caller's confirmed address grants that role and has not lapsed, and then
  deletes the invitation. `auth.verified_email` returns the address
  lowercased too. No token travels in the mail. A nightly
  `drop_expired_invites` deletes lapsed invitations.
- **Membership.** An owner changes a role with `set_role` and removes a
  member with `remove_member`; any member leaves with `leave`. Each states
  what it does to the rest (#6826 c5):
  - **the last owner:** `keep_an_owner` refuses to demote or remove a
    project's only owner (`TaskError.LastOwner`);
  - **open live queries:** the member row is in the read set of every query
    whose policy read it, so deleting it re-runs the removed member's open
    screens of the project, and the policy refuses them;
  - **assignments:** a job, `unassign`, clears the removed member's
    assignee fields as System, unless the account has joined again since;
  - **jobs:** a job runs as System, which no policy limits, so
    `tell_assignee` and `remind_due_today` check that the assignee is still
    a member before they push.
- **Work after commit.** `#App.Job` methods run on the server after the
  transaction that queued them commits (`app.enqueue`), as `db.Who.System`,
  retried up to `retries`. A write that rolls back queues nothing. The body
  given to `app.enqueue` must be one call of a job of the same service with
  codable arguments; the arguments are encoded at that point, so the outbox
  row holds the job's name and bytes, not a closure. A job runs at least
  once: a run that crashes after its push but before it is recorded runs
  again. Each push and mail a run sends has its own key, made from the
  run's key, the recipient and the `open:` or `link:` value; the push and
  mail transports drop a repeat by it, and two different pushes never share
  one. `tell_assignee` pushes to a new assignee and
  `send_invite` mails the invitation. `#App.Every(cron:)` runs
  `remind_due_today` each morning and `drop_expired_invites` each night
  (#6825 c1, c5).
- **The webhook.** `on_issue_event` is reachable only by GitHub. The
  framework checks the `X-Hub-Signature-256` HMAC in constant time with the
  key that the `#App.SigningKey` method named by `key:` returns, before the
  body is decoded; a wrong or missing signature gets 401 [GH-HOOK]. A
  project has no key until an owner makes one with `new_webhook_key`, which
  stores 256 bits from the operating system's random source and shows them
  to that owner once, with the path to paste into GitHub. While the method
  returns `None`, every delivery gets 401: a missing key never skips the
  check. The framework also drops a delivery whose `X-GitHub-Delivery` ID
  it has already run, because a GitHub signature carries no timestamp and
  does not stop a replay [GH-HOOK-BP] (#6820 c2, c3).
- **Server-only.** Jobs, the schedules, the webhook, the signing key and the
  `priv` helpers are not endpoints. A client call to any of them is a
  compile error.

`crewboard/Logic/crew.jet`

```jet
// Proposed, not built (#6445 app; #6822 typed tables; #6557 secrets).
use {
    app
    auth
    db: core.db
    time: core.time
}

// The service. Only the server entry (Web/web.jet) builds one and mounts it;
// clients reach it as app.Remote<Crew>, which has only the marked methods.
// Safe by default: a method is an endpoint only when it is marked
// #App.SignedIn or #App.Public. Unmarked methods, jobs, webhooks and signing
// keys are server-only, and a client call to one is a compile error.
//
// Each method that writes runs in one serializable transaction, retried when
// it loses a race (section 4.2). Row policies (Logic/policy.jet) decide which
// rows the caller reads and writes; the methods hold only workflow rules.
Crew: struct {
    tasks: db.Table<TaskID, Task>,
    projects: db.Table<ProjectID, Project>,
    members: db.Table<MemberKey, Member>,
    invites: db.Table<InviteKey, Invite>,
    attachments: db.Table<app.BlobID, Attachment>,
    files: app.Blobs,

    // Two queries, whatever the number of projects: the projects and tasks
    // policies are key lookups on members, which the database joins in.
    #App.SignedIn
    my_projects: fn(self) -[DB.Read]> [ProjectCard] TaskError! {
        open: self.tasks.query().filter(fn(t: Task) { t.status != Status.Done }).count_by(fn(t: Task) { t.project })
        out := [ProjectCard]{}
        loop p in self.projects.query().collect() {
            out.push(ProjectCard{id: p.id, name: p.name, open_tasks: open.of(p.id)})
        }
        Ok(out)
    }

    #App.SignedIn
    board: fn(self, project: ProjectID) -[DB.Read]> Board TaskError! {
        found: self.projects.get(project) ?? return Err(TaskError.NotFound)
        tasks: self.tasks.query().filter(fn(t: Task) { t.project == project }).collect()
        Ok(Board{
            project: ProjectCard{id: found.id, name: found.name, open_tasks: open_count(tasks)},
            tasks: tasks,
            members: self.members_of(project),
            role: self.role_in(project),
        })
    }

    #App.SignedIn
    task: fn(self, id: TaskID) -[DB.Read]> TaskPage TaskError! {
        task: self.tasks.get(id) ?? return Err(TaskError.NotFound)
        Ok(TaskPage{
            task: task,
            members: self.members_of(task.project),
            attachments: self.attachments.query().filter(fn(a: Attachment) { a.task == id }).collect(),
            role: self.role_in(task.project),
        })
    }

    // Through the tasks table's full-text index, in the caller's projects.
    #App.SignedIn
    search: fn(self, words: String) -[DB.Read]> [Task] TaskError! {
        Ok(self.tasks.search(words).limit(50).collect())
    }

    // Open tasks assigned to the caller.
    #App.SignedIn
    mine: fn(self) -[DB.Read]> [Task] TaskError! {
        me: app.caller()
        Ok(self.tasks.query().filter(fn(t: Task) { is_open_for(t, me) }).collect())
    }

    // The client makes the ID (logic.new_task_id), so an offline create can
    // open its task at once. A taken ID is refused by `add`: Exists when the
    // caller can read that task, NotAllowed when it cannot.
    #App.SignedIn
    create_task: fn(self, id: TaskID, project: ProjectID, draft: TaskDraft) -[DB.Read, DB.Write]> Task TaskError! {
        self.check_assignee(project, draft)
        task: self.tasks.add(Task{
            id: id,
            project: project,
            title: draft.title.trim(),
            notes: draft.notes,
            status: Status.Todo,
            assignee: draft.assignee,
            due: draft.due,
            version: 1,
        })
        self.after_assign(task, None)
        Ok(task)
    }

    // Many tasks in one transaction: all of them or none. Each create_task
    // call joins this transaction. TaskBatch's validate block caps the batch
    // at 500, and the decoder runs each TaskDraft's, naming its row
    // (`tasks[3].title`); in_row gives a refusal from create_task the same
    // row (`tasks[3].assignee`).
    #App.SignedIn
    import_tasks: fn(self, project: ProjectID, batch: TaskBatch) -[DB.Read, DB.Write]> Int TaskError! {
        loop (index, draft) in batch.tasks {
            _: self.create_task(new_task_id(), project, draft) ?? return Err(in_row(err, index))
        }
        Ok(batch.tasks.len())
    }

    // `version` is the version the editor loaded. A stale edit gets the
    // current task back, and the editor offers to reload.
    #App.SignedIn
    update_task: fn(self, id: TaskID, version: Int, draft: TaskDraft) -[DB.Read, DB.Write]> Task TaskError! {
        task: self.tasks.get(id) ?? return Err(TaskError.NotFound)
        if task.version != version -> return Err(TaskError.Conflict(task))
        self.check_assignee(task.project, draft)
        saved: self.tasks.put(Task{
            id: task.id,
            project: task.project,
            title: draft.title.trim(),
            notes: draft.notes,
            status: task.status,
            assignee: draft.assignee,
            due: draft.due,
            version: task.version + 1,
        })
        self.after_assign(saved, task.assignee)
        Ok(saved)
    }

    // A move is an edit too: one made from a stale board is a conflict.
    #App.SignedIn
    move_task: fn(self, id: TaskID, version: Int, to: Status) -[DB.Read, DB.Write]> Task TaskError! {
        task: self.tasks.get(id) ?? return Err(TaskError.NotFound)
        if task.version != version -> return Err(TaskError.Conflict(task))
        if !can_move(self.role_in(task.project), task.status, to) -> return Err(TaskError.NotAllowed)
        Ok(self.tasks.put(Task{
            id: task.id,
            project: task.project,
            title: task.title,
            notes: task.notes,
            status: to,
            assignee: task.assignee,
            due: task.due,
            version: task.version + 1,
        }))
    }

    // The framework has already run may_attach as the caller, before it read
    // the body (403 for a caller who can't edit the task), and spooled the
    // body to a staging file before this transaction began, so a retry reads
    // the same bytes. The role is checked again here: it may have changed
    // while the body arrived. `keep` checks the first bytes against `allow`
    // (415), names the blob itself and moves it into storage, tied to this
    // transaction: a rollback deletes it.
    #[App.SignedIn, App.UploadCheck(method: "may_attach")]
    attach: fn(self, task: TaskID, file: app.Upload) -[DB.Read, DB.Write, FS.Write]> Attachment TaskError! {
        found: self.tasks.get(task) ?? return Err(TaskError.NotFound)
        if !can_edit(self.role_in(found.project)) -> return Err(TaskError.NotAllowed)
        blob: self.files.keep(file, allow: [app.Media]{app.Media.Image, app.Media.PDF})
        Ok(self.attachments.add(Attachment{
            blob: blob.id,
            task: task,
            project: found.project,
            name: file.name,
            media: blob.media,
            bytes: blob.bytes,
        }))
    }

    // attach's check. Server-only: the framework runs it as the caller,
    // before the body of an upload is read. Reading the task runs the task
    // policy.
    may_attach: fn(self, task: TaskID) -[DB.Read]> TaskError! {
        found: self.tasks.get(task) ?? return Err(TaskError.NotFound)
        if !can_edit(self.role_in(found.project)) -> return Err(TaskError.NotAllowed)
        Ok()
    }

    // Who has this task open, for the "also viewing" line. The read of the
    // task runs the policy first, so only its project's members learn it, and
    // app.presence counts only accounts whose own read of the task passed.
    #App.SignedIn
    viewers: fn(self, id: TaskID) -[DB.Read]> [auth.AccountID] TaskError! {
        _: self.tasks.get(id) ?? return Err(TaskError.NotFound)
        Ok(app.presence(TaskView{id: id}))
    }

    // The creator becomes the owner, in the same transaction: the projects
    // policy lets any account insert a project that names it as created_by,
    // and the members policy lets that account insert itself as the first
    // owner. ProjectDraft's validate block ran in the decoder. The project
    // has no webhook key yet.
    #App.SignedIn
    create_project: fn(self, draft: ProjectDraft) -[DB.Read, DB.Write]> ProjectCard TaskError! {
        project: self.projects.add(Project{id: ProjectID.from_string(app.new_id()), name: draft.name.trim(), webhook_secret: None, created_by: app.caller()})
        self.members.add(Member{project: project.id, account: app.caller(), name: name_of(app.caller()), role: Role.Owner})
        Ok(ProjectCard{id: project.id, name: project.name, open_tasks: 0})
    }

    // A new 256-bit key for the GitHub webhook, shown to the owner once so it
    // can be pasted into GitHub's settings. It replaces the old key. Only an
    // owner may: the projects policy refuses everyone else's write.
    #App.SignedIn
    new_webhook_key: fn(self, project: ProjectID) -[DB.Read, DB.Write, Rand]> WebhookSetup TaskError! {
        found: self.projects.get(project) ?? return Err(TaskError.NotFound)
        key: app.random_key()
        self.projects.put(Project{id: found.id, name: found.name, webhook_secret: Val(key), created_by: found.created_by})
        Ok(WebhookSetup{path: "/hooks/github/{found.id}", key: key})
    }

    // Only an owner may invite: the invites table's policy refuses others.
    // The mail goes out after the commit. The invitation lapses in 14 days.
    #App.SignedIn
    invite: fn(self, project: ProjectID, draft: InviteDraft) -[DB.Read, DB.Write, Time]> Invite TaskError! {
        found: self.projects.get(project) ?? return Err(TaskError.NotFound)
        sent: self.invites.put(Invite{project: project, project_name: found.name, email: draft.email.trim().lower(), role: draft.role, expires: time.today().add_days(14)})
        self.queue_invite(sent.project, sent.email)
        Ok(sent)
    }

    // Invitations to the caller's confirmed address that have not lapsed.
    #App.SignedIn
    my_invites: fn(self) -[DB.Read, Time]> [Invite] TaskError! {
        email: auth.verified_email(app.caller()) ?? return Ok([Invite]{})
        today: time.today()
        Ok(self.invites.query().filter(fn(i: Invite) { i.email == email && i.expires >= today }).collect())
    }

    // The members table's policy admits this one insert: the row names the
    // caller, and an invitation to the caller's confirmed address grants it
    // and has not lapsed. The date is checked here too, to answer NotFound.
    // The invitation is then used up: the invites policy lets the invited
    // account delete it.
    #App.SignedIn
    accept_invite: fn(self, project: ProjectID) -[DB.Read, DB.Write, Time]> ProjectCard TaskError! {
        email: auth.verified_email(app.caller()) ?? return Err(TaskError.NotAllowed)
        key: InviteKey{project: project, email: email}
        invite: self.invites.get(key) ?? return Err(TaskError.NotFound)
        if invite.expires < time.today() -> return Err(TaskError.NotFound)
        self.members.add(Member{project: project, account: app.caller(), name: email, role: invite.role})
        self.invites.delete(key)
        Ok(ProjectCard{id: project, name: invite.project_name, open_tasks: 0})
    }

    // An owner changes a member's role. The members policy refuses everyone
    // else, a Member making itself Owner included.
    #App.SignedIn
    set_role: fn(self, project: ProjectID, account: auth.AccountID, role: Role) -[DB.Read, DB.Write]> Member TaskError! {
        found: self.members.get(MemberKey{project: project, account: account}) ?? return Err(TaskError.NotFound)
        if found.role == Role.Owner && role != Role.Owner -> self.keep_an_owner(project)
        Ok(self.members.put(Member{project: project, account: account, name: found.name, role: role}))
    }

    // An owner removes a member. Deleting the member row changes the read
    // set of every live query whose policy read it, so the member's open
    // screens of the project run again and are refused at once. The tasks
    // assigned to it lose their assignee in a job, which runs as System: a
    // Viewer who leaves could not write them itself.
    #App.SignedIn
    remove_member: fn(self, project: ProjectID, account: auth.AccountID) -[DB.Read, DB.Write]> Member TaskError! {
        found: self.members.get(MemberKey{project: project, account: account}) ?? return Err(TaskError.NotFound)
        if found.role == Role.Owner -> self.keep_an_owner(project)
        self.members.delete(MemberKey{project: project, account: account})
        app.enqueue(fn() { self.unassign(project, account) })
        Ok(found)
    }

    // Leaving is removing yourself; the members policy lets any member
    // delete its own row, and the last owner can't leave.
    #App.SignedIn
    leave: fn(self, project: ProjectID) -[DB.Read, DB.Write]> Member TaskError! {
        self.remove_member(project, app.caller())
    }

    // A webhook, not an endpoint. Before this runs, the framework checks
    // GitHub's HMAC-SHA256 signature in constant time with the key that the
    // `webhook_secret` method returns (a wrong signature, or a project with no
    // key, gets 401 and the body is never decoded), and drops a delivery
    // whose X-GitHub-Delivery ID it has already run: the ID is stored in this
    // method's transaction and kept for 30 days. A closed issue closes the
    // task with the same title.
    #App.Webhook(path: "/hooks/github/:project", signature: app.Signature.GitHub, key: "webhook_secret")
    on_issue_event: fn(self, project: ProjectID, event: IssueEvent) -[DB.Read, DB.Write]> Int TaskError! {
        if event.action != "closed" -> return Ok(0)
        self.close_from_issue(project, event.issue.title)
    }

    priv close_from_issue: fn(self, project: ProjectID, title: String) -[DB.Read, DB.Write]> Int TaskError! {
        open: self.tasks.query().filter(fn(t: Task) { t.project == project && t.title == title && t.status != Status.Done }).collect()
        loop t in open {
            self.tasks.put(Task{
                id: t.id,
                project: t.project,
                title: t.title,
                notes: t.notes,
                status: Status.Done,
                assignee: t.assignee,
                due: t.due,
                version: t.version + 1,
            })
        }
        Ok(open.len())
    }

    // The key the webhook checks signatures with. Server-only. None, for an
    // unknown project or one with no key yet, means 401.
    #App.SigningKey
    webhook_secret: fn(self, project: ProjectID) -[DB.Read]> #Secret String? {
        found: self.projects.get(project) ?? return None
        found.webhook_secret
    }

    // Jobs run on the server after the transaction that queued them commits,
    // as db.Who.System, at least once. No policy limits System, so a job that
    // pushes checks that the assignee is still in the project.
    #App.Job(retries: 5)
    tell_assignee: fn(self, id: TaskID) -[DB.Read, Net]> TaskError! {
        task: self.tasks.get(id) ?? return Ok()
        who: task.assignee ?? return Ok()
        if !self.is_member(task.project, who) -> return Ok()
        app.notify(to: who, title: "Assigned to you", body: task.title, open: TaskView{id: task.id})
        Ok()
    }

    #App.Job(retries: 5)
    send_invite: fn(self, project: ProjectID, email: String) -[DB.Read, Net]> TaskError! {
        invite: self.invites.get(InviteKey{project: project, email: email}) ?? return Ok()
        app.mail(to: invite.email, subject: "Join {invite.project_name} on Crewboard", body: "You were invited to {invite.project_name}. Sign in or create an account with this address to join.", link: ProjectsView{})
        Ok()
    }

    // Every morning at 08:00 UTC: a push to each assignee, still in the
    // project, with work due today. One push per task, each with its own key
    // (the run's, the assignee and the task), so a re-run replaces each one
    // and pushes about two tasks stay two; the runner spreads them over the
    // minute.
    #App.Every(cron: "0 8 * * *")
    remind_due_today: fn(self) -[DB.Read, Net, Time]> TaskError! {
        today: time.today()
        loop t in self.tasks.query().filter(fn(t: Task) { due_on(t, today) }).collect() {
            who: t.assignee ?? next
            if !self.is_member(t.project, who) -> next
            app.notify(to: who, title: "Due today", body: t.title, open: TaskView{id: t.id})
        }
        Ok()
    }

    // Clears the assignee of a removed member's tasks, unless it has joined
    // the project again since.
    #App.Job(retries: 5)
    unassign: fn(self, project: ProjectID, account: auth.AccountID) -[DB.Read, DB.Write]> TaskError! {
        if self.is_member(project, account) -> return Ok()
        loop t in self.tasks.query().filter(fn(t: Task) { t.project == project && assigned_to(t, account) }).collect() {
            self.tasks.put(Task{
                id: t.id,
                project: t.project,
                title: t.title,
                notes: t.notes,
                status: t.status,
                assignee: None,
                due: t.due,
                version: t.version + 1,
            })
        }
        Ok()
    }

    // Every night: invitations past their date are deleted.
    #App.Every(cron: "0 3 * * *")
    drop_expired_invites: fn(self) -[DB.Read, DB.Write, Time]> TaskError! {
        today: time.today()
        loop i in self.invites.query().filter(fn(i: Invite) { i.expires < today }).collect() {
            self.invites.delete(InviteKey{project: i.project, email: i.email})
        }
        Ok()
    }

    // A rule that needs the database: the assignee must be in the project.
    // The form shows the refusal beside the Assignee field.
    priv check_assignee: fn(self, project: ProjectID, draft: TaskDraft) -[DB.Read]> TaskError! {
        who: draft.assignee ?? return Ok()
        _: self.members.get(MemberKey{project: project, account: who}) ?? return Err(TaskError.Invalid([FieldError]{app.field_error(at: "assignee", reason: "Pick someone in this project.")}))
        Ok()
    }

    // A push to a new assignee, queued in this transaction. enqueue's body is
    // one call of a job of this service: `task.id` is evaluated and encoded
    // here, and nothing else of `task` is kept.
    priv after_assign: fn(self, task: Task, before: auth.AccountID?) Never! {
        if task.assignee != before && task.assignee != None -> app.enqueue(fn() { self.tell_assignee(task.id) })
    }

    priv queue_invite: fn(self, project: ProjectID, email: String) Never! {
        app.enqueue(fn() { self.send_invite(project, email) })
    }

    // A project keeps at least one owner.
    priv keep_an_owner: fn(self, project: ProjectID) -[DB.Read]> TaskError! {
        owners: self.members.query().filter(fn(m: Member) { m.project == project && m.role == Role.Owner }).count()
        if owners < 2 -> return Err(TaskError.LastOwner)
        Ok()
    }

    priv is_member: fn(self, project: ProjectID, account: auth.AccountID) -[DB.Read]> Bool TaskError! {
        _: self.members.get(MemberKey{project: project, account: account}) ?? return Ok(false)
        Ok(true)
    }

    priv members_of: fn(self, project: ProjectID) -[DB.Read]> [Member] TaskError! {
        Ok(self.members.query().filter(fn(m: Member) { m.project == project }).collect())
    }

    priv role_in: fn(self, project: ProjectID) -[DB.Read]> Role TaskError! {
        found: self.members.get(MemberKey{project: project, account: app.caller()}) ?? return Err(TaskError.NotAllowed)
        Ok(found.role)
    }
}

// Opens the tables and attaches their row policies. Only the server entry
// calls it.
open_crew: fn(path: String) -[DB.Read, DB.Write]> Crew TaskError! {
    store: db.open(path)
    Ok(Crew{
        tasks: store.table("tasks", key: task_key, search: Val(task_text)).guard(task_policy),
        projects: projects_in(store).guard(project_policy),
        members: members_in(store).guard(member_policy),
        invites: invites_in(store).guard(invite_policy),
        attachments: store.table("attachments", key: attachment_key).guard(attachment_policy),
        files: app.blobs("crewboard-files"),
    })
}

task_key: fn(t: Task) -> TaskID Never! { t.id }
task_text: fn(t: Task) -> String Never! { "{t.title} {t.notes}" }
project_key: fn(p: Project) -> ProjectID Never! { p.id }
member_key: fn(m: Member) -> MemberKey Never! { MemberKey{project: m.project, account: m.account} }
invite_key: fn(i: Invite) -> InviteKey Never! { InviteKey{project: i.project, email: i.email} }
attachment_key: fn(a: Attachment) -> app.BlobID Never! { a.blob }

due_on: fn(task: Task, day: time.LocalDate) -> Bool Never! {
    if task.due == {
        .Val(due) -> due == day && task.status != Status.Done
        .None -> false
    }
}

open_count: fn(tasks: [Task]) -> Int Never! {
    count := 0
    loop t in tasks {
        if t.status != Status.Done -> count += 1
    }
    count
}

name_of: fn(account: auth.AccountID) -> String Never! {
    auth.verified_email(account) ?? "member"
}

is_open_for: fn(task: Task, account: auth.AccountID) -> Bool Never! {
    assigned_to(task, account) && task.status != Status.Done
}

assigned_to: fn(task: Task, account: auth.AccountID) -> Bool Never! {
    if task.assignee == {
        .Val(who) -> who == account
        .None -> false
    }
}

// A refusal of one imported row: each field path gains the row's index
// (`tasks[3].assignee`), as the decoder's paths do.
in_row: fn(error: TaskError, index: Int) -> TaskError Never! {
    if error == {
        .Invalid(fields) -> TaskError.Invalid(loop f in fields -> FieldError{path: "tasks[{index}].{f.path}", reason: f.reason})
        .SignedOut | .NotFound | .NotAllowed | .Conflict(_) | .BadFile(_) | .LastOwner | .Unreachable(_) | .UpdateRequired | .Storage(_) -> error
    }
}
```

The tests run the rules on plain values and the service on an in-memory
database. They check a stale edit, the assignee rule, that an outsider sees
nothing through `search` or `my_projects`, and that the server-only methods
are not endpoints.

`crewboard/Logic/logic_test.jet`

```jet
// The rules tests use only plain values. The Crew tests are proposed, not
// built (#6445 app.as_caller, app.endpoints; D-DBPOLICY1 policies).
use {
    app
    auth
}

// A fixed UUIDv7, as every ID is (model.jet).
launch: fn() -> ProjectID Never! { ProjectID.from_string("0193a5e0-7c1a-7000-8000-000000000001") }

sample: fn(status: Status, title: String) -> Task Never! {
    Task{id: new_task_id(), project: launch(), title: title, notes: "", status: status, assignee: None, due: None, version: 1}
}

#Test("a viewer can't move a task") {
    assert(!can_move(Role.Viewer, Status.Todo, Status.Doing))
}

#Test("only an owner reopens finished work") {
    assert(!can_move(Role.Member, Status.Done, Status.Todo))
    assert(can_move(Role.Owner, Status.Done, Status.Todo))
}

#Test("the board filter matches every word, in any case") {
    tasks: [Task]{sample(Status.Todo, "Fix login page"), sample(Status.Todo, "Write docs")}
    found: matching(tasks, "LOGIN fix")
    assert_eq(found.len(), 1)
    assert_eq(found[0].title, "Fix login page")
}

#Test("a stale edit gets the current task back") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    seed(crew, owner)
    app.as_caller(owner, fn() {
        task: crew.create_task(new_task_id(), launch(), TaskDraft{title: "Ship it"}) ?? panic("create")
        crew.update_task(task.id, task.version, TaskDraft{title: "Ship it today"}) ?? panic("first edit")
        stale: if crew.update_task(task.id, task.version, TaskDraft{title: "Ship it now"}) == {
            .Err(.Conflict(_)) -> true
            .Err(_) | .Ok(_) -> false
        }
        assert(stale)
    })
}

// The row policy, not the method, keeps other projects' tasks out.
#Test("an outsider sees no task of the project") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    outsider: app.test_account("eve@example.com")
    seed(crew, owner)
    add_task(crew, owner)
    app.as_caller(outsider, fn() {
        assert_eq((crew.search("ship") ?? panic("search")).len(), 0)
        assert_eq((crew.my_projects() ?? panic("projects")).len(), 0)
    })
}

// A taken ID in a project the caller can't read is refused as NotAllowed, so
// the outsider can't tell it is taken.
#Test("an outsider can't probe for a task ID") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    outsider: app.test_account("eve@example.com")
    seed(crew, owner)
    taken: new_task_id()
    add_task_with_id(crew, owner, taken)
    app.as_caller(outsider, fn() {
        mine: crew.create_project(ProjectDraft{name: "Mine"}) ?? panic("project")
        refused: if crew.create_task(taken, mine.id, TaskDraft{title: "Probe"}) == {
            .Err(.NotAllowed) -> true
            .Err(_) | .Ok(_) -> false
        }
        assert(refused)
    })
}

#Test("creating a project makes the caller its owner") {
    crew: open_crew(":memory:") ?? panic("open")
    ada: app.test_account("ada@example.com")
    app.as_caller(ada, fn() {
        made: crew.create_project(ProjectDraft{name: "Launch"}) ?? panic("project")
        assert_eq((crew.board(made.id) ?? panic("board")).role, Role.Owner)
    })
}

// The members policy judges an update on the stored row too, where the
// caller is a Member, not an owner.
#Test("a member can't make itself an owner") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    member: app.test_account("bob@example.com")
    seed(crew, owner)
    join(crew, owner, member)
    app.as_caller(member, fn() {
        refused: if crew.set_role(launch(), app.caller(), Role.Owner) == {
            .Err(.NotAllowed) -> true
            .Err(_) | .Ok(_) -> false
        }
        assert(refused)
    })
}

#Test("the last owner can't leave") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    seed(crew, owner)
    app.as_caller(owner, fn() {
        refused: if crew.leave(launch()) == {
            .Err(.LastOwner) -> true
            .Err(_) | .Ok(_) -> false
        }
        assert(refused)
    })
}

// After leaving, the project's rows are out of every read.
#Test("a member who leaves sees nothing of the project") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    member: app.test_account("bob@example.com")
    seed(crew, owner)
    join(crew, owner, member)
    app.as_caller(member, fn() {
        _: crew.create_task(new_task_id(), launch(), TaskDraft{title: "Ship it", assignee: Val(app.caller())}) ?? panic("create")
        _: crew.leave(launch()) ?? panic("leave")
        assert_eq((crew.mine() ?? panic("mine")).len(), 0)
        assert_eq((crew.search("ship") ?? panic("search")).len(), 0)
        assert_eq((crew.my_projects() ?? panic("projects")).len(), 0)
    })
}

#Test("an accepted invitation is used up") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    member: app.test_account("bob@example.com")
    seed(crew, owner)
    join(crew, owner, member)
    app.as_caller(member, fn() {
        assert_eq((crew.my_invites() ?? panic("invites")).len(), 0)
    })
}

add_task: fn(crew: Crew, owner: auth.AccountID) Never! {
    add_task_with_id(crew, owner, new_task_id())
}

add_task_with_id: fn(crew: Crew, owner: auth.AccountID, id: TaskID) Never! {
    app.as_caller(owner, fn() {
        _: crew.create_task(id, launch(), TaskDraft{title: "Ship it", assignee: Val(app.caller())}) ?? panic("create")
    })
}

// The owner invites `member`, who accepts.
join: fn(crew: Crew, owner: auth.AccountID, member: auth.AccountID) Never! {
    app.as_caller(owner, fn() {
        _: crew.invite(launch(), InviteDraft{email: "Bob@Example.com", role: Role.Member}) ?? panic("invite")
    })
    app.as_caller(member, fn() {
        _: crew.accept_invite(launch()) ?? panic("accept")
    })
}

#Test("the assignee must be in the project") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    outsider: app.test_account("eve@example.com")
    seed(crew, owner)
    app.as_caller(owner, fn() {
        refused: if crew.create_task(new_task_id(), launch(), TaskDraft{title: "Ship it", assignee: Val(outsider)}) == {
            .Err(.Invalid(fields)) -> fields[0].path == "assignee"
            .Err(_) | .Ok(_) -> false
        }
        assert(refused)
    })
}

// An import's refusal names the row it applies to.
#Test("an import names the refused row") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    outsider: app.test_account("eve@example.com")
    seed(crew, owner)
    app.as_caller(owner, fn() {
        batch: TaskBatch{tasks: [TaskDraft]{TaskDraft{title: "One"}, TaskDraft{title: "Two", assignee: Val(outsider)}}}
        refused: if crew.import_tasks(launch(), batch) == {
            .Err(.Invalid(fields)) -> fields[0].path == "tasks[1].assignee"
            .Err(_) | .Ok(_) -> false
        }
        assert(refused)
    })
}

// The creator rule is tied to the project row: another account can't make
// itself the owner of a project that has no members.
#Test("an outsider can't claim a project with no members") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    outsider: app.test_account("eve@example.com")
    crew.projects.put(Project{id: launch(), name: "Launch", webhook_secret: None, created_by: owner}) ?? panic("project")
    app.as_caller(outsider, fn() {
        refused: if crew.members.add(Member{project: launch(), account: app.caller(), name: "Eve", role: Role.Owner}) == {
            .Err(.Denied) -> true
            .Err(_) | .Ok(_) -> false
        }
        assert(refused)
    })
}

// An upload's check runs before the body is read: an outsider is refused.
#Test("an outsider's upload is refused before it is read") {
    crew: open_crew(":memory:") ?? panic("open")
    owner: app.test_account("ada@example.com")
    outsider: app.test_account("eve@example.com")
    seed(crew, owner)
    id: new_task_id()
    add_task_with_id(crew, owner, id)
    app.as_caller(outsider, fn() {
        refused: if crew.may_attach(id) == {
            .Err(.NotFound) | .Err(.NotAllowed) -> true
            .Err(_) | .Ok(_) -> false
        }
        assert(refused)
    })
}

#Test("server-only methods are not endpoints") {
    assert(!app.endpoints<Crew>().contains("on_issue_event"))
    assert(!app.endpoints<Crew>().contains("webhook_secret"))
    assert(!app.endpoints<Crew>().contains("tell_assignee"))
    assert(!app.endpoints<Crew>().contains("remind_due_today"))
    assert(!app.endpoints<Crew>().contains("drop_expired_invites"))
    assert(!app.endpoints<Crew>().contains("may_attach"))
}

seed: fn(crew: Crew, owner: auth.AccountID) Never! {
    crew.projects.put(Project{id: launch(), name: "Launch", webhook_secret: None, created_by: owner}) ?? panic("project")
    crew.members.put(Member{project: launch(), account: owner, name: "Ada", role: Role.Owner}) ?? panic("member")
}
```

### 3.3 Screens: the client-safe graph and the one route set

`shell.jet` builds the graph. It holds no database: `.api<logic.Crew>("/api",
max_upload:)` says the screens call a `Crew` at `/api`, and the graph gives
each screen that takes an `app.Remote<logic.Crew>` a client for it. Only this
graph adds screens. A `.screen` call in an entry package is a compile error
(#6445 c6), so every target knows every route, and a push or a mail link can
open any of them. A screen's state struct is its route:
`BoardView{project: id, q: "login"}` is `/projects/<id>?q=login`.

`crewboard/Screens/shell.jet`

```jet
// Proposed, not built (#6445 app graph; #6552 c8 typed screens).
use {
    app
    kit
    logic
}

// The client-safe graph and the one route set. Every target runs this graph,
// so it holds no database and no secret, and only this graph adds screens: a
// `.screen` call in an entry package is a compile error. An entry adds menus,
// tabs and other plugins over these routes. The routes are the screen states
// in Logic/routes.jet. `.api<logic.Crew>` declares the service and its
// address; the graph passes app.Remote<logic.Crew> to every screen that
// takes one. Only the server entry mounts a real Crew (Web/web.jet).
crewboard: fn() -> app.App Never! {
    app.new(title: "Crewboard")
        .theme(crew_theme())
        .api<logic.Crew>("/api", max_upload: 10000000)
        .sign_in(logic.SignInView{})
        .screen("/", projects, signed_in: true)
        .screen("/projects/:project", board, signed_in: true)
        .screen("/projects/:project/new", new_task, signed_in: true)
        .screen("/tasks/:id", task_detail, signed_in: true)
        .screen("/search", search, signed_in: true)
        .screen("/mine", mine, signed_in: true)
        .screen("/sign-in", sign_in)
        // Sign-up, email confirmation, password reset, passkeys and two-factor
        // codes, from the auth battery the server entry configures.
        .with(kit.account_flows())
        .error(problem)
        .not_found(lost)
}
```

The projects screen is a live query. `app.state` turns it into a `Loadable`:
`Idle`, `Loading`, `Loaded(value)` or `Failed(error)`. `failed` is the one
place a screen turns a `TaskError` into words for a whole screen. The screen
also lists invitations to the caller's confirmed address, with the same
`failed` view when that read fails, and creates projects through the
`logic.ProjectDraft` form, whose `validate` block the server's decoder runs
again. The badge text is a message with a plural form (D-I18N1); data goes
through `ui.verbatim` and is never translated.

`crewboard/Screens/projects.jet`

```jet
// Proposed, not built (#6445 app; #6447 ui; kit views per D-VIEW-SET1).
use {
    app
    kit
    logic
    ui
}

projects: fn(crew: app.Remote<logic.Crew>, view: logic.ProjectsView) -> ui.Node {
    if app.state(app.live(fn() { crew.my_projects() })) == {
        .Idle | .Loading -> kit.spinner()
        .Failed(error) -> failed(error)
        .Loaded(cards) -> ui.column([
            kit.heading("Your projects"),
            invitations(crew),
            if cards.len() == 0 -> kit.empty("No projects yet. Make one, or accept an invitation.") else -> kit.list(cards, key: fn(card: logic.ProjectCard) { card.id }, row: project_row),
            new_project(crew),
        ])
    }
}

project_row: fn(card: logic.ProjectCard) -> ui.Node {
    kit.link(logic.BoardView{project: card.id}, ui.row([
        ui.text(ui.verbatim(card.name)),
        kit.badge(ui.text("{card.open_tasks} open tasks", one: "1 open task", plural: card.open_tasks)),
    ]))
}

// logic.ProjectDraft's validate block runs here as the person types and
// again in the server's decoder.
new_project: fn(crew: app.Remote<logic.Crew>) -> ui.Node {
    kit.form(logic.ProjectDraft{}, submit: "Create project", on_submit: fn(draft: logic.ProjectDraft) {
        made: crew.create_project(draft)
        app.go(logic.BoardView{project: made.id})
    }, on_error: fn(error: logic.TaskError) { save_failed(error) }, fields: fn(f: kit.Form<logic.ProjectDraft>) {
        f.text(f.name, label: "New project")
    })
}

// Invitations to this account's confirmed address. A failed load says so,
// as every screen does.
invitations: fn(crew: app.Remote<logic.Crew>) -> ui.Node {
    if app.state(app.live(fn() { crew.my_invites() })) == {
        .Loaded(found) -> kit.list(found, key: fn(i: logic.Invite) { i.project }, row: fn(i: logic.Invite) { invitation(crew, i) })
        .Failed(error) -> failed(error)
        .Idle | .Loading -> ui.empty()
    }
}

invitation: fn(crew: app.Remote<logic.Crew>, invite: logic.Invite) -> ui.Node {
    ui.row([
        ui.text("You are invited to {invite.project_name}."),
        kit.button("Join", on_click: fn() { join(crew, invite.project) }),
    ])
}

join: fn(crew: app.Remote<logic.Crew>, project: logic.ProjectID) logic.TaskError! {
    made: crew.accept_invite(project)
    app.go(logic.BoardView{project: made.id})
    Ok()
}

// The one place a screen turns a TaskError into words for a whole screen.
failed: fn(error: logic.TaskError) -> ui.Node {
    if error == {
        .SignedOut -> kit.link(logic.SignInView{}, ui.text("Sign in again"))
        .NotFound -> kit.empty("That item no longer exists.")
        .NotAllowed -> kit.empty("You don't have access to this.")
        .Unreachable(_) -> kit.notice("Can't reach Crewboard. Retrying.")
        .UpdateRequired -> kit.notice("This version of Crewboard is too old. Update it to continue.")
        .Invalid(_) | .Conflict(_) | .BadFile(_) | .LastOwner | .Storage(_) -> kit.notice("Something went wrong. Try again.")
    }
}
```

The board shows three columns on a wide window and one column with a picker
on a phone. The filter box replaces the address, so Back does not step
through every keystroke. Rows are keyed by task ID, so a card keeps its focus
and local state when it moves or the filter changes. A move button appears
only when `can_move` allows it, and it sends the version the board shows, so
a move from a stale board is a conflict. Everyone sees the members panel;
an owner also sees the invite form and the webhook key.

`crewboard/Screens/board.jet`

```jet
// Proposed, not built (#6445 app; #6447 ui; #6817 styling; #6555 predicted writes).
use {
    app
    kit
    logic
    ui
}

board: fn(crew: app.Remote<logic.Crew>, view: logic.BoardView) -> ui.Node {
    if app.state(app.live(fn() { crew.board(view.project) })) == {
        .Idle | .Loading -> kit.spinner()
        .Failed(error) -> failed(error)
        .Loaded(data) -> ui.column([
            ui.row([
                kit.heading(ui.verbatim(data.project.name)),
                kit.search_field(view.q, label: "Filter tasks", on_change: fn(q: String) { app.replace(logic.BoardView{project: view.project, q: q}) }),
                if logic.can_edit(data.role) -> kit.link_button("New task", to: logic.NewTask{project: view.project}) else -> ui.empty(),
            ]),
            // Three columns side by side; one column with a picker when narrow.
            ui.by_width(
                compact: narrow_board(crew, data, view.q),
                regular: wide_board(crew, data, view.q),
            ),
            members_panel(crew, data),
            if data.role == logic.Role.Owner -> ui.column([invite_form(crew, view.project), webhook_key(crew, view.project)]) else -> ui.empty(),
        ])
    }
}

wide_board: fn(crew: app.Remote<logic.Crew>, data: logic.Board, q: String) -> ui.Node {
    ui.row([
        column(crew, data, q, logic.Status.Todo),
        column(crew, data, q, logic.Status.Doing),
        column(crew, data, q, logic.Status.Done),
    ], gap: ui.px(12))
}

narrow_board: fn(crew: app.Remote<logic.Crew>, data: logic.Board, q: String) -> ui.Node {
    shown := ui.local(logic.Status.Todo)
    ui.column([
        kit.segmented(shown, [logic.Status.Todo, logic.Status.Doing, logic.Status.Done], label: logic.status_label),
        column(crew, data, q, shown.get()),
    ])
}

// Rows are keyed by task ID, so a card keeps its focus and state as it moves.
column: fn(crew: app.Remote<logic.Crew>, data: logic.Board, q: String, status: logic.Status) -> ui.Node {
    tasks: logic.column(logic.matching(data.tasks, q), status)
    ui.column([
        kit.heading(logic.status_label(status), level: 3),
        kit.list(tasks, key: fn(task: logic.Task) { task.id }, row: fn(task: logic.Task) { card(crew, data.role, task) }),
    ], style: ui.style(column_style))
}

// A failed move rolls the card back and shows the notice beside the button.
card: fn(crew: app.Remote<logic.Crew>, role: logic.Role, task: logic.Task) -> ui.Node {
    next: logic.next_status(task.status)
    ui.column([
        kit.link(logic.TaskView{id: task.id}, ui.text(ui.verbatim(task.title))),
        due_line(task),
        if logic.can_move(role, task.status, next) -> kit.button("Move to {logic.status_label(next)}", on_click: fn() { _: crew.move_task(task.id, task.version, next) }) else -> ui.empty(),
    ], style: ui.style(card_style))
}

due_line: fn(task: logic.Task) -> ui.Node {
    if task.due == {
        .None -> ui.empty()
        .Val(day) -> ui.text(ui.date(day, .Short), style: if day < ui.today() && task.status != logic.Status.Done -> ui.style(overdue_style) else -> ui.style(muted_style))
    }
}
```

The members panel lists who is in the project. Owners change a role and
remove a member there; anyone can leave. Owners also invite by email and
make the GitHub webhook key, which the server returns once, to them. The
forms take `logic.InviteDraft`, so the server's decoder runs the same
`validate` block, which refuses an Owner invitation.

`crewboard/Screens/members.jet`

```jet
// Proposed, not built (#6445 app; kit forms per D-VIEW-SET1).
use {
    app
    kit
    logic
    ui
}

// Who is in the project. Owners change roles and remove members; anyone can
// leave. The members policy decides who may do which, so these buttons only
// spare people a refusal.
members_panel: fn(crew: app.Remote<logic.Crew>, data: logic.Board) -> ui.Node {
    ui.column([
        kit.heading("Members", level: 2),
        kit.list(data.members, key: fn(m: logic.Member) { m.account }, row: fn(m: logic.Member) { member_row(crew, data.role, m) }),
        kit.button("Leave project", on_click: fn() { leave(crew, data.project.id) }),
    ])
}

member_row: fn(crew: app.Remote<logic.Crew>, role: logic.Role, m: logic.Member) -> ui.Node {
    ui.row([
        ui.text(ui.verbatim(m.name)),
        ui.text(role_label(m.role)),
        if role == logic.Role.Owner -> owner_tools(crew, m) else -> ui.empty(),
    ])
}

owner_tools: fn(crew: app.Remote<logic.Crew>, m: logic.Member) -> ui.Node {
    next: if m.role == logic.Role.Owner -> logic.Role.Member else -> logic.Role.Owner
    ui.row([
        kit.button("Make {role_label(next)}", on_click: fn() { _: crew.set_role(m.project, m.account, next) }),
        kit.button("Remove", on_click: fn() { _: crew.remove_member(m.project, m.account) }),
    ])
}

leave: fn(crew: app.Remote<logic.Crew>, project: logic.ProjectID) logic.TaskError! {
    _: crew.leave(project)
    app.go(logic.ProjectsView{})
    Ok()
}

// Owners invite by email. The server sends the mail after the invitation is
// stored; the invited person joins from the projects screen once their
// account has confirmed that address. logic.InviteDraft's validate block runs
// here and again in the server's decoder.
invite_form: fn(crew: app.Remote<logic.Crew>, project: logic.ProjectID) -> ui.Node {
    kit.form(logic.InviteDraft{}, submit: "Invite", on_submit: fn(draft: logic.InviteDraft) {
        _: crew.invite(project, draft)
    }, on_error: fn(error: logic.TaskError) { save_failed(error) }, fields: fn(f: kit.Form<logic.InviteDraft>) {
        ui.row([
            f.email(f.email, label: "Invite by email"),
            f.pick(f.role, label: "Role", options: [logic.Role.Member, logic.Role.Viewer], show: role_label, value: fn(r: logic.Role) { r.copy() }),
        ])
    })
}

// The GitHub webhook's key. The server makes it and returns it once, to an
// owner; a new key replaces the old one, and GitHub's settings take the new
// path and key.
webhook_key: fn(crew: app.Remote<logic.Crew>, project: logic.ProjectID) -> ui.Node {
    shown := ui.local("")
    text: shown.get()
    ui.column([
        kit.button("New webhook key", on_click: fn() { make_key(crew, project, shown) }),
        if text == "" -> ui.empty() else -> ui.text(ui.verbatim(text)),
    ])
}

make_key: fn(crew: app.Remote<logic.Crew>, project: logic.ProjectID, shown: ui.Local<String>) logic.TaskError! {
    setup: crew.new_webhook_key(project)
    shown.set("{setup.path} {setup.key}")
    Ok()
}

role_label: fn(role: logic.Role) -> String Never! {
    if role == {
        .Owner -> "Owner"
        .Member -> "Member"
        .Viewer -> "Viewer"
    }
}
```

The task screen is an editor for owners and members and a read-only page for
viewers. The form sends the version it loaded, so a stale edit gets
`Conflict` back instead of overwriting someone's work. Attachments download
through `app.blob_url`, which runs the attachment's row policy (#6813 c4).
"Also viewing" calls `Crew.viewers`, which reads the task first, so only the
project's members learn who is looking, and `app.presence` counts only the
accounts whose own read of the task passed: an outsider who opens the link
is refused and never counted. If the line can't load, it says so in muted
text and the rest of the screen still works.

`crewboard/Screens/task.jet`

```jet
// Proposed, not built (#6445 app; #6447 ui; kit forms per D-VIEW-SET1; #6813 files).
use {
    app
    kit
    logic
    ui
}

task_detail: fn(crew: app.Remote<logic.Crew>, view: logic.TaskView) -> ui.Node {
    if app.state(app.live(fn() { crew.task(view.id) })) == {
        .Idle | .Loading -> kit.spinner()
        .Failed(error) -> failed(error)
        .Loaded(page) -> ui.column([
            if logic.can_edit(page.role) -> editor(crew, page) else -> read_only(page.task),
            attachments(crew, page),
            viewing(crew, view.id),
        ])
    }
}

read_only: fn(task: logic.Task) -> ui.Node {
    ui.column([
        kit.heading(ui.verbatim(task.title)),
        ui.text(logic.status_label(task.status), style: ui.style(muted_style)),
        ui.text(ui.verbatim(task.notes)),
    ])
}

// The form validates TaskDraft as the person types. Save runs as a task:
// the button is disabled while it runs and the screen stays responsive. A
// refusal from the server, such as an assignee outside the project, shows
// beside its field.
editor: fn(crew: app.Remote<logic.Crew>, page: logic.TaskPage) -> ui.Node {
    kit.form(logic.draft_of(page.task), submit: "Save", on_submit: fn(draft: logic.TaskDraft) {
        _: crew.update_task(page.task.id, page.task.version, draft)
    }, on_error: fn(error: logic.TaskError) { save_failed(error) }, fields: fn(f: kit.Form<logic.TaskDraft>) {
        ui.column([
            f.text(f.title, label: "Title"),
            f.text_area(f.notes, label: "Notes"),
            f.pick(f.assignee, label: "Assignee", options: page.members, show: fn(m: logic.Member) { m.name }, value: fn(m: logic.Member) { m.account }),
            f.date(f.due, label: "Due"),
        ])
    })
}

// Files on the task. On a phone the picker also offers the camera.
attachments: fn(crew: app.Remote<logic.Crew>, page: logic.TaskPage) -> ui.Node {
    ui.column([
        kit.list(page.attachments, key: fn(a: logic.Attachment) { a.blob }, row: fn(a: logic.Attachment) { kit.download(app.blob_url(a.blob), label: a.name) }),
        if logic.can_edit(page.role) -> attach_button(crew, page.task.id) else -> ui.empty(),
    ])
}

attach_button: fn(crew: app.Remote<logic.Crew>, id: logic.TaskID) -> ui.Node {
    kit.file_button("Attach a file", accept: [app.Media]{app.Media.Image, app.Media.PDF}, max_bytes: 10000000, on_pick: fn(file: app.Upload) { _: crew.attach(id, file) })
}

// "Also here": the others who have this task open. A failed load says so in
// the line's place, quietly: the rest of the screen still works.
viewing: fn(crew: app.Remote<logic.Crew>, id: logic.TaskID) -> ui.Node {
    if app.state(app.live(fn() { crew.viewers(id) })) == {
        .Loaded(others) -> if others.len() > 1 -> ui.text("{others.len() - 1} others have this task open", one: "1 other has this task open", plural: others.len() - 1) else -> ui.empty()
        .Failed(_) -> ui.text("Couldn't load who else is here.", style: ui.style(muted_style))
        .Idle | .Loading -> ui.empty()
    }
}

save_failed: fn(error: logic.TaskError) -> kit.FormReply {
    if error == {
        .Invalid(fields) -> kit.FormReply.Fields(fields)
        .Conflict(_) -> kit.FormReply.Message("Someone changed this task. Reload to see their edit.")
        .Unreachable(_) -> kit.FormReply.Message("Can't reach Crewboard. Your edit is kept; try again.")
        .UpdateRequired -> kit.FormReply.Message("Update Crewboard to save. Your edit is kept.")
        .BadFile(why) -> kit.FormReply.Message(why)
        .LastOwner -> kit.FormReply.Message("A project needs an owner. Make someone else an owner first.")
        .SignedOut | .NotFound | .NotAllowed | .Storage(_) -> kit.FormReply.Message("Couldn't save. Try again.")
    }
}
```

The new-task screen makes the task's ID, so it can open the task at once,
offline too.

`crewboard/Screens/new_task.jet`

```jet
// Proposed, not built (#6445 app; kit forms per D-VIEW-SET1).
use {
    app
    kit
    logic
    ui
}

// The task's ID is made here, so the task screen opens at once, offline
// too: the queued create shows as a predicted task marked "Waiting to send".
new_task: fn(crew: app.Remote<logic.Crew>, view: logic.NewTask) -> ui.Node {
    kit.form(logic.TaskDraft{}, submit: "Create", on_submit: fn(draft: logic.TaskDraft) {
        id: logic.new_task_id()
        _: crew.create_task(id, view.project, draft)
        app.go(logic.TaskView{id: id})
    }, on_error: fn(error: logic.TaskError) { save_failed(error) }, fields: fn(f: kit.Form<logic.TaskDraft>) {
        ui.column([
            f.text(f.title, label: "Title"),
            f.text_area(f.notes, label: "Notes"),
            f.date(f.due, label: "Due"),
        ])
    })
}
```

`crewboard/Screens/search.jet`

```jet
// Proposed, not built (#6445 app; kit views per D-VIEW-SET1).
use {
    app
    kit
    logic
    ui
}

search: fn(crew: app.Remote<logic.Crew>, view: logic.SearchView) -> ui.Node {
    ui.column([
        kit.search_field(view.q, label: "Search tasks", debounce: 200ms, on_change: fn(q: String) { app.replace(logic.SearchView{q: q}) }),
        if view.q.trim().len() < 2 -> kit.empty("Type at least two letters.") else -> results(crew, view.q),
    ])
}

results: fn(crew: app.Remote<logic.Crew>, q: String) -> ui.Node {
    if app.state(app.live(fn() { crew.search(q) })) == {
        .Idle | .Loading -> kit.spinner()
        .Failed(error) -> failed(error)
        .Loaded(found) -> if found.len() == 0 -> kit.empty("No tasks match.") else -> task_list(found)
    }
}

task_list: fn(tasks: [logic.Task]) -> ui.Node {
    kit.list(tasks, key: fn(t: logic.Task) { t.id }, row: fn(t: logic.Task) { kit.link(logic.TaskView{id: t.id}, ui.text(ui.verbatim(t.title))) })
}
```

Mine lists open tasks assigned to the caller. It is a shared screen; the
phone shows it as a tab, and the web and desktop reach it at `/mine`.

`crewboard/Screens/mine.jet`

```jet
// Proposed, not built (#6445 app; kit views per D-VIEW-SET1).
use {
    app
    kit
    logic
    ui
}

// Open tasks assigned to the caller. A swipe (or the row's action menu with
// a mouse or keyboard) marks one done.
mine: fn(crew: app.Remote<logic.Crew>, view: logic.MineView) -> ui.Node {
    if app.state(app.live(fn() { crew.mine() })) == {
        .Idle | .Loading -> kit.spinner()
        .Failed(error) -> failed(error)
        .Loaded(tasks) -> if tasks.len() == 0 -> kit.empty("Nothing assigned to you.") else -> kit.list(tasks, key: fn(t: logic.Task) { t.id }, row: fn(t: logic.Task) { mine_row(crew, t) })
    }
}

mine_row: fn(crew: app.Remote<logic.Crew>, task: logic.Task) -> ui.Node {
    kit.swipe_row(kit.link(logic.TaskView{id: task.id}, ui.text(ui.verbatim(task.title))), actions: [
        kit.swipe_action("Done", run: fn() { _: crew.move_task(task.id, task.version, logic.Status.Done) }),
    ])
}
```

The sign-in form has no confirm field and no copy of the password. The
password is `#Secret String` in `auth.Credentials`; the field shows dots and a
reveal button.

`crewboard/Screens/sign_in.jet`

```jet
// Proposed, not built (#6445 app; auth package per D-AUTH1).
use {
    app
    auth
    kit
    logic
    ui
}

// auth.Credentials holds `email: String` and `password: #Secret String`.
// The password field shows dots, with a button that reveals what was typed.
// kit.account_flows adds the links under the form: create an account, reset
// a password, and sign in with a passkey.
sign_in: fn(view: logic.SignInView) -> ui.Node {
    kit.form(auth.Credentials{}, submit: "Sign in", on_submit: fn(credentials: auth.Credentials) {
        auth.sign_in(credentials)
        app.go_back_or(logic.ProjectsView{})
    }, on_error: fn(error: auth.SignInError) { kit.FormReply.Message(auth.describe(error)) }, fields: fn(f: kit.Form<auth.Credentials>) {
        ui.column([
            f.email(f.email, label: "Email"),
            f.password(f.password, label: "Password", reveal: true),
        ])
    })
}

problem: fn(error: app.Problem) -> ui.Node {
    ui.column([
        kit.heading("Something went wrong"),
        ui.text("The error was logged with id {error.id}."),
        kit.link(logic.ProjectsView{}, ui.text("Back to your projects")),
    ])
}

lost: fn() -> ui.Node {
    ui.column([
        kit.heading("Page not found"),
        kit.link(logic.ProjectsView{}, ui.text("Back to your projects")),
    ])
}
```

The screen tests run the whole graph on a test host with a real `Crew` on an
in-memory database. They drive screens through the same remote boundary a
browser uses.

`crewboard/Screens/screens_test.jet`

```jet
// Proposed, not built (#6445 app.test_host; #6447 ui test driver).
// The test host runs the graph with a real Crew on an in-memory database and
// drives the screens through the same remote boundary a browser uses.
use {
    app
    logic
}

host_with_one_project: fn() -> app.TestHost {
    crew: logic.open_crew(":memory:")
    host: app.test_host(crewboard().mount(crew))
    owner: host.sign_up("ada@example.com")
    crew.projects.put(logic.Project{id: logic.launch(), name: "Launch", webhook_secret: None, created_by: owner})
    crew.members.put(logic.Member{project: logic.launch(), account: owner, name: "Ada", role: logic.Role.Owner})
    host
}

#Test("an empty title shows the field error and sends nothing") {
    host: host_with_one_project() ?? panic("setup")
    screen: host.open(logic.NewTask{project: logic.launch()})
    screen.press("Create")
    assert(screen.shows("Give the task a title."))
    assert_eq(host.calls_sent(), 0)
}

#Test("a crafted request meets the same validation") {
    host: host_with_one_project() ?? panic("setup")
    reply: host.raw_call("create_task", "{{\"id\":\"{app.new_id()}\",\"project\":\"0193a5e0-7c1a-7000-8000-000000000001\",\"draft\":{{\"title\":\"\"}}}}")
    assert_eq(reply.status, 422)
}

// An ID's decoder accepts only UUIDv7 text, so a made-up ID never reaches
// the database.
#Test("an ID that is not a UUIDv7 is refused") {
    host: host_with_one_project() ?? panic("setup")
    reply: host.raw_call("create_task", "{{\"id\":\"t1\",\"project\":\"0193a5e0-7c1a-7000-8000-000000000001\",\"draft\":{{\"title\":\"Ship it\"}}}}")
    assert_eq(reply.status, 422)
}

#Test("moving a card shows it in the next column") {
    host: host_with_one_project() ?? panic("setup")
    screen: host.open(logic.NewTask{project: logic.launch()})
    screen.type("Title", "Ship it")
    screen.press("Create")
    board: host.open(logic.BoardView{project: logic.launch()})
    board.press("Move to Doing")
    assert(board.within("Doing").shows("Ship it"))
}

#Test("signed out, every screen goes to sign-in") {
    crew: logic.open_crew(":memory:") ?? panic("open")
    host: app.test_host(crewboard().mount(crew))
    screen: host.open(logic.BoardView{project: logic.launch()})
    assert(screen.is(logic.SignInView{}))
}
```

`theme.jet` holds the design tokens and styles. Section 6 shows it.

### 3.4 The web entry

The server entry is the only file that opens the database and the account
store. `.mount(crew)` adds the endpoints, the webhook, the jobs and the
schedule that `Crew` declares. `.accounts(...)` turns on email confirmation,
password reset, passkeys, two-factor codes and a sign-in rate limit, and
`.mail(...)` names the transport for the account mails and Crew's own
(#6826). It adds no screen.

`crewboard/Web/web.jet`

```jet
// Proposed, not built (#6445 app; #6820 signed webhooks).
// The server entry: the only file that opens the database and the account
// store. Browsers get the same screens, rendered on the server first. It adds
// no screen.
use {
    app
    auth
    logic
    screens
    server: core.http.server
}

run: fn() logic.TaskError! {
    crew: logic.open_crew("crewboard.db")
    screens.crewboard()
        // Mounts the endpoints, the signed webhook, the jobs and the schedule
        // that Crew declares.
        .mount(crew)
        // The account lifecycle (section 4.7). Its mails, and Crew's, go
        // through the transport below.
        .accounts(auth.sqlite("accounts.db").verify_email().reset_password().passkeys().two_factor().limit_sign_in(per_minute: 5))
        .mail(app.smtp("SMTP_URL"), from: "Crewboard <crew@example.com>")
        .get("/health", fn() { server.response(200, "ok") })
        .serve()
}
```

### 3.5 The desktop entry

The desktop app sets the offline queue, adds a menu bar, opens task screens
in their own windows and exports a board as CSV. The export writes the
status label and the due date in separate columns, and the CSV writer
escapes cells that a spreadsheet would run as formulas (4.15, #6828). A
failed export shows its notice and never stops the app.

`crewboard/Desktop/desktop.jet`

```jet
// Proposed, not built (#6799 menus; #6802 save dialog; #3709 offline queue; #2999 windows).
// The desktop entry. It never mounts Crew: every call goes to `server:`. It
// adds no screen; it adds a menu bar over the shared routes.
use {
    app
    csv: core.encoding.csv
    files: core.files
    logic
    native
    screens
    time: core.time
}

#Codable
CSVRow: struct {
    title: String,
    status: String,
    due: String,
}

#Error
ExportError: enum {
    Board(logic.TaskError),
    Disk(String),
}

impl logic.TaskError -> ExportError {
    ExportError.Board(self)
}

// A failed export shows its notice by the menu bar; it never stops the app.
impl ExportError -> app.Notice {
    if self == {
        .Disk(why) -> app.Notice{text: why}
        .Board(_) -> app.Notice{text: "Couldn't load the board. Try again.", retry: true}
    }
}

run: fn() {
    screens.crewboard()
        .offline(.Queue)
        .with(native.menu_bar(menus))
        .window(width: 1200, height: 800)
        .open_in_window<logic.TaskView>()
        .serve()
}

menus: fn(crew: app.Remote<logic.Crew>) -> [native.Menu] {
    [
        native.menu("File", [
            native.item("New Task", key: "Cmd+N", enabled: app.current<logic.BoardView>() != None, run: new_here),
            native.item("Export Board as CSV…", key: "Cmd+Shift+E", enabled: app.current<logic.BoardView>() != None, run: fn() { export_here(crew) }),
            native.separator(),
            native.quit_item(),
        ]),
        native.edit_menu(),
        native.window_menu(),
    ]
}

new_here: fn() app.Notice! {
    view: app.current<logic.BoardView>() ?? return Ok()
    app.go(logic.NewTask{project: view.project})
    Ok()
}

// csv.to_string writes a text cell that starts with =, +, -, @, a tab or a
// carriage return with a leading apostrophe, so a spreadsheet shows a task
// titled "=HYPERLINK(...)" as text and never runs it (section 4.15).
export_here: fn(crew: app.Remote<logic.Crew>) -[FS.Write, Net]> ExportError! {
    view: app.current<logic.BoardView>() ?? return Ok()
    data: crew.board(view.project)
    path: native.save_file(suggested: "{data.project.name}.csv", types: [native.FileType.CSV]) ?? return Ok()
    rows := [CSVRow]{}
    loop t in data.tasks {
        rows.push(CSVRow{title: t.title, status: logic.status_label(t.status), due: day_text(t)})
    }
    files.write(path, csv.to_string(rows)) ?? return Err(ExportError.Disk("Couldn't write {path}."))
    Ok()
}

day_text: fn(t: logic.Task) -> String {
    if t.due == {
        .None -> ""
        .Val(day) -> time.isoformat_date(day)
    }
}
```

### 3.6 The phone entry

The phone app sets the offline queue and adds three tabs over the shared
routes: Projects, Mine and Search. It opens `https://crew.example.com/tasks/<id>`
in the app when the app is installed, and registers the device for the
pushes that `tell_assignee` and `remind_due_today` send.

`crewboard/Phone/phone.jet`

```jet
// Proposed, not built (#480 phone output; #6800 push notifications; #6804 app links; #3709 offline queue).
// The phone entry. It adds no screen: tabs over the shared routes, links and
// push. It never mounts Crew: every call goes to `server:`.
use {
    logic
    native
    screens
}

run: fn() {
    screens.crewboard()
        .offline(.Queue)
        .with(native.tabs([
            native.tab(logic.ProjectsView{}, title: "Projects", icon: native.Symbol.Folder),
            native.tab(logic.MineView{}, title: "Mine", icon: native.Symbol.Person),
            native.tab(logic.SearchView{}, title: "Search", icon: native.Symbol.Search),
        ]))
        // https://crew.example.com/tasks/<id> opens the task screen when the
        // app is installed. The server output publishes the association files.
        .with(native.app_links(domain: "crew.example.com"))
        // Registers this device for the signed-in account. A push from the
        // server (Crew.tell_assignee, Crew.remind_due_today) carries a screen
        // state, such as TaskView, and opens it when tapped.
        .with(native.push())
        .serve()
}
```

### 3.7 One move, followed across every target

1. Ana is on a train with no signal. On her phone she taps **Move to Done**
   on "Ship v2". The handler is `fn() app.Notice!`; while it runs, the
   button shows busy and a second tap does nothing.
2. `move_task` is a typed edit of a row the phone holds, so the phone predicts
   it (D-OPTIMISTIC1): the card moves at once. Under `.offline(.Queue)`, the
   call joins the durable queue with a request key, a fresh random ID for
   this call (#3709), and returns the predicted task. The handler finishes,
   and the button is ready again.
3. The card shows "Waiting to send" while its write is queued, and the
   status bar counts `app.pending()`. Ana can keep working; a second edit of
   the same card queues behind the first and is rebased on its result.
4. The signal returns. The queue sends `POST /api/move_task` with the key,
   the task ID, the version the board showed and the new status.
5. On the server, the derived decoder checks the published wire types
   (D-WIRESKEW1). The session is valid. In one transaction, the task policy
   allows the read and the write, the version matches, `can_move` passes, and
   the edit commits. The server stores the request key with the result.
6. The commit's write set (this row of `tasks`) meets the read set of every
   open `board(project)` subscription. Each runs again and pushes the new
   `Board`.
7. Ben's browser tab and Chen's desktop window update the one card that
   changed. Ana's phone swaps its predicted card for the server's copy and
   drops the "Waiting to send" mark.
8. If the reply is lost, the queue sends the same request again. The server
   finds the key and returns the stored result; nothing runs twice.
9. Had Ben moved the task first, the version would not match, and the server
   would answer `Conflict` with the current task. Ana's phone puts the card
   where the server has it and shows the notice from `notices.jet`, "Someone
   else changed this task", beside the card, where a screen reader announces
   it.
10. Had Ana closed the card's screen, or restarted the app, before the
    refusal arrived, the notice has no card to sit beside. It joins the
    app's notice list, which the frame shows on every screen and keeps,
    across restarts, until she dismisses it; the notice opens the task. A
    second edit queued behind the refused one was rebased on its predicted
    result, but it keeps the row version it was made with. The queue replays
    it, and the server compares that version with its copy, which Ben
    changed: the edit is refused as `Conflict` and adds its own notice to
    the list. It is never re-derived from the server's copy, which would
    overwrite Ben's change and make the version check protect nothing. An
    edit queued behind the refusal whose row nobody else changed passes or
    fails its own checks. Nothing queued behind a refusal is dropped in
    silence (#3709 c26).

On the web, which sets no queue, step 2 sends the call at once. With no
connection it fails with `Unreachable`, the card moves back, and the notice
offers to try again.

### 3.8 Lines of code per target

Counted from the files as written: lines that are not blank, comments
included.

| Package | Used by | Holds | Code lines | Test lines |
|---|---|---|---|---|
| Logic | every target | the types, the rules and the `Crew` service | 886 | 217 |
| Screens | every target | the graph, the screens and the theme | 469 | 49 |
| Web | the web server | opens the database and the account store, mounts the service, sets mail | 24 | 0 |
| Desktop | desktop | menus, a window per task, the CSV export, the offline queue | 78 | 0 |
| Phone | phones | tabs, app links, push, the offline queue | 25 | 0 |
| Manifests | — | the root and five package manifests | 86 | — |
| Total | | | 1568 | 266 |

The logic and the screens are shared by every target. What is left in each
entry is platform work: mounting the service, accounts, mail, menus, tabs,
app links and push. None of it repeats a rule or adds a screen.

### 3.9 What the probe found

Each row is a need Crewboard has that the framework does not meet today, with
the card that tracks it. Some are seed bugs found while checking these files.
Rows marked "r3" were found by wiring the c6 examples into Crewboard and by
the round-two review.

| Need | Gap | Card |
|---|---|---|
| Typed IDs on the wire | A `distinct` type is not `#Codable` (`jet.NotSerializable`), so the checked files drop the markers in Logic | #6819 c1 |
| `logic.ProjectID.from_string(...)` and `app.CachePolicy.Public(...)` from another package | `jet.ModuleAsValue`: a static call or payload case through a module path is refused. Same-package calls and a contextual `.Public(...)` work. | #6819 c2 |
| A struct alias fix-it | The AliasWithoutParameters fix-it suggests `Name: struct(BaseType)`, which does not parse | #6819 c3 |
| A generic type taken from an annotated closure parameter (r3) | `ui.drop_zone(..., on_drop: fn(task: logic.Task) {...})` gives `jet.UninferableTypeArgument` | #6819 c4 |
| A closure ending in a call that returns nothing, where `fn() -> R E!` is expected | `jet.UninferableTypeArgument` for `R` | #6819 c5 |
| A clean `validate` block (r3) | L0530 `jet.RedundantFinalReturn` fires on `validate {` | #6819 c6 |
| An error conversion declared in a dependency (r3) | the seed does not find `impl TaskError -> app.Notice` from `Screens` | #6819 c7 |
| Policy write semantics and a creator rule (r4) | `db.Op`; an update judged on both rows; a policy's reads see the transaction's earlier writes but not the write being judged; the creator rule tied to `created_by` (r5) | #3709 c22 |
| Removing members and changing roles (r4) | the last owner, live queries, assignments and System jobs after a removal | #6826 |
| A webhook key an owner can set (r4) | `app.random_key`; `None` means 401 | #6820 |
| A notice for a refused queued write whose screen is gone (r4) | the app's notice list | #3709 |
| Form fields named by the form | `f.title` on `kit.Form<TaskDraft>` needs a derived field per struct field (UnknownField today) | #6821 |
| Typed tables with keys, secrets in storage, read and write sets | `db.Table<K, T>` with a `#Secret` storage codec | #6822 |
| Membership checks in every method; the `mine` leak (r3) | typed row policies | #3709 c22 |
| Atomic writes and version checks (r3) | one serializable transaction per write method | #6445 c5 |
| A non-optional caller (r3) | the reachability rule for `app.caller()` | #6445 c4 |
| One route set on every target (r3) | `.screen` only in the graph builder | #6445 c6 |
| A destination for handler errors (r3) | `fn() app.Notice!` handlers | #6445 c7 |
| N+1 queries and in-memory search (r3) | `count_by`, relation loading, a full-text index | #6827 |
| Offline creates (r3) | client-made UUIDv7 IDs, `Table.add` | #3709 c23 |
| Pushes and mail after commit; a daily reminder (r3) | jobs, the outbox and schedules | #6825 |
| Project creation, invitations, email confirmation, password reset, passkeys, two-factor codes, a sign-in rate limit (r3) | the account lifecycle and mail | #6826 |
| An assignee in the project (r3) | server-side field errors | #6831 |
| Attachments (r3) | storage that names blobs, checks content, and runs the row policy before the body and on download | #6813 c1–c4 |
| CSV that a spreadsheet cannot run (r3) | formula escaping in the writer | #6828 |
| Signed webhooks with replay protection | `#App.Webhook(key:)`, `#App.SigningKey`, delivery IDs | #6820 |
| Secrets that never reach a client, a reply or a log | not built; seed-b15 accepts the probe | #6557 |
| Seeing every placement, context read, policy and System run | `jet inspect choices` | #6823, #6825 c4 |
| Native sessions | access and refresh tokens in the Keychain or Keystore, revocation | #6818 |
| Safe offline replay | request keys, per-device rebase, one queue across tabs, auth expiry offline | #3709 |
| Live queries at scale | caps, coalescing, a broker, predicate-to-SQL; no sharing for caller-dependent queries | #3709 c19–c21, c24 |
| Removing a wire field | `min_client`, expiry, typed `UpdateRequired` | #6558 |
| A client runtime budget | measured against Qwik and LiveView | #6556 |
| Rate limits beyond sign-in; traces and request IDs (r3) | request limits; observability | #6829, #6830 |
| Menus, tray, notifications, background work, files, camera, links, share, permissions, a11y, l10n, lifecycle, install and update | the `native` plugins | #6799–#6810 |
| SEO, streaming, uploads, caching, PWA install, assets | the web side of `app` | #6811–#6816 |
| Styling | `ui.Style`, `ui.Theme<T>` | #6817 |

Revision 2 stated an API rule that bent the public API around these seed
gaps. Revision 3 drops it. The proposed APIs take the target forms; where a
checked file cannot use one yet, section 7.13 lists the interim form and the
criterion that removes it.

## 4. The areas, one at a time

Each area says:

- what the shape does;
- which Crewboard code shows it;
- the strongest existing system for it, with a source;
- a verdict:
  - **matches**;
  - **goes further**, with a claim ID from section 8.3 that the next review
    round must test;
  - **behind**, with the card that closes the gap.

"Native" means the desktop and phone apps.

### 4.1 Services, endpoints and calls in flight

**The shape.** A service is a struct mounted on the server (D-ROUTE-ONE1). A
method is reachable from outside only under a marker:

| Marker | Who may call | Address |
|---|---|---|
| `#App.SignedIn` | a caller with a valid session; others get 401 | `POST /api/<method>` |
| `#App.Public` | anyone; the method reads `app.maybe_caller()`, never `app.caller()` | `POST /api/<method>` |
| `#App.Webhook(path:, signature:, key:)` | a sender whose signature checks against the key that the `#App.SigningKey` method named by `key:` returns; others get 401 | the given path |
| `#App.Job(retries:)` | the server, after the transaction that queued it commits; runs as `db.Who.System` | none |
| `#App.Every(cron:)` | the server's scheduler; runs as `db.Who.System` | none |
| `#App.SigningKey` | the framework, while it checks a webhook | none |
| none, or `priv` | server code only | none |

The client never sees the service itself. The graph's `.api<Crew>` gives the
screens an `app.Remote<Crew>`, which offers only the `#App.SignedIn` and
`#App.Public` methods. A call to anything else does not resolve. The seed
shows this today. `Logic/remote_gen.jet` is a hand-written stand-in for what
#6445 would generate (TR2 in A.2), and this probe calls a signing key and a
job through the remote:

`probes/cl1_server_only.jet`

```jet
// CL1 probe. Must fail: a client calls server-only methods. Both calls
// are errors, because app.Remote<Crew> has only the marked methods.
use {
    app
    logic
}

steal: fn(crew: app.Remote<logic.Crew>, project: logic.ProjectID) -> Int logic.TaskError! {
    _: crew.webhook_secret(project)
    _: crew.tell_assignee(logic.new_task_id())
    Ok(0)
}
```

The seed rejects both calls:

```text
Error [jet.UnknownFunction]: `CrewRemote` has no method `webhook_secret`
  --> cl1_server_only.jet:9:13
Error [jet.UnknownFunction]: `CrewRemote` has no method `tell_assignee`
  --> cl1_server_only.jet:10:13
```

Under #6445 the message names `app.Remote<Crew>` and suggests a marker
(#6445 c3).

**Calls in flight.** A remote call is a task on the client. The rules:

- **Pending state.** `kit.button`, `kit.icon_button`, `kit.form` and
  `kit.file_button` run their handler as a task. While it runs, the control
  shows busy and ignores a second press. Other code reads `app.state` of a
  live query, which is `Loading` until the first result.
- **UI thread.** The call never blocks drawing or input. The handler resumes
  on the UI thread when the call settles.
- **Cancellation.** Leaving a screen cancels the tasks it started. A call that
  has already reached the server may still commit; the next live push shows
  the result.
- **Timeouts.** A call that gets no answer in 30 seconds fails with
  `app.CallError.Unreachable`. Under `.offline(.Queue)` a write is queued
  instead (4.8), and only reads fail.
- **Errors.** Every button, menu and swipe handler has the type
  `fn() app.Notice!`. Its error is converted to an `app.Notice`, which the
  frame shows beside the control and announces to screen readers (#6445 c7).
  `Logic/notices.jet` holds Crewboard's one conversion from `TaskError`.
  Forms send their error to `on_error`, and reads send theirs to the
  screen's `failed` view. 4.10 covers `_:`.

**Code.** `Crew` and `notices.jet` in 3.2; `kit.form` in `Screens/task.jet`;
the move button in `Screens/board.jet`.

**Best existing.** Next.js server functions are POST endpoints that anyone can
call. The Next.js team says to "always verify authentication and
authorization inside every Server Function" [NEXT-SEC]. Convex separates
public functions from internal ones. An `internalQuery`, `internalMutation`
or `internalAction` can be called only by other Convex functions, never by a
client [CVX-INTERNAL]. Convex also recommends an auth check in each public
function [CVX-AUTH]. SvelteKit remote functions validate their arguments
[SK-RF]. tRPC types each procedure end to end [TRPC]. Leptos `#[server]`
functions are, in its words, "syntax sugar for defining a public API"
[LEP-SF].

**Verdict.**

- Typed calls **match** tRPC and SvelteKit.
- Exposure **matches** Convex's internal functions (CL1). An endpoint exists
  only by a marker, and a client call to an unmarked method does not
  resolve. Jet is safe by default where Convex asks for an `internal`
  prefix, but the guarantee is the same.

### 4.2 Data, storage and access

**The shape.** A service's tables are typed. `db.Table<K, T>` names the key
type and the row type, and `store.table("tasks", key: fn(t: Task) { t.id })`
names the key function (#6822). On top of that:

- **Row policies.** A policy is a named function from a row, the operation
  (`db.Op`), the caller (`db.Who`) and the store to a `Bool`. `open_crew`
  attaches one to each table with `.guard(...)`. The database asks it about
  every read and every write of that table: endpoints, live queries, jobs
  and webhooks (#3709 c22). It judges a read and a delete on the stored
  row, an insert on the new row, and an update on both, as Postgres's
  `USING` and `WITH CHECK` do [PG-POLICY]. The policy's own reads run as
  `db.Who.System`, so a policy can look up membership without recursing
  into itself. They see the transaction's earlier writes but not the write
  being judged: `create_project`'s member insert sees the project row the
  same transaction inserted, and a write cannot vouch for itself, as
  Postgres evaluates `WITH CHECK`. A row the caller may not read is never
  returned, and `get` answers `None` for it.
- **Policies and query counts.** A policy whose reads are key lookups
  compiles into the query as a join on the table it reads. In Crewboard
  those are `task_policy`, `attachment_policy`, `project_policy`, and
  `member_policy` for a read: each looks up the caller's own member row by
  key. `my_projects` is then two statements, whatever the number of
  projects, with the policy inside each, and `search(...).limit(50)`
  returns the first 50 rows the policy passed: it filters before it limits.
  A policy the compiler cannot join runs once per row, memoized per key
  within the transaction, and `jet inspect choices` names it with its cost.
  In Crewboard those are `invite_policy`, which asks `auth.verified_email`,
  and `member_policy` for an insert, which counts the project's members and
  reads its invitation; both judge a handful of rows per call. #6827 c2 and
  c3 count the policies' statements too.
- **Transactions.** Each method that writes is one serializable transaction,
  retried when it loses a race (#6445 c5). A version check and the write
  after it cannot be split by another writer. A method called from another
  method, an endpoint included, joins the caller's transaction rather than
  starting its own, so `import_tasks`' calls of `create_task` commit or roll
  back together. `close_from_issue` and `import_tasks` are all or none.
- **Adds.** `Table.add(row)` inserts a row whose key must be new. A taken key
  is `Exists` when the caller may read the row that holds it, and `Denied`,
  as any refusal, when it may not; so `create_task` with another project's
  task ID tells the caller nothing. It is how a client-made ID (4.8) reaches
  the table; the ID's decoder accepts only UUIDv7 text.
- **Deletes.** `Table.delete(key)` removes a row; the policy judges it on
  the stored row.
- **Queries without N+1.** `query().filter(...).count_by(...)` counts per key
  in one query. Relation loading fetches the rows a list refers to in one
  more query. `Table.search(words)` uses a full-text index declared on the
  table (#6827).
- **Read and write sets.** Each query records its read set and each write
  records its write set. Live queries use them (4.6). `SQL{...}` stays for
  experts and refreshes at table level.

**Code.** `open_crew`, `Logic/policy.jet` and every method in
`Logic/crew.jet`.

**Best existing.**

- Supabase applies Postgres row-level security to every query a client
  makes [SUPA-RLS].
- EF Core detects concurrent updates with a concurrency token [EF-CONC].
- Ecto preloads associations in one query per relation [ECTO-PRELOAD].
- Postgres has full-text search with a GIN index [PG-FTS], and Laravel Scout
  puts a search driver behind a model [SCOUT].
- Ecto changesets and Rails Active Record are the mature row APIs [ECTO]
  [RAILS-AR].
- Convex declares a schema and tracks each query's read set [CVX-SCHEMA]
  [CVX-RT].
- EF Core translates LINQ predicates to SQL, which is also its hardest
  problem [EF-QUERY].

**Verdict.**

- **Behind** today. The typed table and its query chain are #6822. Policies,
  predicate-to-SQL and client IDs are #3709 c21–c23. Transactions are
  #6445 c5. `count_by`, relation loading and the search index are #6827.
- Once built, policies **match** Supabase row-level security, and the rest
  matches EF Core and Ecto. The read and write sets are why the shape has
  typed tables at all.

### 4.3 Forms and validation

**The shape.** One `validate` block per type, in the form dev accepts:
`check(condition, at: field, "message")`.

- The form runs the block as the person types.
- The server's derived decoder runs it again before the method sees the
  value. A failed decode is HTTP 422 with the `[FieldError]` list, which
  reaches the form as `TaskError.Invalid(fields)` and is shown beside each
  field.
- A rule that needs the database runs in the method and reports through the
  same list: `check_assignee` refuses an assignee who is not a member of the
  project with `app.field_error(at: "assignee", reason: ...)` (#6831).
- Decoding a list runs each element's `validate`, and a failure's path
  names its element: the decoder answers a `TaskBatch` whose fourth task has
  no title with `tasks[3].title`. `import_tasks` gives `check_assignee`'s
  refusal the same kind of path, `tasks[3].assignee`, so a person importing
  500 rows sees which one was refused.
- Every endpoint takes its form's type, never loose fields: `create_task`
  and `update_task` take a `TaskDraft`, `create_project` a `ProjectDraft`,
  `invite` an `InviteDraft` and `import_tasks` a `TaskBatch`. So the rule a
  form checks is the rule the server checks, written once (CL2).
- `field_error(at:)` names a field of a decoded argument, which #6831 c1
  checks at compile time; Crewboard's one call names `assignee` in
  `draft`.
- `kit.form` takes an initial value, the submit label, the handler, the
  error mapper and a layout of fields (#6821).

```text
TaskDraft.validate  ─┬─ in the form, on every change (client)
                     └─ in the derived decoder, before create_task runs (server)
check_assignee      ─── in the method, inside its transaction (server)
```

**Code.** `TaskDraft` and `check_assignee` in 3.2; `editor` in
`Screens/task.jet`; the 422 test in `Screens/screens_test.jet`.

**Best existing.**

- ASP.NET Core `[ApiController]` validates the model before the action runs
  and answers 400 with the field errors [ASPNET-API].
- FastAPI validates every request body against its type and answers 422
  [FASTAPI-BODY].
- Phoenix form bindings with Ecto changesets run one set of rules for the
  form and the database, and Ecto checks that a referenced row exists
  [PHX-FORM] [ECTO].
- Laravel's `exists` rule checks a value against a table [LAR-VAL].
- Rails validations are declared once on the server [RAILS-VAL].
- Blazor runs data annotations in the browser and on the server [BLZ-VAL].
- Remix and React Router post forms without JavaScript [RR-ACT].

**Verdict.** **Matches** ASP.NET Core and FastAPI for validating in the
decoder, and Blazor for the one rule set (CL2). Database-aware checks match
Ecto and Laravel's `exists` once #6831 lands.

### 4.4 Routes and navigation

**The shape.** A screen's state struct is the route. The graph maps each
path to one struct: `.screen("/projects/:project", board)` fills
`BoardView.project` from the path and `BoardView.q` from `?q=`.

- `kit.link`, `app.go`, `app.replace` and `app.go_back_or` take a state
  value.
- **One route set.** `.screen` is allowed only in the graph builder (#6445
  c6). Every target shares that graph, so web URLs, desktop windows, phone
  app links and notifications all resolve through the same table. A state
  that one target can link to has a route on every target.
- An entry may add views over the shared routes (the phone's tabs, the
  desktop's windows), but not new routes.
- A path the table does not know shows `.not_found`.

**Code.** `Logic/routes.jet`, `Screens/shell.jet`;
`kit.link(TaskView{id: task.id}, ...)` in the board;
`app.replace(BoardView{...})` in the filter box.

**Best existing.**

- Navigation Compose builds destinations from serializable types, checked
  by the compiler [NAV-TYPESAFE].
- Expo Router derives one route set from the file tree for iOS, Android and
  the web, and the same paths serve as deep links [EXPO-ROUTER].
- TanStack Router types path and search parameters and validates search
  parameters [TSR].
- Phoenix verified routes check every path at compile time [PHX-VR].
- Elm builds URLs from typed values [ELM-NAV].

**Verdict.** **Matches** Navigation Compose for typed destinations and Expo
Router for one route set across targets (CL3).

### 4.5 Views and components

**The shape.** A screen is a function that returns `ui.Node`. `ui` holds
layout primitives (`column`, `row`, `grid`, `text`, `image`) and `kit` holds
the standard views (D-VIEW-SET1). A component is a function.

- Screen-local state is `ui.local(value)`. State that must survive a restart
  is `ui.saved(value)` (5.1).
- **Identity.** A `kit.list` row is identified by its required `key:`. Local
  state, focus and animation belong to that key, so a filtered or moved card
  keeps its own state. Outside lists, a node's identity is its position in
  its parent.

**Code.** Every file in `Screens/`; the keyed list in `column`
(`Screens/board.jet`).

**Best existing.**

- SwiftUI and Jetpack Compose make views functions of state [SWIFTUI]
  [COMPOSE].
- SwiftUI gives `ForEach` rows an explicit identity [SWIFTUI-ID], and
  Compose's `key` does the same for lazy lists [COMPOSE-KEY].
- Flutter ships the widest drawn widget set [FLUTTER].
- Elm's architecture has one model and one update path [ELM-ARCH].

**Verdict.**

- **Matches** the declarative model and its identity rules.
- **Behind** on the size of the view set until #6447 builds `kit`.

### 4.6 Loading, refreshing and prediction

**The shape.**

- `app.live(fn() { crew.board(id) })` is a live query.
- `app.state` gives `Idle`, `Loading`, `Loaded` or `Failed`.
- A write refreshes every live query whose read set it meets, with no keys
  (D-INVALIDATE1).
- A typed edit of rows the client holds is predicted (D-OPTIMISTIC1), and
  so is a create whose ID the client made (4.8): `new_task` opens the
  predicted task at once, marked "Waiting to send".
- The server runs the live reads a screen needs at the same time, before it
  sends HTML.

The closure is the target form, not an interim one. Its body must be one
call of an `app.Remote` endpoint with codable arguments, or the compiler
refuses it, so the subscription, the device cache and #3709's sharing are
keyed by that method and those arguments (#6445 c8). `app.enqueue` follows the
same rule (4.14). D-LIVEQUERY1's `app.live(query, args)` therefore stays in
its closure form, and the shape does not need bound method values.

**Limits, stated.**

- **Waterfalls.** Reads that do not depend on each other run together. A read
  inside a conditional, or a read whose argument comes from another read's
  result, waits for that result. The search screen's results wait for two
  letters; that is a waterfall by design.
- **Prediction is a subset of the server's run.** The client runs the method
  on its copy of the rows, but it skips what needs the server:
  - the row policies;
  - the version check;
  - `check_assignee`;
  - `app.enqueue`, and so every job, push and mail;
  - the server's clock.

  So a predicted card can be refused and move back, a predicted create can
  be refused and disappear with its notice, and a predicted timestamp is
  replaced by the server's. Only typed edits of rows already on the client
  and creates with a client-made ID are predicted (#6555 c7). Anything else
  shows busy and waits.

**Best existing.**

- Convex re-runs a query only when its read set changes [CVX-RT].
- Meteor methods run the same code as a client stub and on the server, and
  the server's result replaces the stub's [METEOR-METHODS] [METEOR].
- Zero runs mutators on the client and again on the server, which is
  authoritative [ZERO].
- SvelteKit refreshes queries after a command [SK-RF].
- Next.js asks for `revalidatePath` or `revalidateTag` by hand [NEXT-CACHE].

**Verdict.** **Matches** Convex for refresh, and Meteor and Zero for
prediction from the same code (CL4). As in both of those systems, the client
cannot run the checks only the server can.

### 4.7 Accounts, sessions, secrets and webhooks

**The shape.**

- **Accounts.** `.accounts(...)` in the server entry names the account store
  and its lifecycle (#6826):
  - email confirmation, password reset, passkeys and two-factor codes;
  - a sign-in rate limit (`limit_sign_in(per_minute: 5)`, #6826 c1).

  `kit.account_flows()` adds their screens to the one route set. Invitations
  go to a confirmed email address, and accepting one needs no token. The
  account store keeps addresses lowercased, and `auth.verified_email`
  returns them so; `invite` lowercases the address it is given. An
  invitation lapses after 14 days, and accepting it deletes it.
- **The caller.** Inside an `#App.SignedIn` method, `app.caller()` returns the
  caller's account. It is not an option. The compiler accepts the call only
  on paths that start at a signed-in method or a test's `as_caller`; a job,
  a webhook or a public method calling it is a compile error (#6445 c4).
  Public methods use `app.maybe_caller()`. The call reads the request
  context (D-CONTEXT-EFFECT1=F: context is an implicit input, not an
  effect), and `jet inspect choices` lists every method that reads it
  (#6823).
- **Web sessions.** An `HttpOnly`, `Secure`, `SameSite=Lax` cookie. Unsafe
  methods also need a CSRF token, as Rails does by default [RAILS-SEC].
- **Native sessions.** A short-lived access token, and a refresh token in the
  Keychain or Keystore. The account page lists devices and revokes one
  (#6818). A revoked or expired session makes the next call fail with
  `CallError.SignedOut`. That becomes `TaskError.SignedOut`, and the notice
  says "Sign in again to continue."
- **Secrets.** `#Secret` is a tag on a type: `webhook_secret: #Secret
  String`. Today the tag gives fixed-timing rules (E0737, E0739). The rule
  that a secret never flows into a plain value, a reply, a log or a client is
  **proposed** in #6557 (D-SECRETEDGE1). The seed accepts this probe, which
  prints a secret:

`probes/cl5_secret_print.jet`

```jet
// CL5 probe. Passes on seed-b15, which is the finding: dev has no rule yet
// that keeps a #Secret value out of output. D-SECRETEDGE1 (#6557) proposes it.
use {
    logic
}

leak: fn(project: logic.Project) Never! {
    print("key {project.webhook_secret}")
}
```

- **Webhooks.** `#App.Webhook(path:, signature: app.Signature.GitHub, key:
  "webhook_secret")`. `key:` names the `#App.SigningKey` method that returns
  the key for this delivery; the compiler checks that it names one of the
  same service.
  - The framework computes HMAC-SHA256 over the raw body
    (`core.crypto.hmac_sha256`) and compares it with
    `core.crypto.constant_time_equal` before decoding. A wrong or missing
    signature gets 401, and the method does not run.
  - A signing-key method that returns `None` (an unknown project, or one
    with no key yet) also gives 401: a missing key never skips the check
    (#6820 c4).
  - The key is 256 bits from the operating system's random source
    (`app.random_key`), made by an owner with `new_webhook_key`, stored in
    the project row that only owners write, and shown to that owner once.
    It is never derived from an ID.
  - A GitHub signature has no timestamp, so the framework also keeps each
    `X-GitHub-Delivery` ID for 30 days and drops a delivery it has already
    run (#6820 c2, c3).

**Best existing.**

- **Accounts.** Laravel Fortify, Phoenix's `mix phx.gen.auth` and ASP.NET
  Core Identity ship confirmation, reset and two-factor flows [FORTIFY]
  [PHX-AUTH] [ASPNET-ID]. Rails 8 has a built-in rate limiter for sign-in
  [RAILS-RL]. Passkeys follow WebAuthn [WEBAUTHN].
- **Sessions and authorization.** Rails signs and encrypts cookies and checks
  CSRF by default [RAILS-SEC]. Laravel policies put authorization beside the
  model [LAR-AUTH]. Phoenix signs tokens for sockets and APIs [PHX-TOKEN].
- **Secrets.** Rust's `secrecy` crate keeps a secret out of `Debug` output
  [SECRECY]. React's experimental taint APIs mark values that must not reach
  the client [REACT-TAINT].
- **Webhooks.** GitHub and Stripe document the signature check, and GitHub
  the delivery ID, that every receiver must write by hand [GH-HOOK]
  [GH-HOOK-BP] [STRIPE-SIG].

**Verdict.**

- Accounts **match** Fortify and phx.gen.auth once #6826 lands. Sessions
  match Rails and Phoenix once #6818 lands.
- Secrets: today the seed **matches** `secrecy` at most. Whether the
  compiler can keep a secret out of every output (CL5) is untested until
  #6557 builds the rule.
- A webhook whose signature check and replay check cannot be left out goes
  further for methods marked as webhooks (CL6).

### 4.8 Offline

**The shape (O4 A).** The server is the authority. When the entry sets
`.offline(.Queue)`, a device keeps the last live results and a durable queue
of writes. The queue's rules are criteria on #3709:

- **Request keys.** Each queued call carries a random key. The server stores
  the key with the result for 24 hours and answers a repeat with the stored
  result, as Stripe does with idempotency keys [STRIPE-IDEM].
- **Client IDs.** A create carries its ID, a UUIDv7 made on the device
  (`app.new_id`). The task has its final ID at once and a link to it works
  offline. The ID type's decoder accepts only UUIDv7 text, so a crafted
  `"t1"` gets 422, and `Table.add` refuses a taken ID: as `Exists` when the
  caller can read that row, as `Denied` when it cannot (#3709 c23).
- **Rebase per device.** Two queued edits of one task are sent in order. The
  second is rebased on the first's result, so a device never conflicts with
  itself on `version`.
- **One queue across tabs.** In a browser, the queue lives in IndexedDB and
  one tab sends it, chosen with the Web Locks API [WEBLOCKS].
- **Auth expiry offline.** If the session expires while the device is
  offline, the queue stops, keeps its writes, and asks the person to sign
  in. Then it sends the writes under the new session, and each one meets the
  server's checks again.
- **What the person sees.**
  - A queued write returns its predicted result at once, so the handler
    succeeds and the button stops being busy.
  - The card shows "Waiting to send" until the server answers.
  - `app.pending()` counts queued writes, and `examples/realtime.jet` shows
    "2 changes waiting to send".
  - A write the server later refuses is rolled back, and its notice appears
    then.
  - The web entry does not set `.offline(.Queue)`, so on the web a write
    waits online, and nothing is queued.

Data that must merge offline (notes, drawings) opts into `app.collection`
(D-SYNC2). Each pushed change is applied by calling the same server method
an online client calls, under the pusher's session, so the same policies
run. A refused change comes back to the device as a conflict.

**Best existing.**

- Replicache and Zero send mutators with request IDs and rebase on the
  server's answer [REP] [ZERO].
- Firestore keeps data offline and enforces its security rules on sync
  [FS-OFF] [FS-RULES].
- ElectricSQL and PowerSync replicate a declared shape of Postgres with
  permissions [ELEC] [PS].

**Verdict.** **Behind** Replicache and Firestore until #3709 lands. The rules
above are the plan to match them.

### 4.9 Realtime

**The shape.** Stored data is realtime through live queries.

- **Presence.** "Ben is here" is `app.presence(state)`: the accounts that
  have that screen state open now and whose last read of that screen's data
  passed its row policies, updated like a live query. An account whose read
  was refused, such as an outsider who opened a shared link, is never
  counted (#3709 c25). Presence is reached only through a service method:
  `Crew.viewers(id)` reads the task first. The task policy refuses a caller
  who is not in its project, so only the project's members learn who is
  looking.
- **Who shares a result.** Subscribers with the same arguments share one
  result only when the query does not depend on the caller. Crewboard's
  queries all pass through row policies that read the caller, so they do not
  share: each subscriber's query runs again when a write meets its read set
  (#3709 c24).

  That costs one query per open board for each write to that board. A
  project with 20 people looking at it runs 20 queries per move. The
  mitigations are criteria on #3709:
  - writes in one 50 ms window are coalesced;
  - each server caps subscriptions;
  - a Postgres LISTEN/NOTIFY broker carries write sets between servers
    (#3709 c20);
  - a predicate that cannot become SQL falls back to table-level
    invalidation (#3709 c21).

  A query that reads no caller data shares one result, as in Convex.

`examples/realtime.jet`

```jet
// Proposed, not built (#3709 live queries and offline queue; #6445 app).
use {
    app
    ui
}

// Presence is in Screens/task.jet: the screen calls Crew.viewers, a
// signed-in method that reads the task first, so its row policy decides who
// learns who is looking.
//
// Offline: what the queue still holds. The desktop and phone entries set
// .offline(.Queue) and show this in the status bar. The web entry sets no
// queue, so on the web a write that can't be sent fails at once and
// app.pending() is always 0.
waiting: fn() -> ui.Node {
    n: app.pending()
    if n == 0 -> ui.empty() else -> ui.text("{n} changes waiting to send", one: "1 change waiting to send", plural: n)
}
```

**Best existing.**

- Phoenix Presence tracks who is online across a cluster [PHX-PRES].
- Laravel broadcasts events over channels, with an authorization callback
  per private channel [LAR-BC].
- Convex shares one query result among subscribers with the same arguments
  and auth [CVX-RT].
- Turbo Streams push HTML updates [TURBO-STREAMS].

**Verdict.** **Behind** Phoenix Presence and Convex until #3709 lands.

### 4.10 Errors

**The shape.** Each service declares one error type. Conversions from
`app.CallError` and `db.DBError` make transport and storage failures cases of
it.

- **One place for words.** A screen turns a read's error into words in one
  place (`failed` in `Screens/projects.jet`). A handler's error becomes an
  `app.Notice` through `Logic/notices.jet` (4.1).
- **`_:` hides only the value.** `_: crew.move_task(...)` drops the moved
  task, which the live board shows anyway. The error still propagates out of
  the handler, so a refused move rolls the card back and shows "Someone else
  changed this task". No call in Crewboard drops an error.
- **Handlers outside a screen.** A handler whose control is not on a screen
  still has a destination (#6445 c7):
  - the tray and menu bar builders keep their last menu and add a disabled
    first item with the notice's text, and a Retry item when the notice
    allows one;
  - `native.background`'s `run` has no window, so its notice joins the
    app's notice list, which the frame shows when the app next opens and
    keeps until it is dismissed; the app's log keeps it too, with the run's
    time. The next run is on the system's schedule;
  - `native.lifecycle`'s `on_resume` shows its notice at the top of the
    window that came back;
  - `native.drop_files`'s `on_drop` shows it beside the drop target, as a
    button does;
  - a queued write refused after its screen closed goes to the app's
    notice list (3.7, step 10).
- **Reads fail visibly.** Every live read shows its failure: the invitations
  list uses `failed`, and the "also viewing" line says it could not load.
  No read in Crewboard maps a failure to nothing.
- **Unhandled errors.** An unhandled error at the top of the graph shows
  `.error(problem)` with an ID that is in the server log. `.not_found(lost)`
  handles unknown addresses.
- **Unreachable.** `CallError.Unreachable` says "Can't reach Crewboard. Try
  again." It never claims that a write was saved. On the web nothing is
  queued; on native, a queued write shows "Waiting to send" instead (4.8).

**Best existing.**

- Elm's `Http.Error` is a typed value that every request handler must match,
  including `NetworkError` and `Timeout` [ELM-HTTP].
- Elm has no runtime exceptions in its own code [ELM].
- SvelteKit and Next.js show error pages per route [SK-ERR] [NEXT-ERR].

**Verdict.** **Matches** Elm (CL7): network errors are typed values that the
code must handle. It also matches per-route error views.

### 4.11 Tests and observability

**The shape.** Rules are tested as plain functions.

- A service is tested on an in-memory database with
  `app.as_caller(account, fn() {...})`, which also satisfies the caller rule.
- Screens are tested on `app.test_host(graph)`. It runs the real service and
  drives the screens through the same remote boundary a browser uses: `open`,
  `type`, `press`, `shows`, `within`, `raw_call` and `calls_sent`.
- **In production,** every endpoint call, job run, webhook delivery and live
  query refresh is a span with the method, the caller's account ID and the
  outcome. Spans and counters export over OpenTelemetry [OTEL] (#6830).

**Code.** `Logic/logic_test.jet`, `Screens/screens_test.jet`.

**Best existing.**

- Phoenix.LiveViewTest drives a live view without a browser [PHX-TEST].
- Rails ships system and integration tests [RAILS-TEST].
- OpenTelemetry is the common export format for traces and metrics [OTEL].

**Verdict.** **Matches** LiveViewTest. The test host lands with #6445, and
the spans with #6830.

### 4.12 Build, deploy and wire versions

**The shape.**

- `jet build server` makes a self-contained binary, and an OCI image wraps
  it.
- `jet build desktop` and `jet build phone` make signed apps. The desktop
  output checks a signed update feed (`updates:` in the root manifest,
  #6810).
- Phone store signing follows D-NATIVEUI3.

Wire versions (D-WIRESKEW1): a build fails when it changes a published wire
type in a way an installed client cannot read. Removing a field is a
criterion on #6558:

1. Mark the field deprecated; the server keeps reading it.
2. Set `min_client:` in the manifest to the first version that does not need
   it.
3. After the expiry window (90 days by default), the server answers older
   clients with `CallError.UpdateRequired`. Crewboard shows "Update
   Crewboard to continue."
4. Then the field can be removed, and the build allows it.

**Best existing.**

- Phoenix releases are self-contained [PHX-REL].
- Electron and Tauri ship signed auto-updates [ELECTRON-UPD] [TAURI-UPD].
- Expo pushes over-the-air updates within a runtime version [EXPO-UPD].
- buf's breaking-change check fails a build that changes a published
  Protobuf schema incompatibly [BUF].
- Apollo's schema checks test a GraphQL change against the operations
  clients sent recently [APOLLO-CHECKS].

**Verdict.**

- **Matches** Phoenix releases for the server.
- The wire policy **matches** buf (CL8). It is **behind** Apollo, whose
  checks know which clients still read a field.
- Desktop updates are **behind** until #6810 lands.

### 4.13 Performance

**The shape.**

- Pages resume instead of hydrating (D-RESUME1).
- Each route loads its own code.
- The server runs a screen's independent reads together and streams HTML
  (5.2).
- Hashed assets are cached for a year.

**Limits.** The client runtime (the live store, the queue and the resumer)
has no measured size yet. Its budget is a criterion on #6556. It is measured
on the Crewboard board against Qwik and LiveView, and reports the bytes, the
time to interactive and the memory per target. Live-query cost follows the
#3709 model in 4.9.

**Best existing.**

- Qwik resumes with no hydration replay [QWIK].
- LiveView pays no client runtime beyond its socket client [PHX-LV].
- React streams HTML as data arrives [REACT-STREAM].

**Verdict.** **Behind** until #6556 measures it. This section makes no speed
claim.

### 4.14 Jobs, schedules and mail

**The shape.** Work that must happen after a write, or on a clock, runs on
the server (#6825).

- **After commit.** `app.enqueue(fn() { self.tell_assignee(id) })` records a
  job in the same transaction as the write: an outbox. The job runs only
  after the commit, and a write that rolls back queues nothing.
  - The body must be one call of an `#App.Job` method of the same service,
    with codable arguments. The arguments are evaluated and encoded at the
    call, so the outbox row holds the job's name and their bytes, never a
    closure; any other body is a compile error (#6825 c5). `after_assign`
    captures `task`, but only `task.id` is encoded.
  - `update_task` and `create_task` call `after_assign`, which queues
    `tell_assignee` when the assignee changed.
  - `invite` queues `send_invite`; `remove_member` queues `unassign`.
- **At least once.** A run that sends its push and then crashes before it is
  recorded runs again, so a job runs at least once, as in Oban and Solid
  Queue. Each queued call has a key, and so does each scheduled run (the
  job's name and the time it was due). Each push and each mail a run sends
  has its own key, made from the run's key, the recipient and the `open:` or
  `link:` value (#6825 c1). `app.notify` passes it to APNs as
  `apns-collapse-id` [APNS-REQ] and to FCM as the collapse key
  [FCM-COLLAPSE], and `app.mail` uses it as the Message-ID. So a repeated
  run replaces each push it sent before, and a receiver that drops a
  repeated Message-ID shows the mail once. `remind_due_today`'s pushes about
  three tasks due today have three keys and stay three pushes. A key made
  from the send's position in the run would not do: a re-run whose query
  returns another set of tasks would give a push the key of a different
  one. A job that writes rows is a transaction like any method, so a repeat
  finds its own result (`unassign` finds no task left to clear).
- **Retries.** `#App.Job(retries: 5)` retries a failed run with backoff. A run
  that fails five times stays in the job table with its error.
- **Schedules.** `#App.Every(cron: "0 8 * * *")` runs `remind_due_today` each
  morning, and `"0 3 * * *"` runs `drop_expired_invites` each night, on one
  server, not on every replica.
- **Rights.** Jobs and schedules run as `db.Who.System`, which no policy
  limits, so a job that pushes to a person checks that the person is still a
  member (`tell_assignee`, `remind_due_today`). `jet inspect choices` lists
  every method that runs as System (#6825 c4).
- **Mail.** `app.mail(to:, subject:, body:, link:)` sends through the
  transport the server entry names (`.mail(app.smtp(...))`). Inside a job it
  is retried with the job (#6826).

**Code.** `tell_assignee`, `send_invite`, `unassign`, `remind_due_today`,
`drop_expired_invites`, `after_assign` and `queue_invite` in
`Logic/crew.jet`; `.mail` in `Web/web.jet`.

**Best existing.**

- Oban stores jobs in Postgres, inserts them in the caller's transaction and
  runs cron schedules [OBAN].
- Rails Solid Queue does the same for Active Job [SOLIDQ].
- Laravel queues dispatch after the commit when told to [LAR-QUEUE].
- Convex schedules functions from mutations and runs cron jobs [CVX-SCHED].
- Rails warns that a callback which sends mail before the commit can mail
  about a write that rolled back [RAILS-CB].

**Verdict.** **Matches** Oban and Solid Queue once #6825 lands. Today
**behind**.

### 4.15 Exports and imports

**The shape.** `core.encoding.csv`'s writer escapes a cell that starts with
`=`, `+`, `-`, `@`, a tab or a carriage return, by prefixing a single quote
(#6828). So a board that Crewboard exports cannot run a formula when a
spreadsheet opens it. The reader of an import reports each bad row with its
line (`examples/files.jet`, 5.1.4).

The escape makes the round trip lossy, and the shape says so. An exported
title `=SUM(1)` is written as `'=SUM(1)`. Crewboard's import takes off one
leading apostrophe before one of those six characters (`unescape` in
`examples/files.jet`), so its own export imports as it was. The cost: a
title that really began with an apostrophe before one of them, such as
`'=draft`, imports without that apostrophe.

**Best existing.** OWASP's CSV injection guidance gives this escaping rule;
most CSV libraries leave it to the caller [OWASP-CSV].

**Verdict.** **Matches** OWASP's rule once #6828 lands, as a default of the
writer rather than a step each app must remember.

## 5. What a native app and a web app need (card c6)

Each need has:

- a checked example that adds it to Crewboard;
- the card that builds it, and its criteria;
- the strongest existing system for it.

Platform features come from the `native` package as plugins (rule 8). Where a
browser has the same feature, the same call works on the web; where it does
not, the table says so. Every card from #6799 to #6817 has criteria taken
from these examples. Each criterion names a test that proves it, so each gap
named here can be shown closed.

### 5.1 Native needs

| Need | Jet | Desktop | Phone | Web | Example | Card | Best existing |
|---|---|---|---|---|---|---|---|
| Windows | `.window(...)`, `.open_in_window<V>()` | yes | one window | tabs | 3.5 | #2999 | Electron `BrowserWindow`; SwiftUI scenes |
| Menu bar and menus | `native.menu_bar`, `native.menu`, `native.item` | yes | no | no | 3.5 | #6799 | Electron `Menu` [ELECTRON-MENU] |
| Tray and context menus | `native.tray`, `native.context_menu` | yes | context menu on long press | context menu | 5.1.1 | #6799 | Electron `Tray` [ELECTRON-TRAY]; SwiftUI `MenuBarExtra` [SWIFTUI-MBE]; Qt [QT-TRAY] |
| Notifications and push | `app.notify` from a server job; `native.push()` registers a device; `native.notify` for local notifications | yes | APNs, FCM | Web Push [MDN-PUSH] | 5.1.2, 4.14 | #6800, #6825 | Expo push [EXPO-PUSH]; Tauri [TAURI-NOTIF]; Apple [APPLE-UN] |
| Background work | `native.background(every:, run:)` | while running, or a login item | BGTaskScheduler, WorkManager | **no**: a compile error that names server jobs (#6801 c3) | 5.1.3 | #6801 | Expo BackgroundTask [EXPO-BG]; Android [ANDROID-BG]; Apple [APPLE-BG] |
| Files and pickers | `native.open_file`, `native.save_file`, `native.drop_files`, all giving a `native.PickedFile` | yes | yes | the browser's file pickers, or a download | 5.1.4, 3.5 | #6802 | the File System Access API [MDN-FSA]; Tauri dialog [TAURI-DIALOG]; `<input type=file>` [MDN-FILE] |
| Camera, sensors, location | `native.take_photo`, `native.location` | where present | yes | `getUserMedia`, Geolocation | 5.1.5 | #6803 | Capacitor Camera [CAP-CAM]; Expo Location [EXPO-LOC] |
| Deep links and app links | `native.app_links(domain:)`, `native.url_scheme(name)`; one running desktop app | custom scheme, single instance | universal links, app links | the URL itself | 5.1.6, 3.6 | #6804 | Tauri deep linking [TAURI-DL] and single instance [TAURI-SI]; Electron `requestSingleInstanceLock` [ELECTRON-SI]; Apple [APPLE-AD]; Android [ANDROID-AL]; Flutter [FLUTTER-DL] |
| Clipboard, drag and drop, share | `native.copy_text`, `native.paste_text`, `ui.draggable`, `ui.drop_zone`, `native.drop_files`, `native.share` | yes | yes | Clipboard API, DnD, Web Share | 5.1.7 | #6805 | MDN Clipboard [MDN-CLIP] and DnD [MDN-DND]; Electron file drag [ELECTRON-DND] |
| Permissions | `native.ask`, `native.open_settings`, `purposes:` in the phone output | where the OS asks | yes | browser prompts | 5.1.5 | #6806 | Expo permissions and config plugins [EXPO-PERM] [EXPO-CP]; Android [ANDROID-PERM] |
| Accessibility | `kit` roles, `ui.label`, `ui.announce`, required `label:` on icon buttons and drag targets | yes | yes | semantic HTML and ARIA | 5.1.8, 5.1.7 | #6807 | SwiftUI [SWIFTUI-A11Y]; React Native [RN-A11Y]; Flutter [FLUTTER-A11Y]; Android [ANDROID-A11Y] |
| Localization | literals are messages; `ui.text(..., one:, plural:)`; `jet i18n --add=de` | yes | yes | yes | 5.1.9 | #6808 | Flutter [FLUTTER-I18N]; Android [ANDROID-L10N] |
| Dark mode and system theme | paints with light and dark values; `native.theme()` preset | follows the system live | yes | `prefers-color-scheme` | 6.3 | #333, #6817 | SwiftUI `colorScheme` [SWIFTUI-CS]; Flutter `ThemeData` [FLUTTER-THEME] |
| Lifecycle | `ui.saved`, `native.lifecycle(on_resume:)` | yes | yes | Page Lifecycle | 5.1.10 | #6809 | SwiftUI `scenePhase` [SWIFTUI-SP]; Compose state saving [COMPOSE-SAVE] |
| Install, update, signing | `updates:` on the desktop output; store signing per D-NATIVEUI3 | signed feed | store builds | PWA (5.2) | 3.1, 4.12 | #6810, #480 | Tauri updater [TAURI-UPD]; Electron `autoUpdater` and signing [ELECTRON-UPD] [ELECTRON-SIGN]; Apple notarization [APPLE-NOTARY]; Expo builds and updates [EXPO-BUILD] [EXPO-UPD] |

#### 5.1.1 Tray and context menus

`examples/tray.jet`

```jet
// Proposed, not built (#6799 menus, tray and menu bar).
use {
    app
    logic
    native
    ui
}

// Desktop: a tray icon that stays when the last window closes. Its menu
// lists the caller's projects and is rebuilt when that list changes. If the
// list can't load, the tray keeps its last menu and adds a disabled first
// item with the notice's text.
with_tray: fn(graph: app.App) -> app.App Never! {
    graph.with(native.tray(native.Symbol.Crewboard, tray_menu))
}

tray_menu: fn(crew: app.Remote<logic.Crew>) -> [native.MenuItem] logic.TaskError! {
    out := [native.MenuItem]{}
    loop card in crew.my_projects() {
        out.push(native.item(ui.verbatim(card.name), run: fn() { app.go(logic.BoardView{project: card.id}) }))
    }
    out.push(native.separator())
    out.push(native.quit_item())
    Ok(out)
}

// A context menu on a card: right click on desktop, long press on a phone.
// Share hands the task's address to the system share sheet (5.1.7).
card_menu: fn(crew: app.Remote<logic.Crew>, task: logic.Task, body: ui.Node) -> ui.Node {
    native.context_menu(body, [
        native.item("Copy Link", run: fn() { native.copy_text(app.url_of(logic.TaskView{id: task.id})) }),
        native.item("Share…", run: fn() { native.share(app.url_of(logic.TaskView{id: task.id})) }),
        native.item("Mark Done", run: fn() { _: crew.move_task(task.id, task.version, logic.Status.Done) }),
    ])
}
```

#### 5.1.2 Notifications and push

Pushes that people rely on come from the server, after a commit or on a
schedule (4.14). They reach every registered device, the web included. The
device itself only posts local notifications.

`examples/notify.jet`

```jet
// Proposed, not built (#6800 notifications and push).
use {
    logic
    native
    ui
}

// Pushes come from the server: Crew.tell_assignee (a job queued when a task
// gets an assignee) and Crew.remind_due_today (a schedule). Every device the
// account registered with .with(native.push()) gets them: APNs on Apple
// systems, FCM on Android, Web Push in a browser. A tap opens the TaskView
// state through the one route table.
//
// Device: a local notification needs no server.
remind: fn(task: logic.Task) -[IO]> {
    native.notify(title: "Due today", body: ui.verbatim(task.title), open: logic.TaskView{id: task.id})
}
```

#### 5.1.3 Background work

Background work exists on phones and desktops only, and the web column says
so. A browser runs nothing for a closed page. `native.background` in a graph
built for the web output is a compile error that names server jobs as the
replacement (#6801 c3). Crewboard's reminders are a server schedule, so they
do not depend on this.

`examples/background.jet`

```jet
// Proposed, not built (#6801 background work).
use {
    app
    logic
    native
}

// Phone and desktop: while the app is closed, refresh the Mine list, so it
// opens offline with current data. The system picks the moment
// (BGTaskScheduler on iOS, WorkManager on Android, a timer on desktops); 30
// minutes is a floor, not a promise. A run that passes the system's time
// budget is cancelled like any other task. No window is open while it runs,
// so a failed run's notice goes to the app's log; the next run is on the
// system's schedule as usual.
//
// Reminders do not depend on this: the server sends them
// (Crew.remind_due_today), so they reach every device, the web too. A
// browser runs nothing for a closed page, so native.background in a graph
// built for the web output is a compile error.
with_refresh: fn(graph: app.App) -> app.App Never! {
    graph.with(native.background(every: 30min, run: refresh_mine))
}

refresh_mine: fn(crew: app.Remote<logic.Crew>) -[IO]> logic.TaskError! {
    _: crew.mine()
    Ok()
}
```

#### 5.1.4 Files

Pickers and drops give a `native.PickedFile`, never a path string (#6802
c1). It has a name, a size, a media type, `read` and `stream`. Behind it is
a path on desktops, a content URI on Android, a security-scoped URL on iOS
and a `File` handle in a browser. The import is one call to
`Crew.import_tasks`, so every row becomes a task or none does. An import
error becomes a notice through its own conversion.

`examples/files.jet`

```jet
// Proposed, not built (#6802 files and pickers; #6805 drag and drop).
use {
    app
    csv: core.encoding.csv
    logic
    native
    ui
}

#Codable
ImportRow: struct {
    title: String,
    notes: String,
}

#Error
ImportError: enum {
    Unreadable(String),
    BadRows,
    Board(logic.TaskError),
}

impl logic.TaskError -> ImportError {
    ImportError.Board(self)
}

impl ImportError -> app.Notice {
    if self == {
        .Unreadable(name) -> app.Notice{text: "Couldn't read {name}."}
        .BadRows -> app.Notice{text: "That file needs title and notes columns."}
        .Board(_) -> app.Notice{text: "Couldn't import the tasks. Try again.", retry: true}
    }
}

// Pick a CSV file and add its rows as tasks: one call, one transaction, so
// either every row becomes a task or none does. A desktop shows the system
// dialog, a phone the document picker, the web the browser's file chooser;
// each gives a native.PickedFile, never a path string. logic.TaskBatch's
// validate block refuses more than 500 rows, here and in the server's decoder.
import_tasks: fn(crew: app.Remote<logic.Crew>, project: logic.ProjectID) -[IO]> Int ImportError! {
    file: native.open_file(types: [native.FileType.CSV]) ?? return Ok(0)
    import_file(crew, project, file)
}

import_file: fn(crew: app.Remote<logic.Crew>, project: logic.ProjectID, file: native.PickedFile) -[IO]> Int ImportError! {
    text: file.text() ?? return Err(ImportError.Unreadable(file.name))
    rows: csv.decode<ImportRow>(text) ?? return Err(ImportError.BadRows)
    drafts := [logic.TaskDraft]{}
    loop row in rows {
        drafts.push(logic.TaskDraft{title: unescape(row.title), notes: unescape(row.notes)})
    }
    crew.import_tasks(project, logic.TaskBatch{tasks: drafts})
}

// The export (Desktop/desktop.jet) writes a cell that starts with =, +, -,
// @, a tab or a carriage return with one leading apostrophe, so a spreadsheet
// shows it as text (section 4.15). The import takes that one apostrophe off,
// so an exported board imports as it was. The cost: a title that really
// began with an apostrophe before one of those characters loses it.
unescape: fn(cell: String) -> String Never! {
    loop lead in ["'=", "'+", "'-", "'@", "'\t", "'\r"] {
        if cell.starts_with(lead) -> return String.from_bytes(cell.bytes().slice(1, cell.bytes().len())) ?? cell.copy()
    }
    cell.copy()
}

// Desktop: drop CSV files from the file manager onto the board. A failed
// file shows its notice by the board.
drop_csv: fn(crew: app.Remote<logic.Crew>, project: logic.ProjectID, board: ui.Node) -> ui.Node {
    native.drop_files(board, types: [native.FileType.CSV], on_drop: fn(dropped: [native.PickedFile]) {
        loop file in dropped {
            _: import_file(crew, project, file)
        }
    })
}
```

#### 5.1.5 Camera and permissions

Crewboard uses the camera and nothing else. Its phone manifest names only
that purpose, so a call to `native.location` would fail the build. The photo
goes through `Crew.attach`, the same checked upload path as 5.2.3.

`examples/camera.jet`

```jet
// Proposed, not built (#6803 camera; #6806 permissions; #6813 files).
use {
    app
    logic
    native
}

// Phone: attach a photo to a task. The phone manifest says why the app uses
// the camera; a call to native.take_photo without that line fails the build.
// Crewboard has no location feature, so its manifest asks for no location
// and a call to native.location would fail the build the same way.
photo_for_task: fn(crew: app.Remote<logic.Crew>, task: logic.TaskID) -[IO]> logic.TaskError! {
    if native.ask(native.Permission.Camera) == {
        .Granted -> {}
        .Denied -> {
            app.toast("Crewboard can't use the camera. You can allow it in Settings.")
            return Ok()
        }
        .Restricted -> {
            app.toast("This device doesn't allow camera use.")
            return Ok()
        }
    }
    photo: native.take_photo() ?? return Ok()
    _: crew.attach(task, photo.upload())
    Ok()
}
```

#### 5.1.6 Links

The address maps to `TaskView{id}` through the one route set (4.4), with the
task's UUIDv7 ID. The desktop app runs once. A second launch with
`crewboard://tasks/<id>` hands the address to the running app, which focuses
its window and opens the task, and then the second process exits (#6804 c3).

`examples/links.jet`

```jet
// Proposed, not built (#6804 deep links and app links).
use {
    app
    native
}

// https://crew.example.com/tasks/<id> opens the phone app when it is
// installed (Phone/phone.jet adds native.app_links). The server output serves
// /.well-known/apple-app-site-association and /.well-known/assetlinks.json,
// written from the phone output's id and signing identity. The address maps
// to TaskView{id} through the one route table every target uses.
//
// Desktop: crewboard://tasks/<id> does the same through a registered scheme.
// The desktop app runs once per account: a second launch hands its address
// to the running app, which opens it in a window, and then exits.
with_scheme: fn(graph: app.App) -> app.App Never! {
    graph.with(native.url_scheme("crewboard"))
}
```

#### 5.1.7 Drag and drop

Clipboard is in 5.1.1 (`native.copy_text`). Share is the "Share…" item of
a card's context menu (`card_menu` in `examples/tray.jet`, 5.1.1), which
hands the task's address to `native.share`. Dropping files is `drop_csv` in
5.1.4.

Every drag has a keyboard and single-pointer alternative, as WCAG 2.2 SC
2.5.7 requires [WCAG-DRAG]:

- Space picks the card up, the arrow keys move it through the drop zones by
  their labels, and Enter drops it.
- A long press offers the same zones as a menu.
- That is why `label:` is required on both `ui.draggable` and `ui.drop_zone`.

This file keeps a known seed error. `fn(task: logic.Task) {...}` passed to a
generic `on_drop` is `jet.UninferableTypeArgument` today. That is the
closure-parameter inference gap (#6819 c4; A.4).

`examples/drag.jet`

```jet
// Proposed, not built (#6805 drag and drop; #6807 accessibility).
use {
    app
    logic
    ui
}

// Drag a card to another column. The drag carries the task; a drop zone
// names the type it accepts, so a file or text dropped on it is refused.
// Every drag also works without one (WCAG 2.5.7): Space picks the card up,
// the arrow keys step through the drop zones by their labels and Enter
// drops; a long press offers the same zones as a menu. So both labels are
// required arguments.
draggable_card: fn(task: logic.Task, body: ui.Node) -> ui.Node {
    ui.draggable(body, value: task, label: task.title)
}

// A refused move puts the card back and shows the notice by the column.
droppable_column: fn(crew: app.Remote<logic.Crew>, status: logic.Status, body: ui.Node) -> ui.Node {
    ui.drop_zone(body, label: logic.status_label(status), on_drop: fn(task: logic.Task) {
        _: crew.move_task(task.id, task.version, status)
    })
}
```

#### 5.1.8 Accessibility

`kit` views carry their role, name and state into each platform's
accessibility tree. The adapters are written in Jet (D-A11Y-EXPORT1=B), and
the web uses semantic elements.

`examples/access.jet`

```jet
// Proposed, not built (#6807 accessibility; D-A11Y-EXPORT1=B).
use {
    app
    kit
    logic
    ui
}

// kit views carry their role and name: a kit.button is a Button named by its
// label on every target. An icon-only button must say what it does: `label`
// has no default, so leaving it out is a compile error.
move_button: fn(crew: app.Remote<logic.Crew>, task: logic.Task, next: logic.Status) -> ui.Node {
    kit.icon_button(kit.Icon.Move, label: "Move {task.title} to {logic.status_label(next)}", on_click: fn() {
        _: crew.move_task(task.id, task.version, next)
        // Screen readers hear the change; focus stays where it was.
        ui.announce("Moved to {logic.status_label(next)}")
    })
}

// A drawn node with no text gets its name from ui.label.
avatar: fn(name: String, picture: ui.Node) -> ui.Node {
    ui.label(picture, "Assigned to {name}")
}
```

#### 5.1.9 Localization

`examples/words.jet`

```jet
// Proposed, not built (#6808 localization; D-I18N1=A).
use {
    logic
    ui
}

// A literal in a view is a message. `jet i18n --add=de` writes a catalog with
// one entry per literal, and the German build shows the translations. Text
// from data passes through ui.verbatim and is never looked up.
due_summary: fn(tasks: [logic.Task]) -> ui.Node {
    n: tasks.len()
    ui.column([
        ui.text("{n} tasks due today", one: "1 task due today", plural: n),
        // Dates and numbers follow the reader's locale.
        ui.text(ui.date(ui.today(), .Long)),
    ])
}
```

#### 5.1.10 Lifecycle

Live queries resubscribe, and the queue sends on resume, with no code.
D-ONE-LIFECYCLE1=A reuses the shared operations, so there is no universal
runtime object.

`examples/lifecycle.jet`

```jet
// Proposed, not built (#6809 lifecycle; D-ONE-LIFECYCLE1=A).
use {
    app
    kit
    logic
    native
    ui
}

// A half-typed comment survives the phone stopping the app in the
// background: ui.saved keeps it with the screen state and restores it.
comment_box: fn(task: logic.Task) -> ui.Node {
    draft := ui.saved("")
    kit.search_field(draft.get(), label: "Comment", on_change: fn(text: String) { draft.set(text) })
}

// Live queries resubscribe and the offline queue sends on resume with no
// code. An app that needs more registers it. A failed on_resume shows its
// notice at the top of the window that came back.
with_resume: fn(graph: app.App) -> app.App Never! {
    graph.with(native.lifecycle(on_resume: check_session))
}

check_session: fn(crew: app.Remote<logic.Crew>) logic.TaskError! {
    _: crew.my_projects()
    Ok()
}
```

### 5.2 Web needs

| Need | Jet | Default | Example | Card | Best existing |
|---|---|---|---|---|---|
| SEO and metadata | `.head(fn)` returning `app.Head{title, description, image, canonical, index}`; `.sitemap(fn)` | signed-in screens are `noindex` and never in the sitemap | 5.2.1 | #6811 | Next.js metadata [NEXT-META] |
| Streaming server rendering | `.stream(after:)` | the Loading branch of `app.state` is the fallback | 5.2.2 | #6812 | React streaming [REACT-STREAM]; Next.js `loading.js` [NEXT-LOADING]; Leptos SSR modes [LEP-SSR] |
| Images and assets | `ui.asset(path)` checked at compile time; `ui.image(source, alt:)` | hashed names, `immutable`; WebP per D-WEB-IMAGE1 | 5.2.2 | #6816, #4695 | Vite asset handling [VITE] |
| Uploads and downloads | an `app.Upload` argument with `#App.UploadCheck(method:)`; `app.Blobs.keep(file, allow:)`; `kit.file_button(..., max_bytes:)`; `.api(..., max_upload:)`; `app.blob_url` | 413 over the limit before the body is read; the policy before the body, and again in the transaction; blob names made by the server; types checked from the bytes; downloads as attachments with `nosniff` | 5.2.3 | #6813 | Active Storage [RAILS-AS]; LiveView uploads [PHX-UP]; tus resumable uploads [TUS] |
| Caching headers | `.cache<V>(policy)` | signed-in: `private, no-store`; assets: `public, max-age=31536000, immutable` | 5.2.1 | #6814 | MDN HTTP caching [MDN-CACHE] [MDN-CC] |
| Install as an app | `.installable(name:, icon:, color:)` | manifest, icons and a service worker | 5.2.2 | #6815, #6048 | MDN installable PWAs [MDN-PWA]; web.dev [WEBDEV-PWA]; SvelteKit service workers [SK-SW] |

#### 5.2.1 Metadata, sitemap and caching

`examples/seo.jet`

```jet
// Proposed, not built (#6811 SEO and metadata; #6814 caching).
use {
    app
    kit
    ui
}

// A public page. Signed-in screens are noindex by default and never appear
// in the sitemap; a public screen states its metadata. Screens/shell.jet
// would apply with_landing, so /welcome is in the one route set.
LandingView: struct {}

landing: fn(view: LandingView) -> ui.Node {
    ui.column([
        kit.heading("Crewboard"),
        ui.text("A task board for small teams, on the web, the desktop and phones."),
        ui.image(ui.asset("brand/board.png"), alt: "A Crewboard board with three columns"),
    ])
}

landing_head: fn(view: LandingView) -> app.Head Never! {
    app.Head{
        title: "Crewboard: a task board for small teams",
        description: "Plan work in three columns. Works offline on the desktop and phones.",
        image: "brand/board.png",
    }
}

public_pages: fn() -> [LandingView] Never! {
    [LandingView{}]
}

with_landing: fn(graph: app.App) -> app.App Never! {
    graph
        .screen("/welcome", landing)
        .head(landing_head)
        .sitemap(public_pages)
        // A shared cache may keep the page for ten minutes, then revalidates
        // with its ETag. Signed-in pages stay `private, no-store`.
        .cache<LandingView>(.Public(max_age: 10min))
}
```

#### 5.2.2 Streaming, install and assets

`examples/web_delivery.jet`

```jet
// Proposed, not built (#6812 streaming; #6815 install; #6048 service worker; #6816 assets).
use {
    app
    ui
}

// Streaming: after 50 ms the server sends the HTML it has, with each
// screen's Loading branch in place of reads still running, then streams each
// read's HTML as it finishes. The Loading branch of app.state is the
// fallback; screens need no other code.
//
// Install: the web output writes the manifest, the icons and a service
// worker. The worker caches the app shell and the hashed assets, and holds
// the offline queue when the graph sets .offline(.Queue).
with_web_delivery: fn(graph: app.App) -> app.App Never! {
    graph
        .stream(after: 50ms)
        .installable(name: "Crewboard", icon: "brand/icon.png", color: 0x2a5bd7)
}

// An asset reference is checked at compile time: a missing file is an error.
// Built files get a content hash in their name and are served with
// `Cache-Control: public, max-age=31536000, immutable`.
logo: fn() -> ui.Node {
    ui.image(ui.asset("brand/logo.svg"), alt: "Crewboard")
}
```

#### 5.2.3 Uploads and downloads

Attachments are part of Crewboard: `Crew.attach` in `Logic/crew.jet` (3.2),
the attach button and the download list in `Screens/task.jet` (3.3). Round
2's separate upload example had three bugs:

- a path built from the client's file name;
- no role check;
- a type check done only in the browser.

The framework now closes each one, in this order (#6813 c1–c4):

1. **Size.** A body over the api's `max_upload` gets 413 before it is read.
2. **Policy, before the body.** `attach` is marked
   `#App.UploadCheck(method: "may_attach")`. The task ID travels in the URL,
   so after the session check the framework runs `may_attach(task)` as the
   caller, in a read-only transaction. It loads the task, which runs the task
   policy, and checks `can_edit`. A caller who is not an editor of that
   project gets 403, and the body is never read (#6813 c3).
3. **Spooling.** Only then does the framework spool the body to a staging
   area, before the method's transaction starts, so a serialization retry
   reads the staged body again rather than a request stream that is gone. A
   blob kept inside a transaction is tied to it: a rollback deletes the
   blob, so no file is stored without its row (#6813 c5). A staged file
   lives no longer than its request: it is deleted when the request ends,
   unless `keep` moved it in a transaction that committed, and one that a
   crash leaves behind is deleted when the server next starts.
4. **Policy, again, in the transaction.** `attach` loads the task and
   checks `can_edit` once more, and the attachments policy judges the
   insert. A role that changed while the body was spooling is caught here,
   and the staged file is deleted with the request.
5. **Naming and type.** `self.files.keep(file, allow:)` moves the body into
   storage under a name it makes itself. The client's file name is kept only
   as metadata, so `../../etc/passwd` stores safely [OWASP-PT]. `keep` also
   checks the media type from the first bytes against `allow`, and answers
   415 for anything else. What the client declared is ignored.
6. **Downloads.** `app.blob_url(id)` serves a blob only to a caller whose
   read policy admits the row that holds its ID. It is served with
   `Content-Disposition: attachment` and `X-Content-Type-Options: nosniff`
   [MDN-BLOB].

Active Storage names blobs itself and keeps the file name as metadata in the
same way [RAILS-AS]. Resumable uploads (tus) are not in this shape. A 10 MB
attachment is sent in one request, and #6813 decides whether large files
need resumable uploads.

## 6. Styling (card c7)

Today Jet has no style system. `Core/Ui` has `Color` and `SizeConstraint`,
and the web host places every element absolutely with inline pixel styles
(`Compiler/JetWeb/Source/Web/Host.jet:389–393`). Four decisions are ratified:

- D-THEME2: one Jet look, typed overrides, and `native.theme()` as the
  system-like preset.
- D-LAYOUT2: `ui.grid` becomes one column below 600 container points.
- D-FRAME-THEME1: the app follows the system light or dark setting live.
- D-A11Y-EXPORT1.

This section proposes the rest (#6817). It is choice S1 on the ballot.

### 6.1 The model (S1 A, recommended)

- **Tokens are a struct the app declares.** `CrewTheme` lists every color and
  space the app uses. A `ui.Paint` has a light and a dark value, and both are
  required, so a token with no dark value does not compile. A paint may also
  carry increased-contrast values. The frame picks one value from the live
  system settings (D-FRAME-THEME1).
- **A style is a plain function from the tokens to a `ui.Style`.**
  `card_style: fn(t: CrewTheme) -> ui.Style` is an ordinary function. A node
  takes it as `style: ui.style(card_style)`. There is no wrapper and no
  closure, and the seed checks this form today. Each field has its own type,
  so a `ui.Paint` cannot go where a `ui.Space` belongs. A literal sets each
  field once; setting one twice is `jet.InvalidConstructionField` today.
- **The theme type is checked.** The graph builder is the only place that
  sets a theme (`.theme(crew_theme())` in `Screens/shell.jet`, a `ui.Theme<CrewTheme>`), as
  it is the only place that registers screens (#6445 c6). Sema reads the
  token type from it. Every `ui.style`, `ui.tokens` and `ui.with_theme`
  reached from that graph's screens must use the same token type. A style
  built for another token type is a compile error (#6817 c1). The stubs
  cannot express this rule, so the seed does not report it.
- **States and motion are fields.** `states: ui.states(hover:, pressed:,
  focused:, disabled:)` holds partial styles. `motion: ui.ease(150ms)`
  animates a change. Under the system's reduced-motion setting every change
  is instant.
- **What wins.** Three layers can set a property of one node:
  1. the theme;
  2. the `kit` view's default;
  3. the style passed at the call site.

  The call site wins, then the view default, then the theme. There is no
  specificity and no selector order. `jet inspect style <file>:<line>`
  (proposed, #6817) prints each property with the layer that set it.
- **What inherits.** Text color, font, text size and writing direction pass
  from a container to the text inside it, as SwiftUI's environment passes
  them [SWIFTUI-ENV]. Nothing else inherits. `ui.with_theme(theme, body)`
  draws a subtree with other values of the same token type.
- **Layout.** `ui.column`, `ui.row` and `ui.grid` take `gap`, `align` and
  `width`. `ui.by_width(compact:, regular:, wide:)` picks a layout by the
  container's width, as CSS container queries do [MDN-CQ].

The Crewboard theme:

`crewboard/Screens/theme.jet`

```jet
// Proposed, not built (#6817 styling). Section 6 explains each part.
use {
    ui
}

// The app's design tokens: one typed struct. Each paint has a light and a
// dark value, and the frame picks one from the live system setting.
CrewTheme: struct {
    accent: ui.Paint,
    surface: ui.Paint,
    card: ui.Paint,
    text: ui.Paint,
    muted: ui.Paint,
    danger: ui.Paint,
    gap: ui.Space,
    radius: ui.Space,
}

crew_theme: fn() -> ui.Theme<CrewTheme> Never! {
    ui.theme(crew_tokens())
}

crew_tokens: fn() -> CrewTheme Never! {
    CrewTheme{
        accent: ui.paint(light: ui.rgb(0x2a5bd7), dark: ui.rgb(0x7aa2ff)),
        surface: ui.paint(light: ui.rgb(0xf3f4f6), dark: ui.rgb(0x15171c)),
        card: ui.paint(light: ui.rgb(0xffffff), dark: ui.rgb(0x1f232b)),
        text: ui.paint(light: ui.rgb(0x111318), dark: ui.rgb(0xeceef2)),
        // Grey text is the first to fail when a person asks for more contrast.
        muted: ui.paint(light: ui.rgb(0x5b6270), dark: ui.rgb(0xa3aab8), contrast_light: Val(ui.rgb(0x2f3440)), contrast_dark: Val(ui.rgb(0xd5d9e0))),
        danger: ui.paint(light: ui.rgb(0xb42318), dark: ui.rgb(0xff8a80)),
        gap: ui.px(12),
        radius: ui.px(8),
    }
}

// A style is a plain function from the tokens to a Style; a node takes it
// as `style: ui.style(card_style)`. Each field is set at most once in a
// literal, so a value has no internal cascade to resolve.
column_style: fn(t: CrewTheme) -> ui.Style Never! {
    ui.Style{
        background: t.surface,
        padding: t.gap,
        radius: t.radius,
        gap: t.gap,
        width: ui.at_least(ui.px(260)),
    }
}

card_style: fn(t: CrewTheme) -> ui.Style Never! {
    ui.Style{
        background: t.card,
        padding: t.gap,
        radius: t.radius,
        shadow: ui.Shadow.Low,
        states: ui.states(
            hover: ui.Style{shadow: ui.Shadow.Medium},
            focused: ui.Style{outline: ui.outline(t.accent, ui.px(2))},
        ),
    }
}

overdue_style: fn(t: CrewTheme) -> ui.Style Never! {
    ui.Style{text_color: t.danger}
}

muted_style: fn(t: CrewTheme) -> ui.Style Never! {
    ui.Style{text_color: t.muted, text: ui.TextStyle.Caption}
}
```

### 6.2 The board, seven ways

The board screen in every form has:

- three columns side by side when the container is wide;
- one column with a picker when it is narrower than 600 points;
- per card: a link with the title, a due date that turns red when the task
  is late, a move button, a hover shadow and a focus ring;
- light and dark values for every color.

**Jet, S1 A (recommended): plain style functions over tokens.** The view is
`wide_board`, `narrow_board`, `column`, `card` and `due_line` in
`Screens/board.jet` (3.3). The styles are `column_style`, `card_style`,
`overdue_style` and `muted_style` in `theme.jet` above.

**Jet, S1 B: modifier chains,** as in SwiftUI and Compose.

`examples/style_b.jet`

```jet
// Proposed, not built (#6817 styling, option S1-B: modifier chains).
use {
    app
    kit
    logic
    screens
    ui
}

// The board of Screens/board.jet, styled with modifiers in the manner of
// SwiftUI and Compose. Each modifier returns a new node; order matters.
board_b: fn(crew: app.Remote<logic.Crew>, data: logic.Board, q: String) -> ui.Node {
    shown := ui.local(logic.Status.Todo)
    ui.by_width(
        compact: ui.column([
            kit.segmented(shown, [logic.Status.Todo, logic.Status.Doing, logic.Status.Done], label: logic.status_label),
            column_b(crew, data, q, shown.get()),
        ]),
        regular: ui.row([
            column_b(crew, data, q, logic.Status.Todo),
            column_b(crew, data, q, logic.Status.Doing),
            column_b(crew, data, q, logic.Status.Done),
        ], gap: ui.px(12)),
    )
}

column_b: fn(crew: app.Remote<logic.Crew>, data: logic.Board, q: String, status: logic.Status) -> ui.Node {
    t: ui.tokens<screens.CrewTheme>()
    tasks: logic.column(logic.matching(data.tasks, q), status)
    ui.column([
        kit.heading(logic.status_label(status), level: 3),
        kit.list(tasks, key: fn(task: logic.Task) { task.id }, row: fn(task: logic.Task) { card_b(crew, data.role, task) }),
    ], gap: t.gap)
        .padding(t.gap)
        .background(t.surface)
        .corner_radius(t.radius)
        .min_width(ui.px(260))
}

card_b: fn(crew: app.Remote<logic.Crew>, role: logic.Role, task: logic.Task) -> ui.Node {
    t: ui.tokens<screens.CrewTheme>()
    next: logic.next_status(task.status)
    ui.column([
        kit.link(logic.TaskView{id: task.id}, ui.text(ui.verbatim(task.title))),
        due_b(task, t),
        if logic.can_move(role, task.status, next) -> kit.button("Move to {logic.status_label(next)}", on_click: fn() { _: crew.move_task(task.id, task.version, next) }) else -> ui.empty(),
    ], gap: t.gap)
        .padding(t.gap)
        .background(t.card)
        .corner_radius(t.radius)
        .shadow(ui.Shadow.Low)
        .on_hover(ui.Shadow.Medium)
        .focus_ring(t.accent)
}

due_b: fn(task: logic.Task, t: screens.CrewTheme) -> ui.Node {
    day: task.due ?? return ui.empty()
    late: day < ui.today() && task.status != logic.Status.Done
    ui.text(ui.date(day, .Short))
        .text_style(ui.TextStyle.Caption)
        .foreground(if late -> t.danger else -> t.muted)
}
```

**Jet, S1 C: a scoped, checked stylesheet.** The braces are doubled because
`{` starts an interpolation in a Jet string; that is real friction.

`examples/style_c.jet`

```jet
// Proposed, not built (#6817 styling, option S1-C: scoped, checked CSS).
use {
    app
    kit
    logic
    ui
}

// The board with a stylesheet. The compiler parses the CSS, checks each
// property and token name, scopes every class to this file, and makes
// sheet.class("name") an error when the sheet has no such class. Braces are
// doubled because `{` starts an interpolation in a Jet string. The layout
// switch is a container query, as in plain CSS.
board_sheet: fn() -> ui.Sheet Never! {
    ui.sheet("""
        .board {{ container-type: inline-size; }}
        .columns {{ display: flex; gap: 12px; }}
        .picker {{ display: none; }}
        @container (width < 600px) {{
            .columns {{ flex-direction: column; }}
            .picker {{ display: block; }}
            .column:not(.shown) {{ display: none; }}
        }}
        .column {{
            display: flex; flex-direction: column; gap: var(--gap);
            background: var(--surface); padding: var(--gap);
            border-radius: var(--radius); min-width: 260px;
        }}
        .card {{
            display: flex; flex-direction: column; gap: var(--gap);
            background: var(--card); padding: var(--gap);
            border-radius: var(--radius); box-shadow: var(--shadow-low);
        }}
        .card:hover {{ box-shadow: var(--shadow-medium); }}
        .card:focus-within {{ outline: 2px solid var(--accent); }}
        .due {{ color: var(--muted); font: var(--caption); }}
        .due-late {{ color: var(--danger); font: var(--caption); }}
        """)
}

board_c: fn(crew: app.Remote<logic.Crew>, data: logic.Board, q: String) -> ui.Node {
    sheet: board_sheet()
    shown := ui.local(logic.Status.Todo)
    ui.column([
        kit.segmented(shown, [logic.Status.Todo, logic.Status.Doing, logic.Status.Done], label: logic.status_label).class(sheet.class("picker")),
        ui.row([
            column_c(crew, data, q, logic.Status.Todo, shown.get(), sheet),
            column_c(crew, data, q, logic.Status.Doing, shown.get(), sheet),
            column_c(crew, data, q, logic.Status.Done, shown.get(), sheet),
        ]).class(sheet.class("columns")),
    ]).class(sheet.class("board"))
}

column_c: fn(crew: app.Remote<logic.Crew>, data: logic.Board, q: String, status: logic.Status, shown: logic.Status, sheet: ui.Sheet) -> ui.Node {
    tasks: logic.column(logic.matching(data.tasks, q), status)
    node: ui.column([
        kit.heading(logic.status_label(status), level: 3),
        kit.list(tasks, key: fn(task: logic.Task) { task.id }, row: fn(task: logic.Task) { card_c(crew, data.role, task, sheet) }),
    ]).class(sheet.class("column"))
    if status == shown -> node.class(sheet.class("shown")) else -> node
}

card_c: fn(crew: app.Remote<logic.Crew>, role: logic.Role, task: logic.Task, sheet: ui.Sheet) -> ui.Node {
    next: logic.next_status(task.status)
    ui.column([
        kit.link(logic.TaskView{id: task.id}, ui.text(ui.verbatim(task.title))),
        due_c(task, sheet),
        if logic.can_move(role, task.status, next) -> kit.button("Move to {logic.status_label(next)}", on_click: fn() { _: crew.move_task(task.id, task.version, next) }) else -> ui.empty(),
    ]).class(sheet.class("card"))
}

due_c: fn(task: logic.Task, sheet: ui.Sheet) -> ui.Node {
    day: task.due ?? return ui.empty()
    late: day < ui.today() && task.status != logic.Status.Done
    ui.text(ui.date(day, .Short)).class(sheet.class(if late -> "due-late" else -> "due"))
}
```

**HTML and CSS.** Custom properties hold the tokens, and a media query holds
the dark values [MDN-VARS]. A container query switches the layout [MDN-CQ].

```html
<section class="board">
  <select class="picker" aria-label="Column" onchange="show(this.value)">
    <option value="todo">To do</option><option value="doing">Doing</option><option value="done">Done</option>
  </select>
  <div class="columns">
    <section class="column shown" id="todo">
      <h3>To do</h3>
      <ul>
        <li class="card">
          <a href="/tasks/0193a5c2">Ship v2</a>
          <p class="due overdue">Nov 12</p>
          <button>Move to Doing</button>
        </li>
      </ul>
    </section>
    <section class="column" id="doing"><h3>Doing</h3><ul></ul></section>
    <section class="column" id="done"><h3>Done</h3><ul></ul></section>
  </div>
</section>
```

```css
:root {
  --surface: #f3f4f6; --card: #ffffff; --text: #111318; --muted: #5b6270;
  --danger: #b42318; --accent: #2a5bd7; --gap: 12px; --radius: 8px;
}
@media (prefers-color-scheme: dark) {
  :root {
    --surface: #15171c; --card: #1f232b; --text: #eceef2; --muted: #a3aab8;
    --danger: #ff8a80; --accent: #7aa2ff;
  }
}
.board { container-type: inline-size; }
.columns { display: flex; gap: 12px; }
.picker { display: none; }
@container (width < 600px) {
  .columns { flex-direction: column; }
  .picker { display: block; }
  .column:not(.shown) { display: none; }
}
.column {
  display: flex; flex-direction: column; gap: var(--gap); min-width: 260px;
  background: var(--surface); padding: var(--gap); border-radius: var(--radius);
}
.card {
  display: flex; flex-direction: column; gap: var(--gap);
  background: var(--card); color: var(--text);
  padding: var(--gap); border-radius: var(--radius);
  box-shadow: 0 1px 2px rgb(0 0 0 / 0.12);
}
.card:hover { box-shadow: 0 4px 12px rgb(0 0 0 / 0.16); }
.card:focus-within { outline: 2px solid var(--accent); }
.due { color: var(--muted); font-size: 0.875rem; }
.due.overdue { color: var(--danger); }
```

**Tailwind.** Utility classes, `dark:` for dark mode and `@xl:` for the
container query [TW] [TW-DARK].

```html
<section class="@container">
  <select class="@xl:hidden" aria-label="Column" onchange="show(this.value)">
    <option value="todo">To do</option><option value="doing">Doing</option><option value="done">Done</option>
  </select>
  <div class="flex flex-col gap-3 @xl:flex-row">
    <section id="todo" class="flex min-w-[260px] flex-col gap-3 rounded-lg bg-gray-100 p-3 dark:bg-gray-900">
      <h3>To do</h3>
      <ul>
        <li class="flex flex-col gap-3 rounded-lg bg-white p-3 text-gray-900 shadow-sm
                   hover:shadow-md focus-within:outline-2 focus-within:outline-blue-700
                   dark:bg-gray-800 dark:text-gray-100">
          <a href="/tasks/0193a5c2">Ship v2</a>
          <p class="text-sm text-red-700 dark:text-red-300">Nov 12</p>
          <button>Move to Doing</button>
        </li>
      </ul>
    </section>
    <section id="doing" class="hidden min-w-[260px] flex-col gap-3 rounded-lg bg-gray-100 p-3 @xl:flex dark:bg-gray-900"><h3>Doing</h3><ul></ul></section>
    <section id="done" class="hidden min-w-[260px] flex-col gap-3 rounded-lg bg-gray-100 p-3 @xl:flex dark:bg-gray-900"><h3>Done</h3><ul></ul></section>
  </div>
</section>
```

**SwiftUI.** Modifiers on the views. Named colors carry the dark values, and
the size class picks the layout [SWIFTUI-MOD] [SWIFTUI-CS]. The card has the
same focus ring as the other forms.

```swift
// "Surface", "Card", "Danger" and "Accent" are named colors in the asset
// catalog, each with a light and a dark value.
struct BoardView: View {
    let board: Board
    @Environment(\.horizontalSizeClass) private var size
    @State private var shown = Status.todo

    var body: some View {
        if size == .compact {
            VStack {
                Picker("Column", selection: $shown) {
                    ForEach(Status.allCases) { Text($0.label).tag($0) }
                }
                .pickerStyle(.segmented)
                ColumnView(board: board, status: shown)
            }
        } else {
            HStack(alignment: .top, spacing: 12) {
                ForEach(Status.allCases) { ColumnView(board: board, status: $0) }
            }
        }
    }
}

struct ColumnView: View {
    let board: Board
    let status: Status

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(status.label).font(.headline)
            ForEach(board.tasks(in: status)) { CardView(task: $0) }
        }
        .padding(12)
        .frame(minWidth: 260)
        .background(Color("Surface"), in: RoundedRectangle(cornerRadius: 8))
    }
}

struct CardView: View {
    let task: CrewTask
    @State private var hovering = false
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            NavigationLink(task.title, value: task.id)
            if let due = task.due {
                Text(due, style: .date)
                    .font(.caption)
                    .foregroundStyle(task.isOverdue ? Color("Danger") : .secondary)
            }
            Button("Move to \(task.status.next.label)") { move(task) }
        }
        .padding(12)
        .background(Color("Card"), in: RoundedRectangle(cornerRadius: 8))
        .shadow(radius: hovering ? 6 : 1)
        .onHover { hovering = $0 }
        .focusable()
        .focused($focused)
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color("Accent"), lineWidth: focused ? 2 : 0))
    }
}
```

**Flutter.** A `ThemeExtension` holds the app's own tokens, and a
`LayoutBuilder` picks the layout [FLUTTER-THEME] [FLUTTER-THEMES]
[FLUTTER-ADAPT].

```dart
// CrewColors is a ThemeExtension with a light and a dark instance,
// registered in ThemeData.extensions.
class BoardView extends StatefulWidget {
  const BoardView({super.key, required this.board});
  final Board board;

  @override
  State<BoardView> createState() => _BoardViewState();
}

class _BoardViewState extends State<BoardView> {
  Status shown = Status.todo;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(builder: (context, size) {
      if (size.maxWidth < 600) {
        return Column(children: [
          SegmentedButton<Status>(
            segments: [for (final s in Status.values) ButtonSegment(value: s, label: Text(s.label))],
            selected: {shown},
            onSelectionChanged: (s) => setState(() => shown = s.first),
          ),
          ColumnView(board: widget.board, status: shown),
        ]);
      }
      return Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        for (final s in Status.values)
          Padding(padding: const EdgeInsets.only(right: 12), child: ColumnView(board: widget.board, status: s)),
      ]);
    });
  }
}

class ColumnView extends StatelessWidget {
  const ColumnView({super.key, required this.board, required this.status});
  final Board board;
  final Status status;

  @override
  Widget build(BuildContext context) {
    final crew = Theme.of(context).extension<CrewColors>()!;
    return Container(
      constraints: const BoxConstraints(minWidth: 260),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(color: crew.surface, borderRadius: BorderRadius.circular(8)),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(status.label, style: Theme.of(context).textTheme.titleMedium),
        for (final t in board.tasksIn(status)) CardView(key: ValueKey(t.id), task: t),
      ]),
    );
  }
}

class CardView extends StatefulWidget {
  const CardView({super.key, required this.task});
  final CrewTask task;

  @override
  State<CardView> createState() => _CardViewState();
}

class _CardViewState extends State<CardView> {
  bool hovering = false;
  bool focused = false;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final crew = theme.extension<CrewColors>()!;
    final task = widget.task;
    return Focus(
      onFocusChange: (f) => setState(() => focused = f),
      child: MouseRegion(
        onEnter: (_) => setState(() => hovering = true),
        onExit: (_) => setState(() => hovering = false),
        child: Card(
          elevation: hovering ? 4 : 1,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(8),
            side: focused ? BorderSide(color: crew.accent, width: 2) : BorderSide.none,
          ),
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              TextButton(onPressed: () => open(task), child: Text(task.title)),
              if (task.due != null)
                Text(formatDate(task.due!),
                    style: theme.textTheme.bodySmall?.copyWith(color: task.isOverdue ? crew.danger : crew.muted)),
              FilledButton(onPressed: () => move(task), child: Text("Move to ${task.status.next.label}")),
            ]),
          ),
        ),
      ),
    );
  }
}
```

Lines in each form, counted without blank lines, comments and `use` blocks:

| Form | Tokens with light and dark values | Board styles and view | Left out of the count |
|---|---|---|---|
| Jet S1 A | 25 (`CrewTheme`, `crew_theme`, `crew_tokens`; shared by every screen) | 62: 27 styles + 35 view | — |
| Jet S1 B | the same 25 | 48, styles and view together | — |
| Jet S1 C | the same 25 | 59: 26 sheet + 33 view | the token-to-variable mapping, generated |
| HTML and CSS | 10 | 41: 22 rules + 19 markup | the script that switches the shown column; the server template that repeats a card |
| Tailwind | 0: the default palette | 21 | the same script and template; a custom palette, if the app needs one |
| SwiftUI | 0 in code: an asset catalog | 56 | the asset catalog entries |
| Flutter | not shown | 86 | the `CrewColors` extension, about 25 lines |

**What the count shows.** S1 A is not the shortest form.

- Tailwind, and HTML with CSS, style the board in fewer lines, though each
  leaves out a script and a template.
- S1 B is the shortest Jet form.
- S1 A sits next to SwiftUI and below Flutter.

S1 A spends its lines on named tokens and named styles, which the rest of the
app reuses. Round 2 counted S1 A with a `ui.style<CrewTheme>(fn(t) {...})`
wrapper around each style. That wrapper is gone: a style is now a plain
function, and the seed checks it. Whether S1 A is "at least as easy as HTML
and CSS" is measured in 6.5, not assumed.

### 6.3 Every row of card c7

| Row | Jet (S1 A) | Example | Best existing | Verdict |
|---|---|---|---|---|
| Layout: stack, grid, alignment, spacing | `ui.column`, `ui.row`, `ui.grid(columns:, gap:)`, `align:`, `ui.spacer()` | `project_grid`, `header` below | CSS grid and flexbox; Compose layouts | matches the common cases; no subgrid or named areas |
| Typography and fonts | `ui.TextStyle` (Title, Body, Caption…); `ui.font(family, files:)` in the theme; sizes follow the system text size | `branded_theme` below | SwiftUI Dynamic Type; CSS `font-face` | matches |
| Color, themes, dark mode | `ui.Paint` with required light and dark values; the frame follows the system live | `crew_tokens` (6.1) | CSS `prefers-color-scheme`; Tailwind `dark:` [TW-DARK]; SwiftUI `colorScheme` [SWIFTUI-CS]; vanilla-extract `createThemeContract` [VE-CONTRACT] | matches vanilla-extract: its theme contract also refuses a theme that leaves a token out (CL9) |
| Increased contrast and forced colors | `ui.paint(contrast_light:, contrast_dark:)`; under forced colors the frame uses the system palette and keeps focus rings and borders | `muted` in `crew_tokens` (6.1) | CSS `prefers-contrast` [MDN-CONTRAST] and `forced-colors` [MDN-FORCED]; SwiftUI `colorSchemeContrast` [SWIFTUI-CONTRAST] | matches |
| Responsive and adaptive | `ui.by_width` on the container's width; `ui.grid` goes to one column under 600 points (D-LAYOUT2) | the board (3.3, 6.2) | CSS container queries [MDN-CQ]; Compose window size classes [COMPOSE-WSC]; Flutter adaptive [FLUTTER-ADAPT] | matches container queries |
| States | `ui.states(hover:, pressed:, focused:, disabled:)` | `card_style` (6.1), `moving_card_style` below | CSS pseudo-classes; Compose interaction sources | matches |
| Motion | `motion: ui.ease(duration)`; reduced motion makes changes instant | `moving_card_style` below | CSS transitions and `prefers-reduced-motion` [MDN-RM]; Qt Quick animations [QT-ANIM] | matches; reduced motion is the default, not opt-in |
| Overlays | `kit.dialog`, `kit.sheet` and `kit.popover`, and the frame's notices, draw in one top layer above the screen, in opening order; a dialog holds focus until it closes, and Escape or Back closes it | `confirm_delete` below | the HTML `dialog` element and the top layer [MDN-DIALOG]; SwiftUI `.sheet`; Compose `Dialog` | matches the `dialog` element |
| Scroll containers and sticky headers | `ui.scroll(body, sticky:)`; the sticky child stays at the edge while the rest scrolls | `scrolling_board` below | CSS `overflow` and `position: sticky`; SwiftUI `ScrollView` with pinned headers | matches |
| View identity | a `kit.list` row is identified by its `key:`; `ui.local` state belongs to that row, so filtering or moving cards keeps each card's state | `column` in `Screens/board.jet` (3.3) | SwiftUI `ForEach` identity [SWIFTUI-ID]; Compose `key` [COMPOSE-KEY]; React keys | matches; `key:` is a required parameter of `kit.list`, so a list without one is `jet.MissingArgument` with the seed today |
| Reusable styles and tokens | a token struct; styles are plain functions | `theme.jet` | Tailwind theme; vanilla-extract and StyleX [VE] [STYLEX]; Flutter `ThemeExtension` | matches vanilla-extract |
| Per-platform adaptation | `native.theme()` preset; `comptime if` per target (D-PLATFORM-OVERRIDE1) | `system_theme` below | SwiftUI and Compose follow the platform by default; Qt Quick Controls styles [QT-STYLE] | behind: the preset is one look per platform, not the full native control set |
| Accessibility: contrast, focus, reduced motion | focus rings on by default; reduced motion honored; contrast checked by a lint (6.4) | `access.jet` (5.1.8) | WCAG 2.2 contrast minimum [WCAG-CONTRAST]; platform a11y tools | partly behind: the contrast lint is #6817 c2 work |
| A keyboard or single-pointer alternative to dragging | `ui.draggable` and `ui.drop_zone` both require `label:`; Space picks a card up, the arrow keys step through the drop zones by label and Enter drops; a long press offers the same zones as a menu | `drag.jet` (5.1.7) | WCAG 2.2 SC 2.5.7 [WCAG-DRAG] | matches the success criterion |

`examples/style_more.jet`

```jet
// Proposed, not built (#6817 styling; D-LAYOUT2=A; D-THEME2=A; D-FRAME-THEME1=A).
use {
    kit
    logic
    native
    screens
    ui
}

// Layout: a grid of project cards. Below 600 container points it becomes
// one column (D-LAYOUT2's default); `columns` is the wide count.
project_grid: fn(cards: [ui.Node]) -> ui.Node {
    ui.grid(cards, columns: 3, gap: ui.px(16))
}

// A header row: title at the start, actions at the end.
header: fn(title: String, actions: [ui.Node]) -> ui.Node {
    ui.row([kit.heading(ui.verbatim(title)), ui.spacer(), ui.row(actions, gap: ui.px(8))], align: ui.Align.Center)
}

// Typography and fonts: the theme names one family; text styles scale with
// the person's text-size setting on every target.
branded_theme: fn() -> ui.Theme<screens.CrewTheme> {
    ui.theme(screens.crew_tokens(), font: ui.font("Inter", files: [ui.asset("fonts/Inter.woff2")]))
}

// Inheritance: text color, font, text size and direction pass from a
// container to the text inside it, as SwiftUI's environment does. A subtree
// can use other token values: here, the danger zone of a settings page. It
// cannot use another token type: the graph's theme is a CrewTheme, so a
// with_theme or a style built for another type is a compile error.
danger_zone: fn(body: ui.Node) -> ui.Node {
    tokens: screens.crew_tokens()
    ui.with_theme(ui.theme(screens.crew_tokens(), accent: tokens.danger), body)
}

// Per-platform look: D-THEME2's system-like preset, chosen by the entry.
system_theme: fn() -> ui.Theme<screens.CrewTheme> {
    ui.theme(screens.crew_tokens(), preset: native.theme())
}

// Motion: a card eases its shadow over 150 ms. When the person asks for
// reduced motion, every motion becomes an instant change.
moving_card_style: fn(t: screens.CrewTheme) -> ui.Style Never! {
    ui.Style{
        background: t.card,
        shadow: ui.Shadow.Low,
        motion: ui.ease(150ms),
        states: ui.states(
            hover: ui.Style{shadow: ui.Shadow.Medium},
            pressed: ui.Style{shadow: ui.Shadow.Flat},
            disabled: ui.Style{opacity: 50},
        ),
    }
}

// Scrolling: the board scrolls under a header that stays in place.
scrolling_board: fn(header: ui.Node, columns: ui.Node) -> ui.Node {
    ui.scroll(columns, sticky: Val(header))
}

// Overlays: a dialog holds focus until it closes; Escape or Back closes it.
confirm_delete: fn(open: ui.Local<Bool>, on_delete: ui.Node) -> ui.Node {
    kit.dialog("Delete this task?", open: open, body: ui.column([ui.text("This can't be undone."), on_delete]))
}
```

### 6.4 The guarantees, each within its limits

The review listed seven attacks on any claim that a style system beats CSS.
Each guarantee here is stated at the size the attacks leave.

1. **Typed values.** A style field has a type: `padding` takes a `ui.Space`,
   `background` takes a `ui.Paint`, and a token reference must name a field
   of the token struct. vanilla-extract, StyleX and SwiftUI also type style
   values, so this is not new; it is the floor.
2. **Conflicts cannot be written inside one style.** A literal sets a field
   once. Across the three layers (theme, view default, call site) there is
   one fixed order, and `jet inspect style` shows which layer set what. The
   claim is "no hidden conflict", not "no conflict".
3. **No cascade surprises, with inheritance kept.** No selectors and no
   specificity. Text color, font, size and direction still inherit, through
   the environment, and `ui.with_theme` overrides a subtree.
4. **Scoped by construction, on the web too.** A style is attached to the
   node that uses it. The web output writes one generated class per style,
   named from a hash of the style, into one stylesheet. No app style can
   match another element. Third-party HTML (an embed, a widget) is placed in
   a shadow root, so its CSS cannot leak in or out. S1 C's sheets are scoped
   the same way.
5. **No dead rules, under S1 A.**
   - A style is a function, and a token is a field. An unused style
     function, or a token field that no style reads, is reported like any
     unused declaration (#6817 c1).
   - The web output writes only the styles that reachable nodes use.
   - S1 C cannot promise this until #6817 checks class use in both
     directions: every class used exists, and every class defined is used.
6. **Compile-time layout errors, at their real size.** Sema can catch:
   - a `ui.grid` with zero columns;
   - a fixed width smaller than a declared minimum;
   - a token that is not a size;
   - a missing `alt:`, `label:` or `key:`.

   Most layout failures depend on content: long words, a German translation,
   a 320-point phone, 200% text. Preview tests catch the rest. Each screen
   is rendered at three widths and two text sizes, in both themes, and
   compared with a stored image (#6817).
7. **No overstated claims.** Section 6.5 has the measures. No claim of being
   better appears without one.
8. **One style model, two layout engines.** The web uses the browser's layout
   over semantic HTML (D-ONEAPP-WEBOUT1); desktop and phones use Jet's. They
   will differ in text shaping, line breaking, scrollbars and subpixel
   rounding. The rules:
   - A `ui.Style` maps to one CSS declaration set.
   - Jet's layout implements the same flexbox and grid subset.
   - The preview tests run on both engines and report differences above two
     points.
   - Fonts are bundled, so both engines shape the same glyphs.

### 6.5 Measures for "better"

**Seeded style bugs.** The suite has eight common style bugs, one per file.
Each is checked with the seed against the stubs (the `style_bugs` package,
Appendix A). Each bug is a plain style function over `CrewTheme`, in the form
6.1 recommends. The other columns are what each system reports for the same
bug, as documented behavior.

| # | Bug | Jet S1 A with the seed today | CSS in a browser | Tailwind | SwiftUI |
|---|---|---|---|---|---|
| b1 | a token name with a typo (`t.crad`) | `jet.UnknownField`: "`CrewTheme` has no field `crad`" | `var(--crad)` is unset; the property falls back with no error | `bg-crad` generates no CSS; the editor plugin warns | `Color("Crad")` compiles; generated asset symbols make it a compile error |
| b2 | a color where a length belongs | `jet.BindingTypeMismatch`: "This needs Space?, but the value is `Paint`" | the declaration is invalid and ignored | the class generates nothing | compile error |
| b3 | one property set twice | `jet.InvalidConstructionField`: "Field `padding` appears more than once" | the later declaration wins, silently | both classes apply; the generated CSS order decides | two `.padding` modifiers both apply |
| b4 | an image with no text alternative | `jet.MissingArgument`: "missing `alt`" | valid HTML to the browser; checkers such as axe flag it | the same | compiles |
| b5 | an icon-only button with no name | `jet.MissingArgument`: "missing `label`" | no accessible name; checkers flag it | the same | compiles |
| b6 | a state that does not exist (`hovered:`) | `jet.UnknownArgumentLabel` | an invalid selector drops the whole rule | an unknown variant generates nothing | no such API |
| b7 | a class the S1 C sheet does not define | **not caught** by the stub; #6817's sheet parser must make it an error | matches nothing | matches nothing | — |
| b8 | muted text on a muted background | **not caught**; #6817's contrast lint must check each text paint on its background in both themes against WCAG 1.4.3 [WCAG-CONTRAST] | not caught; audit tools flag it | the same | Accessibility Inspector audits flag it |

The seed's type checker catches six of the eight today, given the proposed
stubs. The other two need #6817's own checks. b4 and b5 are not unique to
Jet: Compose's `Image` takes a required `contentDescription` parameter
[COMPOSE-IMAGE], so a missing one is a compile error there too. CL9 matches
Compose here; it does not go further.

This is a small seeded set chosen by the author, not a study; measure 1 below
is the study.

<details>
<summary>The eight bug files</summary>

`style_bugs/b1_token_typo.jet`

```jet
// Seeded bug for section 6.5 (#6817). It must not check.
use {
    kit
    screens
    ui
}

// A token name with a typo.
b1: fn(t: screens.CrewTheme) -> ui.Style {
    ui.Style{background: t.crad}
}
```

`style_bugs/b2_wrong_kind.jet`

```jet
// Seeded bug for section 6.5 (#6817). It must not check.
use {
    kit
    screens
    ui
}

// A color where a length belongs.
b2: fn(t: screens.CrewTheme) -> ui.Style {
    ui.Style{padding: t.accent}
}
```

`style_bugs/b3_conflict.jet`

```jet
// Seeded bug for section 6.5 (#6817). It must not check.
use {
    kit
    screens
    ui
}

// One property set twice in one style.
b3: fn(t: screens.CrewTheme) -> ui.Style {
    ui.Style{padding: t.gap, radius: t.radius, padding: ui.px(4)}
}
```

`style_bugs/b4_image_alt.jet`

```jet
// Seeded bug for section 6.5 (#6817). It must not check.
use {
    kit
    screens
    ui
}

// An image with no text alternative.
b4: fn() -> ui.Node {
    ui.image(ui.asset("brand/logo.svg"))
}
```

`style_bugs/b5_icon_label.jet`

```jet
// Seeded bug for section 6.5 (#6817). It must not check.
use {
    kit
    screens
    ui
}

// An icon-only button with no name for screen readers.
b5: fn() -> ui.Node {
    kit.icon_button(kit.Icon.Trash, on_click: fn() {})
}
```

`style_bugs/b6_unknown_state.jet`

```jet
// Seeded bug for section 6.5 (#6817). It must not check.
use {
    kit
    screens
    ui
}

// A state that does not exist.
b6: fn(t: screens.CrewTheme) -> ui.Style {
    ui.Style{states: ui.states(hovered: ui.Style{shadow: ui.Shadow.Medium})}
}
```

`style_bugs/b7_sheet_class.jet`

```jet
// Seeded bug for section 6.5 (#6817). It must not check.
use {
    kit
    screens
    ui
}

// A class the sheet does not define.
b7_sheet: fn() -> ui.Sheet {
    ui.sheet(".card {{ padding: 12px; }}")
}

b7: fn(body: ui.Node) -> ui.Node {
    body.class(b7_sheet().class("crad"))
}
```

`style_bugs/b8_contrast.jet`

```jet
// Seeded bug for section 6.5 (#6817). It must not check.
use {
    kit
    screens
    ui
}

// Muted text on the muted color: unreadable in both themes.
b8: fn(t: screens.CrewTheme) -> ui.Style {
    ui.Style{background: t.muted, text_color: t.muted}
}
```

</details>


**Measures #6817 must report before any claim of being better than CSS:**

1. Bugs caught: the seeded corpus above grown to 50 bugs, run against S1 A,
   CSS with Stylelint, Tailwind with its editor plugin, and SwiftUI.
2. Lines: the four Crewboard screens styled in each form, counted as in 6.2.
   The owner's floor is that S1 A is at least as easy as HTML and CSS. On
   lines it is not met for the board today: 87 against 51. Measure 5 decides
   it.
3. Dark theme: the number of places edited to add a dark theme to a
   light-only app. Under S1 A it is one per token, and the compiler lists
   each one.
4. Web and native match: the share of preview images that differ by more than
   two points between the browser and Jet's layout.
5. Ease: time to a first correct board and the number of errors along the
   way, for people who know HTML and CSS, given S1 A, S1 B and Tailwind.

## 7. Open choices, and the ballot

Ratified decisions are not reopened. Each choice below is one program in two
or three forms. The recommended form is the one sections 3 to 6 use, and
those blocks are checked. The other forms are marked "not checked" with the
card that would build them.

### 7.1 O1. Where the `app` package lives — decided

D-WEB-RUNTIME-API1=A keeps `app` in Core, and `core.web.browser` becomes
`core.browser`. Revision 1 offered this as a choice; it is no longer on the
ballot.

### 7.2 O2. Which methods are endpoints

**A (recommended): explicit markers.** Only `#App.SignedIn` and `#App.Public`
methods are endpoints. This is the form in 3.2.

**B: signed-in by default.** Every non-`priv` method is a signed-in endpoint;
`#App.Public` opens one up and `priv` closes one.

```jet
// Option B, not checked (#6445).
Crew: struct {
    tasks: db.Table<TaskID, Task>,

    // An endpoint for signed-in callers, with no marker.
    move_task: fn(self, id: TaskID, version: Int, to: Status) -[DB.Read, DB.Write]> Task TaskError! {
        self.apply_move(id, version, to)
    }

    // Must be priv, or it is an endpoint too.
    priv apply_move: fn(self, id: TaskID, version: Int, to: Status) -[DB.Read, DB.Write]> Task TaskError! {
        Err(TaskError.NotFound)
    }
}
```

**C: inferred.** A method is public unless its body reads `app.caller()`
(revision 1's rule).

| | A, explicit | B, signed-in default | C, inferred |
|---|---|---|---|
| A forgotten marker | not an endpoint: the client call fails to compile | a signed-in endpoint | public |
| A helper that should be private | private unless marked | public to signed-in callers unless `priv` | public |
| Refactoring the auth read into a helper | no change | no change | can change exposure silently |
| Lines in Crewboard | one marker per endpoint (15) | one `priv` per helper (6) | none |
| Jobs, webhooks, signing keys | marked, never endpoints | marked, never endpoints | marked, never endpoints |
| Systems | Convex's public functions beside `internal` ones [CVX-INTERNAL] | — | Next.js server functions, which are public endpoints [NEXT-SEC] |

**Recommendation: A.** A missing marker fails closed, and the error says what
to add. The review's attack 1 on C3 rules out C.

### 7.3 O3. How screens are addressed

**A (recommended): the state value is the address.** `kit.link(TaskView{id:
task.id}, ...)`, as in 3.3.

**B: path strings, checked at compile time when they are literals.**

```jet
// Option B, not checked (#6552 c8).
row: fn(task: logic.Task) -> ui.Node {
    kit.link("/tasks/{task.id.raw()}", ui.text(ui.verbatim(task.title)))
}
```

**C: file routes with generated link functions** (D-ROUTE-FILES2: a file
`Routes/Tasks/[ID].jet` answers `/tasks/:id`).

```jet
// Option C, not checked (#6552 c8). `routes` is generated from Routes/.
row: fn(task: logic.Task) -> ui.Node {
    kit.link(routes.tasks.id(task.id), ui.text(ui.verbatim(task.title)))
}
```

| | A, typed value | B, checked string | C, generated functions |
|---|---|---|---|
| A typo or a wrong type | type error | compile error for a literal; a runtime check otherwise | type error |
| Renaming a route | change one line in the graph | find and replace strings | rename the file |
| Reads like the web | less | exactly | less |
| Deep links, Back, push | from the value | parsed back to the value | from the value |
| One route set on every target | checked (#6445 c6) | checked for literals | by the file tree |

**Recommendation: A.** File routes (C) stay available for content sites and
lower to the same rows.

### 7.4 O4. Data on the device

**A (recommended): the server is the authority.** The device keeps the last
live results and a queue of writes (`.offline(.Queue)`, 4.8). Data that must
merge offline opts into `app.collection` (D-SYNC2).

**B: local-first everywhere.** All app data is a synced collection on the
device, and the server is a sync peer.

```jet
// Option B, not checked (#3709).
mine: fn(view: MineView) -> ui.Node {
    tasks: app.collection<logic.Task>("tasks")
    kit.list(tasks.all(), row: mine_row)
}
```

**C: per target.** The web is online only; native uses collections.

| | A | B | C |
|---|---|---|---|
| Permission checks | each write, on the server | each pushed change, by the same methods (4.8) | two paths |
| Conflicts | versions, shown to the person | merged; may surprise | per target |
| Large data | the server pages it | the device holds it all | — |
| Offline | reads what you saw; writes queue | everything | native only |
| Systems | LiveView (online), Firestore [FS-OFF] | Replicache, Zero, ElectricSQL, PowerSync | — |

**Recommendation: A.** It keeps the web's permission model, and D-SYNC2 is
the opt-in for data that must merge.

### 7.5 O5. Forms

**A (recommended): `kit.form` with typed fields.** The form value is the
type; `f.title` is the field (#6821). This is the form in 3.3.

**B: bound fields only.** Each field binds to a local value by hand.

```jet
// Option B, not checked (#6447).
editor: fn(crew: app.Remote<logic.Crew>, page: logic.TaskPage) -> ui.Node {
    title := ui.local(page.task.title)
    ui.column([
        kit.text_field(title, label: "Title"),
        kit.button("Save", on_click: fn() { _: crew.update_task(page.task.id, page.task.version, logic.TaskDraft{title: title.get()}) }),
    ])
}
```

| | A | B |
|---|---|---|
| Validation as you type | from the type's `validate` block | by hand |
| Server errors on fields | by path, automatic | by hand |
| No-script posting on the web | automatic | needs a form anyway |
| A field the type does not have | compile error | compile error |

**Recommendation: A.**

### 7.6 O6. Realtime beyond stored data

**A (recommended): presence through a service method.** The server reads
`app.presence(state)` inside a signed-in method that first reads the row the
state shows, as `Crew.viewers` does (4.9). Messages that are not stored, such as typing indicators, are a later card if
an app needs them.

**B: typed channels.**

```jet
// Option B, not checked (#3709).
typing: fn(view: logic.TaskView) -> ui.Node {
    channel: app.channel<Typing>("task-typing")
    if app.state(channel.latest()) == {
        .Loaded(who) -> ui.text("{who.name} is typing")
        .Idle | .Loading | .Failed(_) -> ui.empty()
    }
}
```

| | A | B |
|---|---|---|
| Mechanisms | live queries plus presence | live queries plus channels |
| Permissions | the row policy of the row the method reads | per channel name, by hand |
| High-rate messages (cursors) | not covered | covered |
| Systems | Phoenix Presence [PHX-PRES] | Laravel broadcasting [LAR-BC] |

**Recommendation: A.** Add B when a measured app needs it.

### 7.7 O7. Sessions across targets

**A (recommended): one session model.** The web uses a cookie; native uses
access and refresh tokens in the Keychain or Keystore (#6818). The app code
is the same: `app.caller()`.

**B: per target, configured by the app.**

```jet
// Option B, not checked (#6818).
run: fn() {
    screens.crewboard()
        .sessions(web: auth.cookie(same_site: .Lax), native: auth.tokens(access: 15min, refresh: 720h))
        .serve()
}
```

**Recommendation: A, with B's lifetimes as optional arguments.**

### 7.8 O8. Deploy shape

**A (recommended): a self-contained server binary and an OCI image.** Any
Linux host or container platform.

**B: host adapters** (one per hosting company).

**C: no server** (D-PORTABLE-WEB1), only for apps with no services.

| | A | B | C |
|---|---|---|---|
| Lock-in | none | per adapter | none |
| Long-lived sockets for live queries | yes | depends on the host | — |
| Work for Jet | none extra | one adapter per host | the player |

**Recommendation: A now.** B only when a host is asked for, and only for hosts
with WebSockets.

### 7.9 O9. Target-specific code in shared screens

**A (recommended, ratified by D-PLATFORM-OVERRIDE1): `comptime if` on the
target, with `native.view` for system views.**

**B: per-target packages only;** shared screens leave a slot the entry fills.

**C: target markers on declarations.** `#Target` names web partitions only at
dev, so C needs new syntax and a ballot of its own.

**Recommendation: A**, as ratified.

### 7.10 O10. How platform features attach

**A (recommended): plugins.** `native.menu_bar(menus)` returns an
`app.Plugin`, and the entry adds it with `.with(...)`. `app` knows nothing of
menus; the `native` package grows without changing Core. This is the form in
3.5 and 3.6.

**B: builder methods on the app.**

```jet
// Option B, not checked (#6799).
run: fn() {
    screens.crewboard()
        .menu_bar(menus)
        .tray(native.Symbol.Crewboard, tray_menu)
        .serve()
}
```

| | A, plugins | B, builder methods |
|---|---|---|
| Core's `app` grows with each feature | no | yes |
| A third-party plugin | the same shape as first-party ones | not possible |
| Discoverability | `native.` completion | `.` completion on the app |
| Systems | Tauri 2 plugins [T2]; Capacitor [CAP] | Electron's `app` and `BrowserWindow` APIs |

**Recommendation: A.**

### 7.11 S1. The style model

**A (recommended): typed styles from tokens** (6.1). **B: modifier chains**
(6.2). **C: scoped, checked CSS sheets** (6.2).

| | A | B | C |
|---|---|---|---|
| Errors caught by the seed today (6.5) | 6 of 8 | the same type checks, per modifier | none until #6817 parses CSS |
| Reuse | named styles | functions that apply modifiers | classes |
| Order matters | no: one value | yes: each modifier wraps the last | the cascade inside a sheet |
| Familiar to | Flutter, vanilla-extract users | SwiftUI and Compose users | web developers |
| Friction | more lines (6.2) | wrapping order | doubled braces |

**Recommendation: A**, with C allowed for teams that bring CSS.

### 7.12 Amendments and the ballot

Revision 3 amends one ratified decision:

- **D-DX-SERVERFN1.** Registration moves from `.action`, `.form` and `.data`
  to a mounted service whose marked methods are endpoints. The derived typed
  client, no-script POST, same-origin CSRF, the retry and idempotency policy
  and the wire checks all stay.

Revision 2 also amended D-LIVEQUERY1, to `app.live(fn() { service.method(args)
})`, because Jet has no bound method values. Revision 3 withdrew that
amendment and proposed bound method values (D-BOUND-METHOD1, #6824).
Revision 4 drops that ballot from this proposal: the shape does not need
them. D-LIVEQUERY1 stands with the closure as its form for a query and its
arguments: the closure must be one call of an `app.Remote` endpoint with
codable arguments, and the subscription is keyed by that call (4.6). So
the closure is the target form, and nothing in this proposal waits on
#6824.

| Item | Options | Recommended | Amends |
|---|---|---|---|
| The shape as a whole (sections 1, 3–6) | adopt as the target shape · revise | adopt | D-DX-SERVERFN1 |
| O2 | A · B · C | A | no |
| O3 | A · B · C | A | no |
| O4 | A · B · C | A | no |
| O5 | A · B | A | no |
| O6 | A · B | A | no |
| O7 | A · B | A | no |
| O8 | A · B · C | A | no |
| O9 | A · B · C | A (ratified) | no |
| O10 | A · B | A | no |
| S1 | A · B · C | A | no |
| Native outputs carry `server:` as a build fact | yes · a runtime setting | yes | no |

### 7.13 Interim forms and target forms

The checked files are written against seed-b15. Where the seed lacks a
language feature, the file uses an interim form. The ballot approves the
target form; the public API is not frozen around the interim one. Each row
names the criterion that lets the file switch.

| Where | Interim form (checked today) | Target form | Switches when |
|---|---|---|---|
| Live queries, every screen | `app.live(fn() { crew.board(view.project) })` | the same; the closure is the target form, keyed by its one call (4.6) | — |
| Jobs | `app.enqueue(fn() { self.tell_assignee(task.id) })` | the same; the body is one job call with codable arguments (4.14) | — |
| Form fields | `f.text(f.title, ...)` in the file; the checked copy uses `f.text(kit.field("title"), ...)` (TR5) | `f.title`, a field derived per struct field | #6821 |
| Handler errors | the stubs type a handler `fn() app.Notice!`; the checked copy widens it to `fn() E!` (TR6), because the seed does not find `impl TaskError -> app.Notice` from another package | `fn() app.Notice!` as written | #6819 c7 |
| Drop zones | `ui.drop_zone(body, label:, on_drop: fn(task: logic.Task) {...})`, refused today (A.4) | the same, with `V` taken from the closure | #6819 c4 |
| Styles | `ui.style(card_style)` with a named function | the same, or an inline closure `ui.style(fn(t: CrewTheme) {...})` | #6819 c4 |
| Handlers that end in a call returning nothing | the handler type is `fn() E!` | `fn() -> R E!` where needed | #6819 c5 |
| Remote clients | `app.Remote<logic.Crew>`; the checked copy uses a generated `CrewRemote` with only the marked methods (TR2) | `app.Remote<S>` derived by the compiler | #6445 c3 |
| Wire types in Logic | `#Codable` on types with `distinct` IDs; the checked copy drops the markers (TR4) | codable distinct types | #6819 c1 |
| Two-part keys | the named struct `MemberKey` | the unnamed record `{project:, account:}` | #6251 c12 |

## 8. How the shape compares (card c8)

Crewboard is a probe: every need it found that Jet does not meet is a gap
with a card (3.9, 5, 6). This section compares the shape, area by area, with
the strongest existing system for that area, stated at its best and sourced.

The verdict column uses four words:

- **behind:** the best system does more today, or does it with less code;
- **matches:** the shape does what the best system does, at about the same
  cost to the developer;
- **goes further:** the shape adds a check or a default that the best system
  leaves to the developer. Each such row names its claim (CL1–CL9 in 8.3),
  and none of them is called "best" until a review round tests it;
- **not covered:** the shape has no answer yet; the card is named.

Round 2 tested every claim. Revision 3 takes its verdicts: six claims now
read "matches", CL6 is narrowed, and CL5 waits on #6557.

Every Jet row is a proposal. Nothing in the Jet column runs today beyond the
seed checks in Appendix A.

### 8.1 Area by area

| Area | Strongest existing, at its best | Jet section | Verdict |
|---|---|---|---|
| Server calls | Next.js server functions [NEXT-SEC]; tRPC end-to-end types [TRPC]; Leptos server functions [LEP-SF] | 4.1 | matches on calls |
| Endpoint exposure | Convex internal functions [CVX-INTERNAL] | 4.1 | matches Convex (CL1) |
| Calls in flight: pending, cancel, timeout, error destination | React Router fetchers and pending UI [RR-ACT] | 4.1, 4.10 | matches |
| Data and schema | Ecto changesets and schemas [ECTO]; Active Record [RAILS-AR]; Convex schemas [CVX-SCHEMA] | 4.2 | behind: typed tables are #6822 |
| Authorization policy | Supabase and Postgres row-level security [SUPA-RLS]; Laravel policies [LAR-AUTH] | 4.2 | behind until #3709 c22; then matches row-level security |
| Transactions and concurrency | EF Core concurrency tokens [EF-CONC]; Ecto transactions [ECTO] | 4.2 | behind until #6445 c5; then matches |
| Queries: relations, counts, search | Ecto preloads [ECTO-PRELOAD]; Postgres full-text search [PG-FTS]; Laravel Scout [SCOUT] | 4.2 | behind (#6827) |
| Live data | Convex reactive queries [CVX-RT]; Phoenix LiveView [PHX-LV]; Meteor [METEOR] | 4.6 | matches Convex on the design; nothing runs yet (#3709) |
| Validation | ASP.NET Core `[ApiController]` [ASPNET-API]; FastAPI [FASTAPI-BODY]; Phoenix changesets [PHX-FORM]; Laravel [LAR-VAL]; Blazor [BLZ-VAL] | 4.3 | matches ASP.NET Core and FastAPI (CL2); database-aware checks behind until #6831 |
| Routes and navigation | Navigation Compose typed routes [NAV-TYPESAFE]; Expo Router [EXPO-ROUTER]; TanStack Router [TSR]; Phoenix verified routes [PHX-VR] | 4.4 | matches Navigation Compose and Expo Router: one route set on every target (CL3) |
| Screens, state and identity | SwiftUI [SWIFTUI] [SWIFTUI-ID]; Compose [COMPOSE] [COMPOSE-KEY]; Flutter [FLUTTER]; The Elm Architecture [ELM-ARCH] | 4.5 | matches |
| Optimistic updates | Meteor methods [METEOR-METHODS]; Zero and Replicache mutators [ZERO] [REP] | 4.6 | matches Meteor and Zero (CL4) |
| Caching and revalidation | Next.js caching [NEXT-CACHE]; Convex [CVX-RT] | 4.6 | matches Convex: live queries replace manual revalidation |
| Accounts: sign-up, confirmation, reset, passkeys, two-factor | Laravel Fortify [FORTIFY]; Phoenix `phx.gen.auth` [PHX-AUTH]; ASP.NET Core Identity [ASPNET-ID]; WebAuthn [WEBAUTHN] | 4.7 | behind until #6826 |
| Sessions and CSRF | Rails security defaults [RAILS-SEC]; Phoenix tokens [PHX-TOKEN] | 4.7 | behind on native sessions (#6818); matches on CSRF |
| Rate limiting | Rails 8 `rate_limit` [RAILS-RL] | 4.7 | behind until #6829 |
| Secrets | Rust `secrecy` [SECRECY]; SvelteKit private env [SK-ENV]; React taint APIs [REACT-TAINT] | 4.7 | not covered today: the seed accepts a printed secret; CL5 is proposed in #6557 |
| Webhooks | Stripe signature checks [STRIPE-SIG]; GitHub's delivery guidance [GH-HOOK-BP] | 4.7 | goes further for methods marked as webhooks: the signature check and redelivery de-duplication cannot be left out (CL6) |
| Server jobs, outbox and schedules | Oban [OBAN]; Solid Queue [SOLIDQ]; Laravel queues [LAR-QUEUE]; Convex scheduling [CVX-SCHED] | 4.14 | behind until #6825; then matches Oban |
| Email | Laravel mail and Fortify [FORTIFY]; Phoenix `phx.gen.auth` mailers [PHX-AUTH] | 4.14 | behind until #6826 |
| Offline | Firestore offline cache [FS-OFF]; Replicache [REP]; Zero [ZERO]; ElectricSQL [ELEC]; PowerSync [PS] | 4.8 | behind the sync engines on merging; matches Firestore on queued writes, once #3709 lands |
| Realtime and presence | Phoenix Presence [PHX-PRES]; Laravel broadcasting [LAR-BC]; Turbo Streams [TURBO-STREAMS] | 4.9 | matches Presence; caller-dependent queries do not share (#3709 c24); behind on free-form channels (O6) |
| Errors | Elm `Http.Error` [ELM-HTTP]; SvelteKit errors [SK-ERR]; Next.js error handling [NEXT-ERR] | 4.10 | matches Elm: network errors are typed cases (CL7) |
| Testing | Phoenix LiveViewTest [PHX-TEST]; Rails system tests [RAILS-TEST] | 4.11 | matches LiveViewTest, through one in-process host |
| Observability | OpenTelemetry traces and metrics [OTEL] | 4.11 | behind until #6830 |
| Deploy and versions | Phoenix releases [PHX-REL]; buf breaking checks [BUF]; Apollo schema checks [APOLLO-CHECKS]; Electron, Tauri and Expo updates [ELECTRON-UPD] [TAURI-UPD] [EXPO-UPD] | 4.12 | matches buf (CL8); behind Apollo, which checks against real client use |
| Performance | Qwik resumability [QWIK]; LiveView's small client [PHX-LV] | 4.13 | behind until #6556 measures it; no speed claim |
| Server rendering and streaming | React streaming [REACT-STREAM]; Leptos SSR modes [LEP-SSR] | 5.2 | matches on the design (#6812) |
| SEO and metadata | Next.js metadata [NEXT-META] | 5.2 | matches; signed-in screens are `noindex` by default |
| Images and assets | Vite asset handling [VITE] | 5.2 | matches |
| Uploads and downloads | Active Storage [RAILS-AS]; LiveView uploads [PHX-UP]; tus [TUS] | 5.2.3 | matches Active Storage on naming and type checks; behind on resumable uploads (#6813) |
| CSV export | OWASP CSV injection guidance [OWASP-CSV] | 4.15 | matches OWASP's rule, as the writer's default (#6828) |
| HTTP caching | MDN caching guidance [MDN-CACHE] | 5.2 | matches, with private defaults |
| Install as a web app | MDN PWAs [MDN-PWA]; SvelteKit service workers [SK-SW] | 5.2 | matches (#6815) |
| Windows and menus | Electron [ELECTRON-MENU] [ELECTRON-WIN]; SwiftUI scenes | 5.1 | matches (#6799, #2999) |
| Tray | Electron Tray [ELECTRON-TRAY]; SwiftUI `MenuBarExtra` [SWIFTUI-MBE]; Qt [QT-TRAY] | 5.1 | matches |
| Notifications and push | Expo push [EXPO-PUSH]; Tauri [TAURI-NOTIF] | 5.1 | matches; a push opens a route of the one route set |
| Background work | Expo BackgroundTask [EXPO-BG]; WorkManager [ANDROID-BG] | 5.1 | matches the design on phones and desktops (#6801); none on the web, by design |
| Files | File System Access API [MDN-FSA]; Tauri dialog [TAURI-DIALOG]; the HTML file input [MDN-FILE] | 5.1 | matches: one `PickedFile` type on every target |
| Camera, location, sensors | Capacitor [CAP-CAM]; Expo Location [EXPO-LOC] | 5.1 | behind: two of many sensors (#6803) |
| Deep and app links | Apple universal links [APPLE-AD]; Android App Links [ANDROID-AL]; Tauri [TAURI-DL]; Electron single instance [ELECTRON-SI] | 5.1 | matches |
| Clipboard, drag and drop, share | MDN Clipboard and DnD [MDN-CLIP] [MDN-DND]; Web Share [MDN-SHARE] | 5.1 | matches; drags need a keyboard alternative [WCAG-DRAG] |
| Permissions | Expo permissions and config plugins [EXPO-PERM] [EXPO-CP] | 5.1 | matches |
| Accessibility | Compose `contentDescription` [COMPOSE-IMAGE]; SwiftUI [SWIFTUI-A11Y]; React Native [RN-A11Y]; Flutter [FLUTTER-A11Y] | 5.1, 6.3 | matches Compose: icon labels and image alternatives are required (CL9) |
| Localization | Flutter [FLUTTER-I18N]; Android resources [ANDROID-L10N] | 5.1 | matches: literals are messages |
| Lifecycle | SwiftUI `scenePhase` [SWIFTUI-SP]; Compose saved state [COMPOSE-SAVE] | 5.1 | matches |
| Install, update, signing | Tauri updater [TAURI-UPD]; Electron [ELECTRON-SIGN]; Expo [EXPO-BUILD] | 3.1, 5.1 | matches the design (#6810) |
| Layout, overlays, scrolling | CSS grid and container queries [MDN-CQ]; the `dialog` element [MDN-DIALOG]; Compose window size classes [COMPOSE-WSC] | 6.3 | matches the common cases |
| Typography and fonts | SwiftUI Dynamic Type; CSS fonts | 6.3 | matches |
| Color, themes, dark mode, contrast | vanilla-extract theme contracts [VE-CONTRACT]; Tailwind `dark:` [TW-DARK]; CSS `prefers-contrast` [MDN-CONTRAST]; SwiftUI `colorScheme` [SWIFTUI-CS] | 6.1, 6.3 | matches vanilla-extract: a token with no dark value does not compile (CL9) |
| States and motion | CSS pseudo-classes and transitions; `prefers-reduced-motion` [MDN-RM] | 6.3 | matches; reduced motion by default |
| Tokens and reuse | vanilla-extract [VE]; StyleX [STYLEX]; Tailwind [TW] | 6.1 | matches vanilla-extract |
| Per-platform look | SwiftUI and Compose follow the platform; Qt Quick styles [QT-STYLE] | 6.3 | behind (D-THEME2 chose one Jet look) |
| One codebase for web and native | Flutter [FLUTTER-WEB]; React Native and Expo with server functions [EXPO-API]; Dioxus fullstack [DIOXUS]; Kotlin Multiplatform [KMP] | 1, 3 | matches Dioxus and Expo on reach; the web output is semantic HTML, not a canvas |

### 8.2 Each system at its best

| System | Best at | Where Jet's shape stands |
|---|---|---|
| Next.js and React Server Components | server rendering with streaming and the largest component ecosystem [REACT-RSC] [NEXT-SEC] | behind on ecosystem; Jet's endpoints exist only by a marker, as Convex's public functions do (CL1) |
| SvelteKit | small bundles, form actions that work without script [SK-RF] | matches on forms |
| Remix and React Router | web-standard forms and loaders, pending UI [RR-ACT] | matches |
| Rails and Hotwire | productivity, conventions and secure defaults [RAILS-SEC] [HOTWIRE] | behind on batteries and maturity |
| Phoenix LiveView | server-driven realtime UI with presence and tests [PHX-LV] [PHX-PRES] [PHX-TEST] | matches on realtime; adds native targets |
| Laravel | batteries: auth, queues, broadcasting, validation [LAR-AUTH] [LAR-QUEUE] [LAR-BC] [LAR-VAL] | behind on batteries until #6825 and #6826 |
| ASP.NET and Blazor | one language across server and browser, and Hybrid for desktop and mobile [BLZ-HYBRID]; validation on every action [ASPNET-API] | matches on reach and validation |
| Leptos | fine-grained reactivity with server functions in Rust [LEP-SF] | matches |
| Qwik | resumability: no hydration cost [QWIK] | matches the design: Jet pages resume too (D-RESUME1); behind until #6556 measures it |
| Convex | reactive queries and internal functions [CVX-RT] [CVX-INTERNAL] | matches the design; Convex runs today |
| Replicache and Zero | instant local writes with server authority [REP] [ZERO] | matches on prediction (CL4); behind on partial sync |
| Firestore | offline cache and security rules for direct client access [FS-OFF] [FS-RULES] | matches on offline writes; Jet's rules are row policies on the server |
| ElectricSQL and PowerSync | Postgres sync to devices [ELEC] [PS] | behind on local-first; D-SYNC2 is the opt-in |
| TanStack Router | fully typed routes and search params [TSR] | matches |
| Meteor | the first full-stack latency compensation [METEOR-METHODS] | matches (CL4) |
| SwiftUI | native look and platform integration on Apple systems [SWIFTUI] | behind on Apple platform depth |
| Jetpack Compose | the same for Android, with adaptive layouts and type-safe navigation [COMPOSE] [COMPOSE-WSC] [NAV-TYPESAFE] | behind on Android depth; matches on routes (CL3) |
| Flutter | one renderer, pixel-identical on every target [FLUTTER] | matches on reach; Jet's web output is HTML, Flutter's is a canvas |
| React Native and Expo | native views from React, with store builds, updates and one route set [RN] [EXPO-UPD] [EXPO-ROUTER] | behind on ecosystem; matches on routes (CL3) |
| Tauri | small desktop apps with plugins for system features [T2] [TAURI-UPD] | matches on the plugin shape (O10) |
| Electron | the most complete desktop API [ELECTRON-MENU] [ELECTRON-UPD] | behind on breadth |
| Qt | mature native widgets and Qt Quick on desktop and embedded [QT-STYLE] | behind on maturity |
| Elm | typed HTTP errors, no runtime exceptions, one architecture [ELM-HTTP] [ELM] [ELM-ARCH] | matches on typed errors (CL7) |
| Lustre | Elm's model for Gleam, with server components [LUSTRE-SC] | matches |
| Capacitor | web apps with native plugins [CAP] | matches on plugins |

### 8.3 Claims register

Round 2 tested each claim, and round 3 tested them again. CL2, CL4 and CL7
failed their next tests in round 3; revision 4 fixed the files and the
rules. Round 4 found CL2 and CL4 holding and CL7 holding except for
`native.background`, whose failed run revision 5 sends to the app's notice
list. The rows below say what now holds. Only CL6 reads "goes further", and
CL5 waits on #6557.

| ID | Claim | Strongest existing | Evidence today | Verdict | Next test |
|---|---|---|---|---|---|
| CL1 | No method is reachable from a client unless it is marked | Convex internal functions [CVX-INTERNAL] | `app.Remote<Crew>` offers only marked methods; the `cl1_server_only` probe fails with `jet.UnknownFunction` on a signing key and a job (4.1) | matches Convex | find a path from client code to an unmarked method once #6445 generates the remote (c3) |
| CL2 | Every decoder runs the type's `validate` block | ASP.NET Core `[ApiController]` [ASPNET-API]; FastAPI [FASTAPI-BODY] | D-VALIDATE-DECODE1=B; every endpoint takes its form's type (`TaskDraft`, `ProjectDraft`, `InviteDraft`, `TaskBatch`), all in `Logic`, so no rule is written twice and no endpoint takes loose fields (4.3); database-aware checks through `app.field_error` (#6831). Round 3's test (`invite` with any string as an email) now fails at the decoder | matches | find a decode path that skips validation |
| CL3 | One route set serves web addresses, deep links, windows and push on every target | Navigation Compose [NAV-TYPESAFE]; Expo Router [EXPO-ROUTER] | `.screen` only in the graph builder (#6445 c6); entries add views, not routes (4.4) | matches | find a state a target can link to or push that has no route on another target |
| CL4 | Prediction runs the server method's body, minus what needs the server | Meteor methods [METEOR-METHODS]; Zero [ZERO] | D-OPTIMISTIC1; the subset rule in 4.6 now names everything the client skips: policies, version checks, database rules, `app.enqueue` and so every job, push and mail, and the server's clock; and it covers creates with a client-made ID, which `new_task` predicts (#6555) | matches | find an eligible method whose prediction differs from the server for a reason outside the subset rule |
| CL5 | A secret value cannot reach any client path, reply or log | Rust `secrecy` [SECRECY]; SvelteKit private env [SK-ENV] | none: seed-b15 accepts the `cl5_secret_print` probe; the rule is proposed in #6557 (D-SECRETEDGE1) | not carried until #6557 is built; then it would go further | re-run the probe on a build with #6557 |
| CL6 | A method marked `#App.Webhook` cannot skip the signature check or run a redelivery twice | Stripe signature verification [STRIPE-SIG]; GitHub's delivery guidance [GH-HOOK-BP] | the shape (4.7, #6820 c2, c3); a signing key of `None` is 401, never "no check"; the key is 256 random bits an owner makes (#6820) | goes further than frameworks where both checks are a recipe; narrowed to marked methods: a third-party POST to an `#App.Public` method has no check | mount a marked webhook that decodes before checking, runs a delivery ID twice, or runs with no key |
| CL7 | Network failures are typed cases the code must handle | Elm `Http.Error` [ELM-HTTP] | `app.CallError` cases in `TaskError`; every handler is `fn() app.Notice!` (4.1, 4.10); every live read in Crewboard shows its failure (the invitations list, the "also viewing" line); handlers outside a screen (tray and menu builders, `on_resume`, `on_drop`) have a stated destination, and a failed background run and a refused queued write join the app's notice list, which persists until dismissed (4.10, #6445 c7) | matches Elm | find a failure that reaches the person as a crash or as nothing |
| CL8 | Removing a wire field follows a compiler-checked policy | buf [BUF]; Apollo schema checks [APOLLO-CHECKS] | D-WIRESKEW1 and 4.12 (#6558) | matches buf; behind Apollo's usage data | remove a field without the steps and ship |
| CL9 | Style tokens need dark values; icon buttons need labels; images need text alternatives | Compose `contentDescription` [COMPOSE-IMAGE]; vanilla-extract `createThemeContract` [VE-CONTRACT] | the seeded bugs b4 and b5 in 6.5, and the `ui.Paint` type | matches | add a token, a button or an image that skips them |

## 9. Costs and what is not built

| Cost | Size | Card |
|---|---|---|
| Client runtime | a budget, not a guess: the web client must stay under the limit #6556 sets, measured on Crewboard | #6556 |
| Live queries | one subscription per screen; a result is shared only by callers it does not depend on, so Crewboard's policy-guarded queries run once per subscriber per write that meets them (4.9, #3709 c24); at most 50 ms between a commit and the push, with changes inside that window coalesced; a cap on live queries per session; a Postgres LISTEN/NOTIFY broker across server processes; table-level invalidation as the fallback when a footprint is too wide | #3709 |
| Prediction | the client carries the code of every eligible method; a method that is not eligible costs nothing | #3709 |
| Typed tables | a new Core surface, and the codec derive for ID types | #6822, #6819 |
| Row policies | a policy is asked on every read and write of its table. A policy whose reads are key lookups joins into the query, so a list read stays one statement; any other policy runs once per row, memoized per key in the transaction, and `jet inspect choices` shows which. An update pays two checks, on the stored row and the new one | #3709 c21, c22, #6827 |
| Transactions | every write method is a serializable transaction; a lost race retries the whole method | #6445 c5 |
| Jobs and mail | an outbox table written in each queuing transaction, a runner on each server, and one scheduler lease for `#App.Every` | #6825, #6826 |
| Accounts | confirmation, reset, passkeys, two-factor codes, a sign-in rate limit, invitations that lapse, and member removal, to build and keep secure | #6826 |
| Observability | a span per call, job, delivery and refresh, exported over OpenTelemetry | #6830 |
| Language gaps | closure-parameter inference, `R`-returns-nothing inference, the L0530 false positive and cross-package conversions; public API waits on them where 7.13 says so | #6819 c4–c7 |
| Drawn controls on desktop and phones | Jet draws `kit` views itself; native look comes from `native.theme()` and `native.view` | #6447, D-PLATFORM-OVERRIDE1 |
| Wire commitment | every endpoint type is snapshotted at release, and removal follows 4.12 | #6558 |
| Styling | a style compiler with two outputs (CSS classes, Jet layout) and the preview tests of 6.4 | #6817 |

**What each paused design must measure before its advantage is claimed:**

| Card | Design | Measure |
|---|---|---|
| #6553 | D-REACT-REF1: plain data updates the screen | update cost per changed field against signals, on the board with 500 cards |
| #6554 | D-INVALIDATE1: the compiler decides which reads a write makes stale | stale reads missed and reads refetched for nothing, against hand-written keys |
| #6555 | D-OPTIMISTIC1: prediction from the server function | the share of Crewboard's writes that are eligible, and wrong guesses per thousand |
| #6556 | D-RESUME1: resume from server state | client bytes and time to interactive, against Qwik and LiveView |
| #6557 | D-SECRETEDGE1: secrets never reach the client | the CL5 attack set, run as cases |
| #6558 | D-WIRESKEW1: old clients keep working | releases an old client survives, under the 4.12 policy |

**Not built, and not promised here:**

- mobile code generation, which D-MOBILE-UNDEFER1=B keeps deferred; the phone
  entry in 3.6 is the target shape once it is undeferred;
- resumable uploads, free-form realtime channels and local-first sync for all
  data (7.4, 7.6, 5.2);
- the cards paused by the owner stay paused; this proposal does not restart
  them.

## 10. The proposed surface

Everything below is new public API, so all of it is owner-gated. It is the
surface the stubs declare; the stubs are what the seed checked Crewboard and
the examples against. Bodies are `panic("stub")` or a trivial value: the
stubs fix names and types, not behavior.

| Package | Where | Card | Declares |
|---|---|---|---|
| `app` | Core (D-WEB-RUNTIME-API1=A) | #6445, #6825, #6813, #6820, #6831 | markers `#App.SignedIn`, `#App.Public`, `#App.Webhook(path:, signature:, key:)`, `#App.SigningKey`, `#App.UploadCheck(method:)`, `#App.Job(retries:)`, `#App.Every(cron:)`, `#App.Query`; `Signature`; `App` and its builder (`theme`, `api`, `sign_in`, `screen`, `head`, `cache`, `sitemap`, `stream`, `installable`, `error`, `not_found`, `mount`, `accounts`, `mail`, `get`, `offline`, `window`, `open_in_window`, `with`, `serve`), and `new`; `Remote<S>`, offering only the marked methods (the stub has none: transform TR2 generates `CrewRemote` in its place); `live` (one endpoint call), `state`, `Loadable`, `Live`, `CallError`, `Notice`, `Problem`; `caller`, `maybe_caller`, `field_error`, `new_id`, `random_key`, `presence`, `pending`; navigation (`go`, `replace`, `go_back_or`, `current`, `url_of`); `toast`; server work (`enqueue` (one job call), `notify`, `mail`, `Mail`, `smtp`); files (`Upload`, `Media`, `BlobID`, `Blob`, `BlobError`, `Blobs` with `keep`, `blobs`, `blob_url`); `Head`, `CachePolicy`, `Offline`, `Plugin`; tests (`test_host`, `TestHost` with `run_jobs` and `pushes_to`, `Screen`, `Reply`, `test_account`, `as_caller`, `endpoints`) |
| `ui` | first-party package (D-VIEW-SET1=C) | #6447, #6817 | layout (`column`, `row`, `grid`, `by_width`, `scroll`, `spacer`, `empty`, `Align`), `text`, `verbatim`, `date`, `today`, `DateStyle`, `image`, `asset`, `Asset`; style (`Style`, `style`, `Styled`, `Theme`, `theme`, `with_theme`, `Color`, `Paint`, `paint` with contrast values, `rgb`, `Space`, `px`, `Size`, `at_least`, `States`, `states`, `Motion`, `ease`, `Outline`, `outline`, `Shadow`, `TextStyle`, `Weight`, `Font`, `font`, `Preset`; S1 B `tokens` and the `Node` modifiers `padding`, `background`, `corner_radius`, `min_width`, `shadow`, `on_hover`, `focus_ring`, `foreground`, `text_style`; S1 C `sheet`, `Sheet`, `Class`, `class`); state (`local`, `saved`, `Local`); `draggable`, `drop_zone`, `label`, `announce` |
| `kit` | first-party package (D-VIEW-SET1=C) | #6447, #6821, #6817 | `button`, `icon_button`, `link`, `link_button`, `list` (with a required `key:`), `heading`, `notice`, `empty`, `badge`, `spinner`, `search_field`, `segmented`, `form`, `Form`, `Field`, `FormReply`, `file_button`, `download`, `progress`, `swipe_row`, `swipe_action`, `SwipeAction`, `Icon`; overlays (`dialog`, `sheet`, `popover`); `account_flows` |
| `native` | first-party package | #6799–#6810 | plugins (`menu_bar`, `tray`, `tabs`, `tab`, `app_links`, `url_scheme`, `push`, `background`, `lifecycle`); menus (`Menu`, `MenuItem`, `Symbol`, `menu`, `item`, `separator`, `quit_item`, `edit_menu`, `window_menu`, `context_menu`); device calls (`notify`, `open_file`, `save_file`, `drop_files`, `take_photo`, `location`, `ask`, `open_settings`, `copy_text`, `paste_text`, `share`); `PickedFile` (`read`, `text`, `upload`), `FileType`, `Place`, `Permission`, `Grant`; `theme` |
| `auth` | Core (D-AUTH1=A batteries) | #6818, #6826 | `AccountID`, `Credentials`, `SignInError`, `Accounts` (`verify_email`, `reset_password`, `passkeys`, `two_factor`, `limit_sign_in`), `sqlite`, `sign_in`, `describe`, `verified_email` (the address, lowercased) |
| `core.db` additions | Core | #6822, #3709, #6827 | `Table<K, T>` (`query`, `search`, `get`, `add`, `put`, `delete`, `guard`), `Rows<T>` (`filter`, `limit`, `collect`, `count`, `count_by`), `Counts`, `Store`, `open`; `Who`, `Op`; `DBError` with its `Denied` and `Exists` cases |

The full stubs follow.

<details>
<summary><code>app</code> stub</summary>

`stubs/appstub/app.jet`

```jet
// Stub surface for the proposed additions to Core's `app` module
// (D-WEB-RUNTIME-API1=A keeps `app` in Core). Signatures only; bodies stop.
use {
    auth
}

// Endpoint exposure (section 4.1). A method of a mounted service is an
// endpoint only under one of these markers.
SignedIn: struct {
    impl Marker {
        sites: [.Method]
    }
}

Public: struct {
    impl Marker {
        sites: [.Method]
    }
}

Signature: enum {
    GitHub,
    Stripe,
    HMACSHA256(header: String),
}

// `key` names the #App.SigningKey method that returns the key for a request;
// the compiler checks that it names one of the same service. A delivery for
// which that method returns None gets 401: a missing key never skips the
// check. `.GitHub` and `.Stripe` also drop redeliveries by their delivery ID.
Webhook: struct {
    path: String,
    signature: Signature,
    key: String,

    impl Marker {
        sites: [.Method]
    }
}

SigningKey: struct {
    impl Marker {
        sites: [.Method]
    }
}

// `method` names the method that decides, before the body of an upload is
// read, whether the caller may send it. It is a method of the same service
// that takes the endpoint's arguments other than the `Upload`, which travel
// in the URL; the compiler checks that it names one. The framework runs it
// as the caller, after the session check, in a read-only transaction under
// the row policies. An error ends the request with that error's status (403
// for a refusal), and the body is never read.
UploadCheck: struct {
    method: String,

    impl Marker {
        sites: [.Method]
    }
}

// Server jobs (section 4.14). A job is server-only. `enqueue` stores the call
// in the transaction of the method that makes it; the runner starts it after
// the commit and retries it with backoff. A run can repeat (a crash after the
// send but before the run is recorded), so delivery is at least once. Each
// queued call, and each scheduled run, has a key; `notify` and `mail` make
// each send's own key from it.
Job: struct {
    retries: Int{5},

    impl Marker {
        sites: [.Method]
    }
}

// A job the runner starts on a schedule, written as a cron line in UTC.
Every: struct {
    cron: String,

    impl Marker {
        sites: [.Method]
    }
}

// A screen-state field that lives after `?` in the address.
Query: struct {
    impl Marker {
        sites: [.Field]
    }
}

Loadable: enum<T, E> {
    Idle,
    Loading,
    Loaded(T),
    Failed(E),
}

Live: struct<T, E> {
    id: Int,
}

// How a remote call failed. Every client call can fail this way, so a
// service's error type declares `impl app.CallError -> E`.
#Error
CallError: enum {
    // No answer: offline past the queue, timed out, or the server is down.
    Unreachable(String),
    // The server's derived decoder ran `validate` and refused (HTTP 422).
    Invalid([FieldError]),
    // The session expired or was revoked (HTTP 401).
    SignedOut,
    // The server no longer accepts this client's wire version (#6558).
    UpdateRequired,
}

// Where a handler's failure goes (section 4.10). A button, menu item or swipe
// action takes `fn() app.Notice!`, so its error type must convert to a
// Notice. The frame shows it beside the control, announces it, and keeps it
// until it is dismissed.
#Error
Notice: struct {
    text: String,
    retry: Bool{false},
}

Problem: struct {
    id: String,
}

Offline: enum {
    Off,
    Queue,
}

// What a target-specific package adds to the graph: menus, tabs, push,
// background work. First-party packages build them; apps only pass them.
Plugin: struct {
    name: String,
}

// Page metadata for one screen on the web output.
Head: struct {
    title: String,
    description: String?{None},
    image: String?{None},
    canonical: Bool{true},
    index: Bool{true},
}

CachePolicy: enum {
    // Signed-in screens and endpoints: `Cache-Control: private, no-store`.
    Private,
    // Shared caches may keep it for `max_age`, then revalidate with the ETag.
    Public(max_age: Duration),
}

// A file a person sends. An endpoint that takes one must name its check
// (#App.UploadCheck). After the session check, the size cap (413) and the
// check (403 for a caller the policies refuse), the framework spools the body
// to a staging file, before the method's transaction starts; a serialization
// retry reads the same staged bytes. Only `Blobs.keep` reads it. A staged file
// lives no longer than its request: it is deleted when the request ends,
// unless `keep` moved it in a transaction that committed. One that a crash
// leaves behind is deleted when the server next starts.
Upload: struct {
    name: String,
    bytes: Int,
}

// Media types the server recognises from a file's first bytes.
Media: enum {
    Image,
    PDF,
    Text,
}

BlobID: distinct String

Blob: struct {
    id: BlobID,
    media: Media,
    bytes: Int,
}

#Error
BlobError: enum {
    // The bytes are not one of the allowed media types (HTTP 415).
    WrongType,
    TooLarge,
    Storage(String),
}

// File storage that names each file itself. A client's file name is never
// part of a path. A kept blob belongs to the transaction: a rollback deletes
// it, and a staged file nothing kept is deleted when the request ends.
Blobs: struct {
    name: String,

    keep: fn(self, file: Upload, allow: [Media]) -[FS.Write]> Blob BlobError! { panic("stub") }
}

blobs: fn(name: String) -> Blobs Never! { Blobs{name: name} }

// The address a client downloads a blob from. The download runs the read
// policy of the row that holds the BlobID.
blob_url: fn(id: BlobID) -> String Never! { panic("stub") }

// Server: an outgoing mail transport (section 4.14).
Mail: struct {
    url: String,
}

smtp: fn(url_env: String) -> Mail Never! { Mail{url: url_env} }

App: struct {
    title: String,

    theme: fn<T>(self, theme: T) -> App Never! { App{title: self.title} }
    api: fn<S>(self, path: String, max_upload: Int{1000000}) -> App Never! { App{title: self.title} }
    sign_in: fn<V>(self, view: V) -> App Never! { App{title: self.title} }
    screen: fn<F>(self, path: String, view: F, signed_in: Bool{false}) -> App Never! { App{title: self.title} }
    head: fn<F>(self, build: F) -> App Never! { App{title: self.title} }
    cache: fn<V>(self, policy: CachePolicy) -> App Never! { App{title: self.title} }
    sitemap: fn<F>(self, list: F) -> App Never! { App{title: self.title} }
    stream: fn(self, after: Duration) -> App Never! { App{title: self.title} }
    installable: fn(self, name: String, icon: String, color: Int) -> App Never! { App{title: self.title} }
    error: fn<F>(self, view: F) -> App Never! { App{title: self.title} }
    not_found: fn<F>(self, view: F) -> App Never! { App{title: self.title} }
    mount: fn<S>(self, service: S) -> App Never! { App{title: self.title} }
    accounts: fn<A>(self, store: A) -> App Never! { App{title: self.title} }
    mail: fn(self, transport: Mail, from: String) -> App Never! { App{title: self.title} }
    get: fn<R>(self, path: String, handler: fn() -> R) -> App Never! { App{title: self.title} }
    offline: fn(self, mode: Offline) -> App Never! { App{title: self.title} }
    window: fn(self, width: Int, height: Int) -> App Never! { App{title: self.title} }
    open_in_window: fn<V>(self) -> App Never! { App{title: self.title} }
    with: fn(self, plugin: Plugin) -> App Never! { App{title: self.title} }
    serve: fn(self) Never! { panic("stub") }
}

new: fn(title: String) -> App Never! { App{title: title} }

// A live query. The closure's body must be one call of an endpoint of an
// app.Remote<S> with #Codable arguments, or a compile error; the subscription,
// the device cache and #3709's sharing are keyed by that method and those
// arguments.
live: fn<T, E>(query: fn() -> T E!) -> Live<T, E> Never! { panic("stub") }

state: fn<T, E>(binding: Live<T, E>) -> Loadable<T, E> Never! { panic("stub") }

// The signed-in caller. Sema proves that every path to this call starts at
// an #App.SignedIn endpoint or a test's `as_caller`; any other path is a
// compile error that names it.
caller: fn() -> auth.AccountID Never! { panic("stub") }

// For #App.Public methods: the caller if there is a session.
maybe_caller: fn() -> auth.AccountID? Never! { panic("stub") }

// A refusal that names a field of a decoded argument, from a rule that needs
// the database. The form shows it beside the field, as it shows a `validate`
// failure.
field_error: fn(at: String, reason: String) -> FieldError Never! { FieldError{path: at, reason: reason} }

// A fresh UUIDv7, made where it is called: client IDs for offline creates.
new_id: fn() -> String Never! { panic("stub") }

// Server-only: 256 bits from the operating system's random source, as 64 hex
// digits. A key for signatures, never an ID.
random_key: fn() -[Rand]> String Never! { panic("stub") }

// Server-only: the accounts that have the screen state `key` open now and
// whose last read of that screen's data passed its row policies. An account
// whose read was refused or failed is not counted. A service method returns
// it, so the method's own access rules decide who may see it.
presence: fn<V>(key: V) -> [auth.AccountID] Never! { panic("stub") }

// Writes the offline queue still holds.
pending: fn() -> Int Never! { panic("stub") }

go: fn<V>(view: V) Never! { panic("stub") }

replace: fn<V>(view: V) Never! { panic("stub") }

go_back_or: fn<V>(view: V) Never! { panic("stub") }

current: fn<V>() -> V? Never! { panic("stub") }

url_of: fn<V>(view: V) -> String Never! { panic("stub") }

// A short message at the edge of the window; it never blocks input.
toast: fn(message: String) Never! { panic("stub") }

// Server-only: stores one job call in the current transaction (an outbox).
// The body must be exactly one call of an #App.Job method of the same
// service, with #Codable arguments; anything else is a compile error. The
// arguments are evaluated and encoded here, so the outbox row holds the job's
// name and their bytes, never a closure.
enqueue: fn<E>(job: fn() E!) Never! { panic("stub") }

// Server-only: sends a push notification to every device the account
// registered. `open` is the screen state a tap opens; it is a route of the
// one route table, so every target has it. Inside a job or a scheduled run,
// each push has its own key, made from the run's key, `to` and `open`. It
// goes to APNs as `apns-collapse-id` and to FCM as the collapse key, so a
// repeated run replaces each push it sent before, and two different pushes
// from one run never replace each other.
notify: fn<V>(to: auth.AccountID, title: String, body: String, open: V) -[Net]> Never! { panic("stub") }

// Server-only: sends a mail through the transport the server entry names.
// Inside a job or a scheduled run, the mail's key, made from the run's key,
// `to` and `link`, becomes the Message-ID, so a receiver that drops a
// repeated Message-ID shows a repeated run's mail once.
mail: fn<V>(to: String, subject: String, body: String, link: V) -[Net]> Never! { panic("stub") }

test_account: fn(email: String) -> auth.AccountID Never! { panic("stub") }

as_caller: fn(account: auth.AccountID, body: fn()) Never! { panic("stub") }

endpoints: fn<S>() -> [String] Never! { panic("stub") }

Reply: struct {
    status: Int,
}

Screen: struct {
    id: Int,

    press: fn(self, label: String) Never! { panic("stub") }
    type: fn(self, label: String, text: String) Never! { panic("stub") }
    shows: fn(self, text: String) -> Bool Never! { panic("stub") }
    within: fn(self, label: String) -> Screen Never! { panic("stub") }
    is: fn<V>(self, view: V) -> Bool Never! { panic("stub") }
}

TestHost: struct {
    id: Int,

    sign_up: fn(self, email: String) -> auth.AccountID Never! { panic("stub") }
    open: fn<V>(self, view: V) -> Screen Never! { panic("stub") }
    calls_sent: fn(self) -> Int Never! { panic("stub") }
    raw_call: fn(self, method: String, body: String) -> Reply Never! { panic("stub") }
    // Runs the jobs the outbox holds, as the runner would after a commit.
    run_jobs: fn(self) -> Int Never! { panic("stub") }
    pushes_to: fn(self, account: auth.AccountID) -> Int Never! { panic("stub") }
}

test_host: fn(graph: App) -> TestHost Never! { panic("stub") }
```

</details>

<details>
<summary><code>ui</code> stub</summary>

`stubs/ui/ui.jet`

```jet
// Stub surface for the proposed `ui` package (#6447 views, #6817 styling,
// D-I18N1 text, D-LAYOUT2 layout, D-THEME2 themes). Signatures only.
// T5: D-I18N1=A makes view text `ui.Text`, built from literals by context.
// The seed has no contextual construction, so the stub reads Text as String.
use {
    app: appstub
    time: core.time
}

Node: struct {
    id: Int,

    // Option S1-B (section 6.2): modifiers return a new node.
    padding: fn(self, value: Space) -> Node Never! { Node{id: self.id} }
    background: fn(self, value: Paint) -> Node Never! { Node{id: self.id} }
    corner_radius: fn(self, value: Space) -> Node Never! { Node{id: self.id} }
    min_width: fn(self, value: Space) -> Node Never! { Node{id: self.id} }
    shadow: fn(self, value: Shadow) -> Node Never! { Node{id: self.id} }
    on_hover: fn(self, value: Shadow) -> Node Never! { Node{id: self.id} }
    focus_ring: fn(self, value: Paint) -> Node Never! { Node{id: self.id} }
    foreground: fn(self, value: Paint) -> Node Never! { Node{id: self.id} }
    text_style: fn(self, value: TextStyle) -> Node Never! { Node{id: self.id} }
    // Option S1-C (section 6.2): a scoped class from a checked stylesheet.
    class: fn(self, value: Class) -> Node Never! { Node{id: self.id} }
}

Color: struct {
    rgb: Int,
}

// Light and dark values are required. The increased-contrast values are
// optional and default to the plain ones. Under forced colors (Windows high
// contrast, CSS forced-colors) the frame maps each paint to the system color
// of its role, and focus rings and borders stay visible.
Paint: struct {
    light: Color,
    dark: Color,
    contrast_light: Color?{None},
    contrast_dark: Color?{None},
}

Space: struct {
    points: Int,
}

Size: struct {
    min: Space?{None},
    max: Space?{None},
}

Shadow: enum {
    Flat,
    Low,
    Medium,
    High,
}

TextStyle: enum {
    Body,
    Caption,
    Title,
    Heading,
}

Weight: enum {
    Regular,
    Medium,
    Bold,
}

Align: enum {
    Start,
    Center,
    End,
    Stretch,
}

Outline: struct {
    paint: Paint,
    width: Space,
}

Motion: struct {
    duration: Duration,
}

States: struct {
    hover: Style?{None},
    pressed: Style?{None},
    focused: Style?{None},
    disabled: Style?{None},
}

Style: struct {
    background: Paint?{None},
    text_color: Paint?{None},
    text: TextStyle?{None},
    weight: Weight?{None},
    padding: Space?{None},
    gap: Space?{None},
    radius: Space?{None},
    width: Size?{None},
    align: Align?{None},
    shadow: Shadow?{None},
    outline: Outline?{None},
    opacity: Int?{None},
    motion: Motion?{None},
    states: States?{None},
}

Font: struct {
    family: String,
}

Asset: struct {
    path: String,
}

Preset: struct {
    name: String,
}

Theme: struct<T> {
    tokens: T,
}

Local: struct<T> {
    value: T,

    get: fn(self) -> T Never! { self.value }
    set: fn(self, value: T) Never! { panic("stub") }
}

DateStyle: enum {
    Short,
    Long,
}

Styled: struct {
    id: Int,
}

Class: struct {
    name: String,
}

Sheet: struct {
    id: Int,

    class: fn(self, name: String) -> Class Never! { Class{name: name} }
}

// Themes. accent and radius are D-THEME2's typed overrides; `tokens` is the
// app's own token struct; `preset` swaps in a system-like look.
theme: fn<T>(tokens: T, accent: Paint?{None}, radius: Space?{None}, font: Font?{None}, preset: Preset?{None}) -> Theme<T> Never! { Theme<T>{tokens: tokens} }
rgb: fn(value: Int) -> Color Never! { Color{rgb: value} }
paint: fn(light: Color, dark: Color, contrast_light: Color?{None}, contrast_dark: Color?{None}) -> Paint Never! { Paint{light: light, dark: dark, contrast_light: contrast_light, contrast_dark: contrast_dark} }
px: fn(value: Int) -> Space Never! { Space{points: value} }
at_least: fn(value: Space) -> Size Never! { Size{min: value} }
outline: fn(paint: Paint, width: Space) -> Outline Never! { Outline{paint: paint, width: width} }
ease: fn(duration: Duration) -> Motion Never! { Motion{duration: duration} }
states: fn(hover: Style?{None}, pressed: Style?{None}, focused: Style?{None}, disabled: Style?{None}) -> States Never! { States{hover: hover, pressed: pressed, focused: focused, disabled: disabled} }
asset: fn(path: String) -> Asset Never! { Asset{path: path} }
font: fn(family: String, files: [Asset]) -> Font Never! { Font{family: family} }

// A style is a function from the theme tokens to a Style. The graph knows
// its theme type T, so a style or a with_theme subtree that asks for other
// tokens is a compile error (#6817).
style: fn<T>(build: fn(T) -> Style) -> Styled Never! { panic("stub") }
// A subtree drawn with other tokens. Everything it does not set is inherited.
with_theme: fn<T>(theme: Theme<T>, body: Node) -> Node Never! { panic("stub") }
// S1-B: the current theme's tokens.
tokens: fn<T>() -> T Never! { panic("stub") }
// S1-C: one stylesheet per file; class names are checked against it.
sheet: fn(css: String) -> Sheet Never! { panic("stub") }

// Text (D-I18N1=A).
text: fn(text: String, one: String?{None}, plural: Int?{None}, style: Styled?{None}) -> Node Never! { panic("stub") }
verbatim: fn(value: String) -> String Never! { value.copy() }
date: fn(day: time.LocalDate, style: DateStyle) -> String Never! { panic("stub") }
today: fn() -> time.LocalDate Never! { panic("stub") }

// Layout (D-LAYOUT2=A).
column: fn(children: [Node], gap: Space?{None}, align: Align?{None}, style: Styled?{None}) -> Node Never! { panic("stub") }
row: fn(children: [Node], gap: Space?{None}, align: Align?{None}, style: Styled?{None}) -> Node Never! { panic("stub") }
grid: fn(children: [Node], columns: Int, gap: Space?{None}, style: Styled?{None}) -> Node Never! { panic("stub") }
spacer: fn() -> Node Never! { panic("stub") }
empty: fn() -> Node Never! { panic("stub") }
by_width: fn(compact: Node, regular: Node, wide: Node?{None}) -> Node Never! { panic("stub") }
// A scroll container. A sticky child stays at the edge while the rest scrolls.
scroll: fn(body: Node, sticky: Node?{None}) -> Node Never! { panic("stub") }
image: fn(source: Asset, alt: String) -> Node Never! { panic("stub") }

// Screen-local state. `saved` also survives the app being stopped in the
// background and restored (section 5, lifecycle).
local: fn<T>(initial: T) -> Local<T> Never! { Local<T>{value: initial} }
saved: fn<T>(initial: T) -> Local<T> Never! { Local<T>{value: initial} }

// Drag and drop inside the app. Every drag has a keyboard and single-pointer
// form (WCAG 2.5.7): Space picks the item up, the arrow keys step through
// the drop zones by their labels, Enter drops; a long press offers the same
// list as a menu. So a drop zone must have a label.
draggable: fn<V>(body: Node, value: V, label: String) -> Node Never! { panic("stub") }
drop_zone: fn<V>(body: Node, label: String, on_drop: fn(V) app.Notice!) -> Node Never! { panic("stub") }

// Accessibility.
label: fn(body: Node, text: String) -> Node Never! { panic("stub") }
announce: fn(text: String) Never! { panic("stub") }
```

</details>

<details>
<summary><code>kit</code> stub</summary>

`stubs/kit/kit.jet`

```jet
// Stub surface for the proposed `kit` views (D-VIEW-SET1, #6821). Signatures only.
use {
    app: appstub
    time: core.time
    ui
}

FormReply: enum {
    Fields([FieldError]),
    Message(String),
}

Field: struct<T> {
    name: String,
}

// #6821: the proposal derives one typed Field per member of T, so
// `f.title` is a Field<String>. The stub cannot, which is residual R1.
Form: struct<T> {
    id: Int,

    text: fn(self, field: Field<String>, label: String) -> ui.Node Never! { panic("stub") }
    text_area: fn(self, field: Field<String>, label: String) -> ui.Node Never! { panic("stub") }
    email: fn(self, field: Field<String>, label: String) -> ui.Node Never! { panic("stub") }
    password: fn(self, field: Field<#Secret String>, label: String, reveal: Bool{false}) -> ui.Node Never! { panic("stub") }
    date: fn(self, field: Field<time.LocalDate?>, label: String) -> ui.Node Never! { panic("stub") }
    pick: fn<V, O>(self, field: Field<V>, label: String, options: [O], show: fn(O) -> String, value: fn(O) -> V) -> ui.Node Never! { panic("stub") }
}

SwipeAction: struct {
    label: String,
}

Icon: enum {
    Move,
    Share,
    Trash,
}

spinner: fn() -> ui.Node Never! { panic("stub") }
heading: fn(text: String, level: Int{1}) -> ui.Node Never! { panic("stub") }
empty: fn(text: String) -> ui.Node Never! { panic("stub") }
notice: fn(text: String) -> ui.Node Never! { panic("stub") }
badge: fn(body: ui.Node) -> ui.Node Never! { panic("stub") }
link: fn<V>(to: V, body: ui.Node) -> ui.Node Never! { panic("stub") }
// `key` is each row's identity: ui.local state, focus and motion follow the
// key when rows move, appear or are filtered out.
list: fn<T, K>(items: [T], key: fn(T) -> K, row: fn(T) -> ui.Node) -> ui.Node Never! { panic("stub") }
// A handler runs as a task: the button shows it is busy, refuses a second
// press, and the task is cancelled if the screen closes. Its error becomes
// an app.Notice that the frame shows beside the button.
button: fn(label: String, on_click: fn() app.Notice!) -> ui.Node Never! { panic("stub") }
// An icon-only button must say what it does: `label` has no default.
icon_button: fn(icon: Icon, label: String, on_click: fn() app.Notice!) -> ui.Node Never! { panic("stub") }
// A link that saves a stored file. The server sends it as an attachment with
// nosniff, after the read policy of the row that holds its BlobID.
download: fn(url: String, label: String) -> ui.Node Never! { panic("stub") }
link_button: fn<V>(label: String, to: V) -> ui.Node Never! { panic("stub") }
search_field: fn(value: String, label: String, debounce: Duration?{None}, on_change: fn(String)) -> ui.Node Never! { panic("stub") }
segmented: fn<T>(value: ui.Local<T>, options: [T], label: fn(T) -> String) -> ui.Node Never! { panic("stub") }
form: fn<T, E>(initial: T, submit: String, on_submit: fn(T) E!, on_error: fn(E) -> FormReply, fields: fn(Form<T>) -> ui.Node) -> ui.Node Never! { panic("stub") }
// The system picker: files, and on phones the camera when `accept` holds
// images. The browser's `accept` is a hint only; the server checks the bytes.
file_button: fn(label: String, accept: [app.Media], max_bytes: Int, on_pick: fn(app.Upload) app.Notice!) -> ui.Node Never! { panic("stub") }
progress: fn(done: Int, total: Int) -> ui.Node Never! { panic("stub") }
swipe_row: fn(body: ui.Node, actions: [SwipeAction]) -> ui.Node Never! { panic("stub") }
swipe_action: fn(label: String, run: fn() app.Notice!) -> SwipeAction Never! { SwipeAction{label: label} }
// Overlays stack in one order per window: sheets and dialogs over the
// screen, a popover over its anchor, notices and toasts over everything. A
// dialog or a sheet holds focus while it is open, and Escape or Back closes it.
dialog: fn(title: String, open: ui.Local<Bool>, body: ui.Node) -> ui.Node Never! { panic("stub") }
sheet: fn(open: ui.Local<Bool>, body: ui.Node) -> ui.Node Never! { panic("stub") }
popover: fn(anchor: ui.Node, body: ui.Node) -> ui.Node Never! { panic("stub") }
// The account screens the `auth` battery needs: sign-up, email confirmation,
// password reset, passkeys and two-factor codes. They join the one route set.
account_flows: fn() -> app.Plugin Never! { app.Plugin{name: "account_flows"} }
```

</details>

<details>
<summary><code>native</code> stub</summary>

`stubs/native/native.jet`

```jet
// Stub surface for the proposed `native` package (#6799-#6810). Each
// function that changes the app returns an app.Plugin for `.with(...)`.
// Signatures only.
use {
    app: appstub
    ui
}

Menu: struct {
    title: String,
}

MenuItem: struct {
    title: String,
}

Symbol: enum {
    Folder,
    Person,
    Search,
    Crewboard,
}

FileType: enum {
    CSV,
    JSON,
    Image,
}

Tab: struct {
    title: String,
}

// A file the person chose, dropped or photographed. A path on desktops, a
// content URI on Android, a security-scoped URL on Apple systems and a File
// in the browser all give the same handle; no path string reaches the app.
PickedFile: struct {
    name: String,
    bytes: Int,
    media: app.Media?,

    read: fn(self) -[IO]> [U8] Never! { panic("stub") }
    text: fn(self) -[IO]> String? Never! { panic("stub") }
    // The handle as an upload argument; it streams when the call is sent.
    upload: fn(self) -> app.Upload Never! { app.Upload{name: self.name, bytes: self.bytes} }
}

Place: struct {
    latitude: Float,
    longitude: Float,
    accuracy_m: Float,
}

Permission: enum {
    Camera,
    Location,
    Notifications,
}

Grant: enum {
    Granted,
    Denied,
    // Parental controls or device policy; the person cannot change it.
    Restricted,
}

// Menus, the menu bar and the tray (#6799).
menu: fn(title: String, items: [MenuItem]) -> Menu Never! { Menu{title: title} }
item: fn(title: String, key: String?{None}, enabled: Bool{true}, run: fn() app.Notice!) -> MenuItem Never! { MenuItem{title: title} }
separator: fn() -> MenuItem Never! { MenuItem{title: ""} }
quit_item: fn() -> MenuItem Never! { MenuItem{title: "Quit"} }
edit_menu: fn() -> Menu Never! { Menu{title: "Edit"} }
window_menu: fn() -> Menu Never! { Menu{title: "Window"} }
// If `build` fails, the bar or the tray keeps its last menu and adds a
// disabled first item with the notice's text (and a Retry item when the
// notice allows one).
menu_bar: fn<S>(build: fn(S) -> [Menu] app.Notice!) -> app.Plugin Never! { app.Plugin{name: "menu_bar"} }
tray: fn<S>(icon: Symbol, build: fn(S) -> [MenuItem] app.Notice!) -> app.Plugin Never! { app.Plugin{name: "tray"} }
context_menu: fn(body: ui.Node, items: [MenuItem]) -> ui.Node Never! { panic("stub") }

// Phone navigation.
tabs: fn(tabs: [Tab]) -> app.Plugin Never! { app.Plugin{name: "tabs"} }
tab: fn<V>(view: V, title: String, icon: Symbol) -> Tab Never! { Tab{title: title} }

// Links (#6804). On desktops a second launch through the scheme hands its
// address to the running app, which opens it; the second process exits.
app_links: fn(domain: String) -> app.Plugin Never! { app.Plugin{name: "app_links"} }
url_scheme: fn(name: String) -> app.Plugin Never! { app.Plugin{name: "url_scheme"} }

// Notifications (#6800).
push: fn() -> app.Plugin Never! { app.Plugin{name: "push"} }
notify: fn<V>(title: String, body: String, open: V) -[IO]> Never! { panic("stub") }

// Background work (#6801) on desktops and phones. The system picks the
// moment; `every` is a floor. In a graph built for the web output it is a
// compile error: browsers run nothing for a closed page. No window is open
// while it runs, so a failed run's notice joins the app's notice list, which
// the frame shows when the app next opens and keeps until it is dismissed;
// the app's log keeps it too, with the run's time. The next run starts on the
// system's schedule as usual.
background: fn<S>(every: Duration, run: fn(S) app.Notice!) -> app.Plugin Never! { app.Plugin{name: "background"} }

// Files (#6802).
open_file: fn(types: [FileType]) -[IO]> PickedFile? Never! { panic("stub") }
save_file: fn(suggested: String, types: [FileType]) -[IO]> String? Never! { panic("stub") }
// A failed drop's notice shows beside the drop target, as a button's does.
drop_files: fn(body: ui.Node, types: [FileType], on_drop: fn([PickedFile]) app.Notice!) -> ui.Node Never! { panic("stub") }

// Camera, sensors and location (#6803).
take_photo: fn() -[IO]> PickedFile? Never! { panic("stub") }
location: fn() -[IO]> Place? Never! { panic("stub") }

// Permissions (#6806).
ask: fn(permission: Permission) -[IO]> Grant Never! { panic("stub") }
open_settings: fn() -[IO]> Never! { panic("stub") }

// Clipboard and share (#6805).
copy_text: fn(text: String) -[IO]> Never! { panic("stub") }
paste_text: fn() -[IO]> String? Never! { panic("stub") }
share: fn(url: String) -[IO]> Never! { panic("stub") }

// Lifecycle (#6809). A failed `on_resume` shows its notice at the top of the
// window that came back, and the frame announces it.
lifecycle: fn<S>(on_resume: fn(S) app.Notice!) -> app.Plugin Never! { app.Plugin{name: "lifecycle"} }

// D-THEME2's system-like preset.
theme: fn() -> ui.Preset Never! { ui.Preset{name: "system"} }
```

</details>

<details>
<summary><code>auth</code> and <code>core.db</code> stubs</summary>

`stubs/auth/auth.jet`

```jet
// Stub surface for the proposed `auth` package (D-AUTH1). Signatures only.
AccountID: distinct Int

Credentials: struct {
    email: String{""},
    password: #Secret String{""},
}

#Error
SignInError: enum {
    WrongPassword,
    Locked,
    Unreachable,
}

// The account store and the account lifecycle it runs (section 4.7).
Accounts: struct {
    path: String,

    // A new account must confirm its address before it signs in.
    verify_email: fn(self) -> Accounts Never! { Accounts{path: self.path} }
    // A mailed, single-use link that expires after an hour.
    reset_password: fn(self) -> Accounts Never! { Accounts{path: self.path} }
    // WebAuthn passkeys beside passwords.
    passkeys: fn(self) -> Accounts Never! { Accounts{path: self.path} }
    // TOTP codes, with recovery codes, that an account may turn on.
    two_factor: fn(self) -> Accounts Never! { Accounts{path: self.path} }
    // Per address and per network: failed sign-ins past the limit wait.
    limit_sign_in: fn(self, per_minute: Int) -> Accounts Never! { Accounts{path: self.path} }
}

sqlite: fn(path: String) -> Accounts Never! { Accounts{path: path} }

sign_in: fn(credentials: Credentials) SignInError! { panic("stub") }

describe: fn(error: SignInError) -> String Never! { panic("stub") }

// Server: the account's email address once it is confirmed, lowercased, as
// the account store keeps every address. Callers compare it with addresses
// they lowercased too.
verified_email: fn(account: AccountID) -> String? Never! { panic("stub") }
```

`stubs/dbstub/db.jet`

```jet
// Stub surface for the proposed typed tables (db.Table). Signatures only.
use {
    auth
}

#Error
DBError: enum {
    Closed,
    // A row policy refused the read or the write.
    Denied,
    // `add` found a row with that key, and the caller may read that row.
    // When the caller may not, `add` answers Denied instead, so a taken key
    // in another project tells the caller nothing.
    Exists,
    Constraint(String),

    summary: fn(self) -> String Never! {
        if self == {
            .Closed -> "closed"
            .Denied -> "denied"
            .Exists -> "exists"
            .Constraint(what) -> what
        }
    }
}

// Whose rights a query runs with. An #App.SignedIn endpoint runs as the
// caller's account; a webhook, a job and a schedule run as System.
Who: enum {
    Account(auth.AccountID),
    System,
}

// What a row policy is asked about. A read is judged on the stored row, an
// insert on the new row and a delete on the stored row. An update is judged
// twice, on the stored row and on the new one, as Postgres judges USING and
// WITH CHECK; both must pass. The policy's own reads see the transaction's
// earlier writes but not the write being judged, so a write cannot vouch for
// itself.
Op: enum {
    Read,
    Insert,
    Update,
    Delete,
}

// Group counts from `count_by`: one query, one number per group.
Counts: struct<G> {
    id: Int,

    of: fn(self, group: G) -> Int Never! { panic("stub") }
}

Rows: struct<T> {
    id: Int,

    filter: fn(self, keep: fn(T) -> Bool) -> Rows<T> Never! { Rows<T>{id: self.id} }
    limit: fn(self, count: Int) -> Rows<T> Never! { Rows<T>{id: self.id} }
    collect: fn(self) -[DB.Read]> [T] DBError! { panic("stub") }
    count: fn(self) -[DB.Read]> Int DBError! { panic("stub") }
    count_by: fn<G>(self, group: fn(T) -> G) -[DB.Read]> Counts<G> DBError! { panic("stub") }
}

Table: struct<K, T> {
    name: String,

    query: fn(self) -> Rows<T> Never! { Rows<T>{id: 0} }
    // Rows whose search text matches every word of `words`, through the
    // table's full-text index.
    search: fn(self, words: String) -> Rows<T> Never! { Rows<T>{id: 0} }
    get: fn(self, key: K) -[DB.Read]> T? DBError! { panic("stub") }
    // Inserts; a taken key is DBError.Exists, or DBError.Denied when the
    // caller may not read the row that holds it.
    add: fn(self, row: T) -[DB.Write]> T DBError! { panic("stub") }
    put: fn(self, row: T) -[DB.Write]> T DBError! { panic("stub") }
    delete: fn(self, key: K) -[DB.Write]> DBError! { panic("stub") }
    // The table's row policy (D-DBPOLICY1, #3709 c22), asked about every read
    // and write of the table for every caller (see Op). It reads the store
    // with System rights. A policy whose reads are key lookups, as each of
    // Crewboard's is, compiles into the query as a join, so a list read is
    // one statement and `limit` counts only the rows the policy passes.
    // Any other policy runs once per row, memoized per key within the
    // transaction, and `jet inspect choices` names it.
    guard: fn(self, policy: fn(T, Op, Who, Store) -> Bool DBError!) -> Table<K, T> Never! { Table<K, T>{name: self.name} }
}

Store: struct {
    path: String,

    // `search` names the text the full-text index holds for a row.
    table: fn<K, T>(self, name: String, key: fn(T) -> K, search: (fn(T) -> String)?{None}) -> Table<K, T> Never! { Table<K, T>{name: name} }
}

open: fn(path: String) -[DB.Read, DB.Write]> Store DBError! { Store{path: path} }
```

</details>

## 11. Sources

Every link was opened and returned the page named when this revision was
written.

| Key | Source |
|---|---|
| ANDROID-A11Y | [Android: accessibility](https://developer.android.com/guide/topics/ui/accessibility) |
| ANDROID-AL | [Android: App Links](https://developer.android.com/training/app-links?hl=en) |
| ANDROID-BG | [Android: background tasks (WorkManager)](https://developer.android.com/develop/background-work/background-tasks) |
| ANDROID-L10N | [Android: localization](https://developer.android.com/guide/topics/resources/localization) |
| ANDROID-PERM | [Android: requesting permissions](https://developer.android.com/training/permissions/requesting) |
| APNS-REQ | [Apple: sending notification requests to APNs](https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns) |
| APOLLO-CHECKS | [Apollo GraphOS: schema checks](https://www.apollographql.com/docs/graphos/platform/schema-management/checks) |
| APPLE-AD | [Apple: supporting associated domains](https://developer.apple.com/documentation/xcode/supporting-associated-domains) |
| APPLE-BG | [Apple: BackgroundTasks](https://developer.apple.com/documentation/backgroundtasks) |
| APPLE-NOTARY | [Apple: notarizing macOS software](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) |
| APPLE-UN | [Apple: UserNotifications](https://developer.apple.com/documentation/usernotifications) |
| ASPNET-API | [ASP.NET Core: automatic HTTP 400 responses](https://learn.microsoft.com/en-us/aspnet/core/web-api/#automatic-http-400-responses) |
| ASPNET-ID | [ASP.NET Core Identity](https://learn.microsoft.com/en-us/aspnet/core/security/authentication/identity) |
| BLZ-HYBRID | [ASP.NET Core Blazor Hybrid](https://learn.microsoft.com/en-us/aspnet/core/blazor/hybrid/) |
| BLZ-VAL | [Blazor forms validation](https://learn.microsoft.com/en-us/aspnet/core/blazor/forms/validation) |
| BUF | [buf: breaking change detection](https://buf.build/docs/breaking/) |
| CAP | [Capacitor APIs](https://capacitorjs.com/docs/apis) |
| CAP-CAM | [Capacitor Camera](https://capacitorjs.com/docs/apis/camera) |
| COMPOSE | [Jetpack Compose: thinking in Compose](https://developer.android.com/develop/ui/compose/mental-model) |
| COMPOSE-IMAGE | [Compose: loading images](https://developer.android.com/develop/ui/compose/graphics/images/loading) |
| COMPOSE-KEY | [Compose: lists and item keys](https://developer.android.com/develop/ui/compose/lists?hl=en) |
| COMPOSE-SAVE | [Compose: saving UI state](https://developer.android.com/develop/ui/compose/state-saving) |
| COMPOSE-WSC | [Compose: window size classes](https://developer.android.com/develop/ui/compose/layouts/adaptive/use-window-size-classes) |
| CVX-AUTH | [Convex: auth in functions](https://docs.convex.dev/auth/functions-auth) |
| CVX-INTERNAL | [Convex: internal functions](https://docs.convex.dev/functions/internal-functions) |
| CVX-RT | [Convex: realtime](https://docs.convex.dev/realtime) |
| CVX-SCHED | [Convex: scheduled functions](https://docs.convex.dev/scheduling/scheduled-functions) |
| CVX-SCHEMA | [Convex: schemas](https://docs.convex.dev/database/schemas) |
| DIOXUS | [Dioxus: fullstack on native](https://dioxuslabs.com/learn/0.7/essentials/fullstack/native) |
| ECTO | [Ecto.Changeset](https://hexdocs.pm/ecto/Ecto.Changeset.html) |
| ECTO-PRELOAD | [Ecto.Query: preload](https://hexdocs.pm/ecto/Ecto.Query.html#preload/3) |
| EF-CONC | [EF Core: concurrency conflicts](https://learn.microsoft.com/en-us/ef/core/saving/concurrency) |
| EF-QUERY | [EF Core: how queries work](https://learn.microsoft.com/en-us/ef/core/querying/how-query-works) |
| ELEC | [ElectricSQL: shapes](https://electric-sql.com/docs/guides/shapes) |
| ELECTRON-DND | [Electron: native file drag and drop](https://www.electronjs.org/docs/latest/tutorial/native-file-drag-drop) |
| ELECTRON-MENU | [Electron: Menu](https://www.electronjs.org/docs/latest/api/menu) |
| ELECTRON-SI | [Electron: app.requestSingleInstanceLock](https://www.electronjs.org/docs/latest/api/app) |
| ELECTRON-SIGN | [Electron: code signing](https://www.electronjs.org/docs/latest/tutorial/code-signing) |
| ELECTRON-TRAY | [Electron: Tray](https://www.electronjs.org/docs/latest/api/tray) |
| ELECTRON-UPD | [Electron: autoUpdater](https://www.electronjs.org/docs/latest/api/auto-updater) |
| ELECTRON-WIN | [Electron: BrowserWindow](https://www.electronjs.org/docs/latest/api/browser-window) |
| ELM | [Elm](https://elm-lang.org/) |
| ELM-ARCH | [The Elm Architecture](https://guide.elm-lang.org/architecture/) |
| ELM-HTTP | [Elm: Http.Error](https://package.elm-lang.org/packages/elm/http/latest/Http#Error) |
| ELM-NAV | [Elm: Browser.Navigation](https://package.elm-lang.org/packages/elm/browser/latest/Browser-Navigation) |
| EXPO-API | [Expo Router: API routes](https://docs.expo.dev/router/web/api-routes/) |
| EXPO-BG | [Expo BackgroundTask](https://docs.expo.dev/versions/latest/sdk/background-task/) |
| EXPO-BUILD | [EAS Build](https://docs.expo.dev/build/introduction/) |
| EXPO-CP | [Expo config plugins](https://docs.expo.dev/config-plugins/introduction/) |
| EXPO-LOC | [Expo Location](https://docs.expo.dev/versions/latest/sdk/location/) |
| EXPO-PERM | [Expo: permissions](https://docs.expo.dev/guides/permissions/) |
| EXPO-PUSH | [Expo: push notifications](https://docs.expo.dev/push-notifications/overview/) |
| EXPO-ROUTER | [Expo Router: introduction](https://docs.expo.dev/router/introduction/) |
| EXPO-UPD | [EAS Update](https://docs.expo.dev/eas-update/introduction/) |
| FASTAPI-BODY | [FastAPI: request body](https://fastapi.tiangolo.com/tutorial/body/) |
| FCM-COLLAPSE | [Firebase Cloud Messaging: collapsible messages](https://firebase.google.com/docs/cloud-messaging/concept-options#collapsible_and_non-collapsible_messages) |
| FLUTTER | [Flutter architectural overview](https://docs.flutter.dev/resources/architectural-overview) |
| FLUTTER-A11Y | [Flutter: accessibility](https://docs.flutter.dev/ui/accessibility) |
| FLUTTER-ADAPT | [Flutter: adaptive and responsive design](https://docs.flutter.dev/ui/adaptive-responsive) |
| FLUTTER-DL | [Flutter: deep linking](https://docs.flutter.dev/ui/navigation/deep-linking) |
| FLUTTER-I18N | [Flutter: internationalization](https://docs.flutter.dev/ui/accessibility-and-internationalization/internationalization) |
| FLUTTER-THEME | [Flutter: ThemeData](https://api.flutter.dev/flutter/material/ThemeData-class.html) |
| FLUTTER-THEMES | [Flutter: themes cookbook](https://docs.flutter.dev/cookbook/design/themes) |
| FLUTTER-WEB | [Flutter: web support](https://docs.flutter.dev/platform-integration/web) |
| FORTIFY | [Laravel Fortify](https://laravel.com/docs/fortify) |
| FS-OFF | [Firestore: offline data](https://firebase.google.com/docs/firestore/manage-data/enable-offline) |
| FS-RULES | [Firestore: security rules](https://firebase.google.com/docs/firestore/security/get-started) |
| GH-HOOK | [GitHub: validating webhook deliveries](https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries) |
| GH-HOOK-BP | [GitHub: webhook best practices](https://docs.github.com/en/webhooks/using-webhooks/best-practices-for-using-webhooks) |
| HOTWIRE | [Hotwire](https://hotwired.dev/) |
| KMP | [Kotlin Multiplatform: supported platforms](https://kotlinlang.org/docs/multiplatform/supported-platforms.html) |
| LAR-AUTH | [Laravel: authorization](https://laravel.com/docs/authorization) |
| LAR-BC | [Laravel: broadcasting](https://laravel.com/docs/broadcasting) |
| LAR-QUEUE | [Laravel: queues](https://laravel.com/docs/queues) |
| LAR-VAL | [Laravel: validation](https://laravel.com/docs/validation) |
| LEP-SF | [Leptos: server functions](https://book.leptos.dev/server/25_server_functions.html) |
| LEP-SSR | [Leptos: SSR modes](https://book.leptos.dev/ssr/23_ssr_modes.html) |
| LUSTRE-SC | [Lustre: server components](https://hexdocs.pm/lustre/lustre/server_component.html) |
| MDN-BLOB | [MDN: Content-Disposition](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Disposition) |
| MDN-CACHE | [MDN: HTTP caching](https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/Caching) |
| MDN-CC | [MDN: Cache-Control](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Cache-Control) |
| MDN-CLIP | [MDN: Clipboard API](https://developer.mozilla.org/en-US/docs/Web/API/Clipboard_API) |
| MDN-CONTRAST | [MDN: prefers-contrast](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/prefers-contrast) |
| MDN-CQ | [MDN: container queries](https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_containment/Container_queries) |
| MDN-DIALOG | [MDN: the dialog element](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/dialog) |
| MDN-DND | [MDN: HTML drag and drop](https://developer.mozilla.org/en-US/docs/Web/API/HTML_Drag_and_Drop_API) |
| MDN-FILE | [MDN: file input](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/input/file) |
| MDN-FORCED | [MDN: forced-colors](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/forced-colors) |
| MDN-FSA | [MDN: File System API](https://developer.mozilla.org/en-US/docs/Web/API/File_System_API) |
| MDN-PUSH | [MDN: Push API](https://developer.mozilla.org/en-US/docs/Web/API/Push_API) |
| MDN-PWA | [MDN: making PWAs installable](https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Guides/Making_PWAs_installable) |
| MDN-RM | [MDN: prefers-reduced-motion](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/prefers-reduced-motion) |
| MDN-SHARE | [MDN: Web Share API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Share_API) |
| MDN-VARS | [MDN: CSS custom properties](https://developer.mozilla.org/en-US/docs/Web/CSS/Using_CSS_custom_properties) |
| METEOR | [Meteor API](https://docs.meteor.com/api/meteor) |
| METEOR-METHODS | [Meteor guide: methods](https://guide.meteor.com/methods.html) |
| NAV-TYPESAFE | [Navigation Compose: type safety](https://developer.android.com/guide/navigation/design/type-safety) |
| NEXT-CACHE | [Next.js: caching](https://nextjs.org/docs/app/getting-started/caching) |
| NEXT-ERR | [Next.js: error handling](https://nextjs.org/docs/app/getting-started/error-handling) |
| NEXT-LOADING | [Next.js: loading.js](https://nextjs.org/docs/app/api-reference/file-conventions/loading) |
| NEXT-META | [Next.js: metadata and OG images](https://nextjs.org/docs/app/getting-started/metadata-and-og-images) |
| NEXT-SEC | [Next.js: data security](https://nextjs.org/docs/app/guides/data-security) |
| OBAN | [Oban](https://hexdocs.pm/oban/Oban.html) |
| OTEL | [OpenTelemetry documentation](https://opentelemetry.io/docs/) |
| OWASP-CSV | [OWASP: CSV injection](https://owasp.org/www-community/attacks/CSV_Injection) |
| OWASP-PT | [OWASP: path traversal](https://owasp.org/www-community/attacks/Path_Traversal) |
| PG-FTS | [PostgreSQL: full text search](https://www.postgresql.org/docs/current/textsearch.html) |
| PG-POLICY | [PostgreSQL: CREATE POLICY](https://www.postgresql.org/docs/current/sql-createpolicy.html) |
| PHX-AUTH | [Phoenix: mix phx.gen.auth](https://hexdocs.pm/phoenix/mix_phx_gen_auth.html) |
| PHX-FORM | [LiveView: form bindings](https://hexdocs.pm/phoenix_live_view/form-bindings.html) |
| PHX-LV | [Phoenix.LiveView](https://hexdocs.pm/phoenix_live_view/Phoenix.LiveView.html) |
| PHX-PRES | [Phoenix.Presence](https://hexdocs.pm/phoenix/Phoenix.Presence.html) |
| PHX-REL | [Phoenix: releases](https://hexdocs.pm/phoenix/releases.html) |
| PHX-TEST | [Phoenix.LiveViewTest](https://hexdocs.pm/phoenix_live_view/Phoenix.LiveViewTest.html) |
| PHX-TOKEN | [Phoenix.Token](https://hexdocs.pm/phoenix/Phoenix.Token.html) |
| PHX-UP | [LiveView: uploads](https://hexdocs.pm/phoenix_live_view/uploads.html) |
| PHX-VR | [Phoenix.VerifiedRoutes](https://hexdocs.pm/phoenix/Phoenix.VerifiedRoutes.html) |
| PS | [PowerSync: sync rules](https://docs.powersync.com/usage/sync-rules) |
| QT-ANIM | [Qt Quick: animations](https://doc.qt.io/qt-6/qtquick-statesanimations-animations.html) |
| QT-STYLE | [Qt Quick Controls: styles](https://doc.qt.io/qt-6/qtquickcontrols-styles.html) |
| QT-TRAY | [Qt: QSystemTrayIcon](https://doc.qt.io/qt-6/qsystemtrayicon.html) |
| QWIK | [Qwik: resumability](https://qwik.dev/docs/concepts/resumable/) |
| RAILS-AR | [Rails: Active Record basics](https://guides.rubyonrails.org/active_record_basics.html) |
| RAILS-AS | [Rails: Active Storage](https://guides.rubyonrails.org/active_storage_overview.html) |
| RAILS-CB | [Rails: Active Record callbacks](https://guides.rubyonrails.org/active_record_callbacks.html) |
| RAILS-RL | [Rails: rate limiting](https://api.rubyonrails.org/classes/ActionController/RateLimiting/ClassMethods.html) |
| RAILS-SEC | [Rails: security guide](https://guides.rubyonrails.org/security.html) |
| RAILS-TEST | [Rails: testing](https://guides.rubyonrails.org/testing.html) |
| RAILS-VAL | [Rails: validations](https://guides.rubyonrails.org/active_record_validations.html) |
| REACT-RSC | [React: Server Components](https://react.dev/reference/rsc/server-components) |
| REACT-STREAM | [React: renderToPipeableStream](https://react.dev/reference/react-dom/server/renderToPipeableStream) |
| REACT-TAINT | [React: taintUniqueValue](https://react.dev/reference/react/experimental_taintUniqueValue) |
| REP | [Replicache: how it works](https://doc.replicache.dev/concepts/how-it-works) |
| RN | [React Native: core and native components](https://reactnative.dev/docs/intro-react-native-components) |
| RN-A11Y | [React Native: accessibility](https://reactnative.dev/docs/accessibility) |
| RR-ACT | [React Router: actions](https://reactrouter.com/start/framework/actions) |
| SCOUT | [Laravel Scout](https://laravel.com/docs/scout) |
| SECRECY | [Rust secrecy crate](https://docs.rs/secrecy/latest/secrecy/) |
| SK-ENV | [SvelteKit: $env/static/private](https://svelte.dev/docs/kit/$env-static-private) |
| SK-ERR | [SvelteKit: errors](https://svelte.dev/docs/kit/errors) |
| SK-RF | [SvelteKit: remote functions](https://svelte.dev/docs/kit/remote-functions) |
| SK-SW | [SvelteKit: service workers](https://svelte.dev/docs/kit/service-workers) |
| SOLIDQ | [Solid Queue](https://github.com/rails/solid_queue) |
| STRIPE-IDEM | [Stripe: idempotent requests](https://docs.stripe.com/api/idempotent_requests) |
| STRIPE-SIG | [Stripe: webhook signatures](https://docs.stripe.com/webhooks/signature) |
| STYLEX | [StyleX](https://stylexjs.com/) |
| SUPA-RLS | [Supabase: row level security](https://supabase.com/docs/guides/database/postgres/row-level-security) |
| SWIFTUI | [SwiftUI: managing user interface state](https://developer.apple.com/documentation/swiftui/managing-user-interface-state) |
| SWIFTUI-A11Y | [SwiftUI: view accessibility](https://developer.apple.com/documentation/swiftui/view-accessibility) |
| SWIFTUI-CONTRAST | [SwiftUI: colorSchemeContrast](https://developer.apple.com/documentation/swiftui/environmentvalues/colorschemecontrast) |
| SWIFTUI-CS | [SwiftUI: colorScheme](https://developer.apple.com/documentation/swiftui/environmentvalues/colorscheme) |
| SWIFTUI-ENV | [SwiftUI: Environment](https://developer.apple.com/documentation/swiftui/environment) |
| SWIFTUI-ID | [SwiftUI: id(_:)](https://developer.apple.com/documentation/swiftui/view/id(_:)) |
| SWIFTUI-MBE | [SwiftUI: MenuBarExtra](https://developer.apple.com/documentation/swiftui/menubarextra) |
| SWIFTUI-MOD | [SwiftUI: ViewModifier](https://developer.apple.com/documentation/swiftui/viewmodifier) |
| SWIFTUI-SP | [SwiftUI: ScenePhase](https://developer.apple.com/documentation/swiftui/scenephase) |
| T2 | [Tauri 2: plugins](https://v2.tauri.app/plugin/) |
| TAURI-DIALOG | [Tauri: dialog](https://v2.tauri.app/plugin/dialog/) |
| TAURI-DL | [Tauri: deep linking](https://v2.tauri.app/plugin/deep-linking/) |
| TAURI-NOTIF | [Tauri: notifications](https://v2.tauri.app/plugin/notification/) |
| TAURI-SI | [Tauri: single instance](https://v2.tauri.app/plugin/single-instance/) |
| TAURI-UPD | [Tauri: updater](https://v2.tauri.app/plugin/updater/) |
| TRPC | [tRPC](https://trpc.io/docs) |
| TSR | [TanStack Router: type safety](https://tanstack.com/router/latest/docs/framework/react/guide/type-safety) |
| TURBO-STREAMS | [Turbo Streams](https://turbo.hotwired.dev/handbook/streams) |
| TUS | [tus resumable uploads](https://tus.io/) |
| TW | [Tailwind CSS](https://tailwindcss.com/docs) |
| TW-DARK | [Tailwind CSS: dark mode](https://tailwindcss.com/docs/dark-mode) |
| VE | [vanilla-extract](https://vanilla-extract.style/) |
| VE-CONTRACT | [vanilla-extract: createThemeContract](https://vanilla-extract.style/documentation/api/create-theme-contract/) |
| VITE | [Vite: features](https://vite.dev/guide/features) |
| WCAG-CONTRAST | [WCAG 2.2: contrast (minimum)](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html) |
| WCAG-DRAG | [WCAG 2.2: dragging movements](https://www.w3.org/WAI/WCAG22/Understanding/dragging-movements.html) |
| WEBAUTHN | [W3C: Web Authentication](https://www.w3.org/TR/webauthn-3/) |
| WEBDEV-PWA | [web.dev: learn PWA](https://web.dev/learn/pwa/) |
| WEBLOCKS | [MDN: Web Locks API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Locks_API) |
| ZERO | [Zero: mutators](https://zero.rocicorp.dev/docs/mutators) |

## Appendix A. How the code was checked

### A.1 Setup

- Compiler: the seed `jet check` of build seed-b15 (identity
  `jet@a0.0.45#1b3638536ec7e43ed264a5cf665df0a225293f9a4c073395cc58a3aefc0c3a77`,
  read by `check.sh` from the `IDENTITY` file beside the seed), with a dev
  checkout at `2fcd2d5247` as the toolchain root.
- One file at a time, under an 8 GiB memory cap.
- Four packages are checked:
  - `crewboard` (section 3);
  - `examples` (sections 4–6);
  - `style_bugs` (section 6.5);
  - `probes`, the claim probes of 4.1 and 4.7.

  They depend on the stub packages of section 10.

### A.2 What the checked copy changes

The files in this document are the files as written. A build script makes a
checked copy with six mechanical changes, because the stubs are ordinary
packages, not Core:

| Change | Why |
|---|---|
| TR1: `use { app }` reads the stub package `appstub`; `db: core.db` reads `dbstub` | `app` and the typed-table `db` would live in Core; a stub cannot |
| TR2: `app.Remote<Crew>` reads as `CrewRemote`, a struct the script generates in `Logic/remote_gen.jet` with only Crew's `#App.SignedIn` and `#App.Public` methods | a stub cannot forward a service's methods; the generated struct keeps the boundary, so a client call to an unmarked method is still an error (the `cl1_server_only` probe) |
| TR3: version dependencies on `ui`, `kit`, `auth` and `native` read the stub paths; `outputs:` is removed | the packages do not exist; the root manifest is checked separately (A.5) |
| TR4: `#Codable` and `#PublishedSchema` are dropped in `Logic` | no `distinct` ID type derives a codec today (#6819) |
| TR5: in a `kit.form` fields closure, `f.text(f.title, …)` reads `f.text(kit.field("title"), …)` | the seed cannot derive one `Field` per struct field (#6821) |
| TR6: a handler parameter typed `fn() app.Notice!` or `fn(...) -> [T] app.Notice!` in the `kit`, `native` and `ui` stubs reads `fn() E!` or `fn(...) -> [T] E!` for a type parameter `E` | the seed does not find a conversion declared in another package (A.6 finding 7) |

### A.3 Results per file

Lines are non-blank lines of the file as written, the block in this
document. The checked copy can be shorter: TR4 drops the `#Codable` and
`#[Codable, PublishedSchema]` lines of `Logic`, so the checked `model.jet`
has 16 lines fewer. "Errors" are the errors the seed reports in that file;
"clean" means no errors. Warnings are counted below the table.

Round 2's table counted a file as clean when one of its dependencies failed,
so files that import `screens` were never checked themselves. Since revision
3 no package in `crewboard` has an error, so every file below was checked in
full.

The seed reports 48 distinct warnings across the packages: 10 in `Logic`
and 38 in `Screens`; `Desktop` and the examples have none.

- 41 are `jet.InferredNeverReturn`. They come from the stubs: a stub body is
  `panic("stub")`, so a function that calls one always diverges.
- 4 are `jet.RedundantFinalReturn` at `validate {`, which is A.6 finding 6.
- 3 are `jet.UnusedParameter` from the files: `projects`, `mine` and
  `sign_in` take their screen's view as every route does, though the view
  holds no data.

| File | Lines | Errors |
|---|---|---|
| `style_bugs/b1_token_typo.jet` | 10 | `jet.UnknownField` at 10 |
| `style_bugs/b2_wrong_kind.jet` | 10 | `jet.BindingTypeMismatch` at 10 |
| `style_bugs/b3_conflict.jet` | 10 | `jet.InvalidConstructionField` at 10 |
| `style_bugs/b4_image_alt.jet` | 10 | `jet.MissingArgument` at 10 |
| `style_bugs/b5_icon_label.jet` | 10 | `jet.MissingArgument` at 10 |
| `style_bugs/b6_unknown_state.jet` | 10 | `jet.UnknownArgumentLabel` at 10 |
| `style_bugs/b7_sheet_class.jet` | 13 | clean |
| `style_bugs/b8_contrast.jet` | 10 | clean |
| `crewboard/Desktop/desktop.jet` | 78 | clean |
| `crewboard/Logic/crew.jet` | 431 | clean |
| `crewboard/Logic/logic_test.jet` | 217 | clean |
| `crewboard/Logic/model.jet` | 244 | clean |
| `crewboard/Logic/notices.jet` | 23 | clean |
| `crewboard/Logic/policy.jet` | 116 | clean |
| `crewboard/Logic/remote_gen.jet` | 27 | clean |
| `crewboard/Logic/routes.jet` | 30 | clean |
| `crewboard/Logic/rules.jet` | 42 | clean |
| `crewboard/Phone/phone.jet` | 25 | clean |
| `crewboard/Screens/board.jet` | 64 | clean |
| `crewboard/Screens/members.jet` | 73 | clean |
| `crewboard/Screens/mine.jet` | 21 | clean |
| `crewboard/Screens/new_task.jet` | 22 | clean |
| `crewboard/Screens/projects.jet` | 66 | clean |
| `crewboard/Screens/screens_test.jet` | 49 | clean |
| `crewboard/Screens/search.jet` | 23 | clean |
| `crewboard/Screens/shell.jet` | 31 | clean |
| `crewboard/Screens/sign_in.jet` | 36 | clean |
| `crewboard/Screens/task.jet` | 71 | clean |
| `crewboard/Screens/theme.jet` | 62 | clean |
| `crewboard/Web/web.jet` | 24 | clean |
| `examples/access.jet` | 21 | clean |
| `examples/background.jet` | 25 | clean |
| `examples/camera.jet` | 26 | clean |
| `examples/drag.jet` | 21 | `jet.UninferableTypeArgument` at 20 |
| `examples/files.jet` | 67 | clean |
| `examples/lifecycle.jet` | 24 | clean |
| `examples/links.jet` | 17 | clean |
| `examples/notify.jet` | 16 | clean |
| `examples/realtime.jet` | 17 | clean |
| `examples/seo.jet` | 36 | clean |
| `examples/style_b.jet` | 58 | clean |
| `examples/style_c.jet` | 71 | clean |
| `examples/style_more.jet` | 57 | clean |
| `examples/tray.jet` | 32 | clean |
| `examples/web_delivery.jet` | 24 | clean |
| `examples/words.jet` | 16 | clean |
| `probes/cl1_server_only.jet` | 11 | `jet.UnknownFunction` at 9; `jet.UnknownFunction` at 10 |
| `probes/cl5_secret_print.jet` | 8 | clean |

### A.4 Known errors, each with its card

| File | Error | Cause | Card |
|---|---|---|---|
| `examples/drag.jet` | `jet.UninferableTypeArgument` at line 20 | a closure `fn(task: logic.Task) {...}` passed to a generic `on_drop` does not fix the type argument from its parameter | #6819 c4 |
| `probes/cl1_server_only.jet` | `jet.UnknownFunction` at lines 9 and 10 | intended: a screen calls the signing key and a job through `app.Remote<Crew>` (CL1) | — |
| `probes/cl5_secret_print.jet` | none | expected today: the secret rule is proposed in #6557 and not built, so the seed accepts a printed secret (CL5) | #6557 |
| `style_bugs/b1`–`b6` | one error each, as section 6.5 lists | seeded on purpose | — |
| `style_bugs/b7`, `b8` | none | not caught today; section 6.5 | #6817 c1, c2 |

### A.5 Manifests

- The root `crewboard/package.jet`, with its `outputs:`, was checked as
  written beside an empty program: no problems. The seed parses output blocks
  but does not check their kinds yet, so `updates:` and `purposes:` are
  checked for syntax only.
- `crewboard/Logic/package.jet` checks the same way. The other manifests
  have path dependencies and are checked in place with the files that use
  them.

### A.6 Seed findings

Found while checking these files. Each is filed under #6819.

1. A distinct type such as `ProjectID: distinct Int` does not derive a codec
   (`jet.NotSerializable`).
2. A static call or a payload case through a module path from another package
   (`logic.ProjectID.from_int(1)`, `app.CachePolicy.Public(max_age: 10min)`)
   gives `jet.ModuleAsValue`. A contextual `.Public(...)` works.
3. A closure passed where the parameter is `fn() -> R E!` gives
   `jet.UninferableTypeArgument` for `R` when its last call returns nothing.
   The stubs type every handler as `fn() app.Notice!`, which has no `R`.
4. The `AliasWithoutParameters` fix-it suggests `Name: struct(BaseType)`,
   which does not parse.
5. An unknown key in a package manifest makes path dependencies on that
   package fail with `jet.UseTargetNotFound` instead of naming the key.
6. Every `validate {` block gets the warning `jet.RedundantFinalReturn`
   (L0530), though it has no `return`: the four in `Logic/model.jet`, at
   lines 52, 65, 78 and 90 as written.
7. Package B declares `impl Oops -> pa.Notice` for its own error and a type
   from package A. The conversion works inside B, but package C does not
   find it. Propagating B's error with `_:` in C gives
   `jet.MissingErrorConversion` or `jet.ErrorNotPropagatable` (#6819 c7).
   This is why TR6 exists. The three-package probe is kept in the kit.

### A.7 Running it again

The kit holds the files above, the stubs, the probes and the scripts. It is
kept at `$JET_SCRATCH_ROOT/lanes/G12Docs/webshape-r5-kit/`. The kit is a
working copy and may be cleaned away; every file it checks is embedded in
this document verbatim, so the files can be rebuilt from the text.

```sh
cd "$JET_SCRATCH_ROOT/lanes/G12Docs/webshape-r5-kit"
node build.mjs                       # writes chk/ and chk/files.txt
JET_WORKTREE=<dev checkout> JET_SEED=<seed-b15 jet> \
  bash check.sh chk/files.txt run    # one file at a time, 8 GiB cap
node report.mjs run                  # the table in A.3
```
