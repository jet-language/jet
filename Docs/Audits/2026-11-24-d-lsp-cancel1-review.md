# Adversarial review: D-LSP-CANCEL1, what `$/cancelRequest` means to `jet self lsp` and when a cancelled request stops

Date: 2026-11-24
Reviewer: LspCancelReview (Claude Opus, fresh agent)
Ballot reviewed: `$JET_SCRATCH_ROOT/tmp/orch/ballots/D-LSP-CANCEL1.json` (draft, author G2Editor, recommendation A). The text as received is kept as `D-LSP-CANCEL1.json.before-review` beside it.
Compiler for probes: `$JET_SCRATCH_ROOT/tmp/orch/tip18-chain/s1/jet` (`jet a0.0.46-dev`, built from dev 568729492f)
Source read: dev worktree `Main-dev-collectall` at dc046ef3d1. 568729492f is its ancestor, and nothing under Compiler/JetLSP, Compiler/JetRuntime, Core/Term or Tests/Cases/LSP differs between the two.
Evidence: `$JET_SCRATCH_ROOT/tmp/orch/ballot/lspcancel/review-probes/`. Every output is in `review-probes/outputs-tip18.txt`, made by one run of all probes, one at a time. p1, p2, p3 and p5 feed `jet self lsp` framed messages (`frame.sh`) and print one line per server frame (`summarize.py`). p4 is a typing client in Python. p6 is a Jet program run at O0, O1 and O2.

Verdict scale: **holds**, **holds within limits** (the limits are stated), **fails**.
Finding IDs (F1…F15) are the items for the author. The open/closed state is in the last section.

## 0. Context the ballot depends on

- Card #6903 c1 (open): "Cancelled work returns the -32800 response and never later publishes a result as current". Its proof is the case filter `LSP/lsp_canceled_costly_request_returns_32800_and_never_publishes`. The card body says: "No new scheduling policy, worker count or public cancellation semantics is chosen silently." c4 mentions cancellation in host regions, and c3 moved to #6960 c10.
- Tower coverage. I listed all 352 decisions read-only. None ratifies LSP cancellation semantics, so the ballot is not blocked on coverage. D-LSP1=C (ratified, now retired and archived: one incremental compiler service) does not decide cancellation. Three ratified decisions constrain the options:
  - D-CORE-TERMRAW1=A: `term.read_key_within(wait: Duration) -[IO, Time.Wait]> Key? IOError!`, "A wait of zero checks without waiting". Its detail says: "The earlier poll-then-read option is dropped, because A's timed call already returns the key in one step." That is the owner's shape for a timed stdin read. A bare readiness predicate is the shape the owner dropped.
  - D-TERM-INCREMENTAL1=A: `term.keys()` returns the key stream as a `Receiver<Key>`; the runtime owns one process stdin buffer, and every Core stdin reader takes bytes only from it.
  - D-CANCELMODEL1=C, amended by D-TASK-SLICE1=A (spec.md:4433-4445): "Cancellation is preemptive at wait points and at checks … a loop or a recursion that only computes stops at its next iteration or call." The scheduler is M:N with time slices (spec.md:4863-4870).
  - Core's existing readiness calls take a deadline and return the answer in one step: `tcp_ready(stream, interest, deadline_ms)` (Net.jet:527), `ready(self, interest, /, *, deadline: Duration)` (TLS.jet:155). Receivers have `is_ready` and `try_recv` (Tasks.jet:292, 298).
- The server today:
  - Loop.jet:15-38 `lsp_read_message`: `read_until("\r\n\r\n")`, then one `take(length)`. `if bytes.len() != length` fails with "input ended inside a message body" (34-36).
  - Loop.jet:42-58 `lsp_serve` handles one message at a time. Loop.jet:96 calls `lsp_flush_dirty` before every request.
  - Diagnostics.jet:211-216 `lsp_flush_dirty` republishes "every dirty document and the open documents that read one". `lsp_publish_document_diagnostics` (220-234) runs `server.check(doc)` and records the publication.
  - Facts.jet:51: requests read `server.published.get(uri) ?? server.check(doc)`. So the costly check before a request is the publishing step, which no request owns. Only Completion, Hints and Tokens run extra per-request stages.
  - Requests.jet:53-54: "Cancellation and tracing need no answer". `$/cancelRequest` falls to `else -> {}`. didOpen publishes at once (58-62); didChange only marks the document dirty.
  - Navigation.jet:306 already answers -32801 ContentModified when a document "changed after the check; rename again".
  - No partial results, `workDoneProgress` or `$/progress` anywhere in JetLSP.
- Core and runtime stdin:
  - Core/Term/Term.jet offers `read_until` (262), `__take_bytes` (272, reached by `core.term.take`), `read_all_input`, `binread` and `read_key`. All of them wait. There is no readiness query and no timed read.
  - Compiler/JetRuntime/Source/TerminalInput.jet keeps one 64 KiB buffer. `term_input_fill` (69-86) does one read into it. `term_input_take` (92-102) returns buffered bytes when any are buffered, else the result of one direct read.
  - RouteTerm.jet:245-249 `jet_terminal8_take`: "One stdin read of up to `n` bytes (the buffered ones first, without a read)". `read_until` leaves bytes past the delimiter buffered.
  - Every stdin read goes through `rt_prove_stdin_read` (ProveJournal.jet:139) and `replay_read` (RecordReplay.jet:578). Prove capture and `jet debug` replay journal each read.
  - Stdin is implemented only in the Linux pack. `rt_prove_stdin_os_read` (ProveJournal.jet:168-175) does "a raw Linux read of stdin" through `host_syscall`.
- Cases:
  - `lsp_canceled_costly_request_returns_32800_and_never_publishes.case` is known-failing at Tests/known-failing.tsv:**358**, not 359 as the ballot says. Its header `PENDING-SEAM(read_bytes): Main ballot + CoreState2 + kit batch three` names the harness's child-read seam. It sends didChange v2 (49,248 B), request 50 and the cancel as three `send` lines (11-13). It then expects id 50 error -32800, no `"result":`, and no publication of any version but 2 (14-17).
  - `lsp_late_cancel_does_not_poison_a_reused_request_id.case` (passing). Its cancel for id 2 is sent **before** any request 2 (lines 11-12), so it tests an early cancel of an unseen id, not a late one. Its header cites D-CHILD-READ-BYTES1, which does not exist in Tower.
  - `lsp_overlapping_edits_latest_checked_revision_wins.case` (passing). It sends request 2, then edits to v11, v12 and a stale v11, and expects request 2 to answer `marker_one`, not `marker_new` (14-21). Later it sends a cancel of the answered id 3 and a new request 3, and expects no -32800 (31-36).

## 1. Strongest existing solutions, stated fairly

| Language / library | How a cancel takes effect | Best case | What it does that this design does not, or the ballot omits |
|---|---|---|---|
| LSP 3.17 specification | `$/cancelRequest {id: integer \| string}`. "A request that got canceled still needs to return from the server and send a response back … In addition it allows for returning partial results on cancel. If the request returns an error response on cancellation it is advised to set the error code to `ErrorCodes.RequestCancelled`." | The base rule every option keeps. | Three rules the ballot omits. (1) `$/` notifications: "if the server implementation uses a single threaded synchronous programming language then there is little a server can do to react to a `$/cancelRequest` notification … it is free to ignore the notification". That is D at its strongest. (2) Implementation Considerations: "servers should therefore not decide by themselves to cancel requests simply due to that fact that a state change notification is detected in the queue". (3) ContentModified: "A server should NOT send this error code if it detects a content change in its unprocessed messages." Rules (2) and (3) forbid A's "edits among them are applied first" when the edits come after a request. Also: ordering ("should not reorder `textDocument/definition` and `textDocument/rename`"), and partial results on RequestCancelled. Source: microsoft.github.io/language-server-protocol, specification 3.17, sections Cancellation Support, $ Notifications, Request/Notification/Response Ordering, Implementation Considerations. |
| rust-analyzer (lsp-server + salsa) | A reader thread (lsp-server `stdio.rs`, thread "LspServerReader") feeds a channel. `$/cancelRequest` → `GlobalState::cancel` → `req_queue.incoming.cancel(id)`, which answers `ErrorCode::RequestCanceled`, "canceled by client", **at once** for any pending request, queued or running. `respond` drops a later result (`incoming.complete` returns None). | Immediate -32800 with no stop: the work runs on, and its answer is discarded. | Salsa unwinding is triggered by **writes** (`apply_change` → `trigger_cancellation`), not by `$/cancelRequest`. The unwound request answers **ContentModified** (`content_modified_error`, handlers/dispatch.rs:304-310) or is retried (`Task::Retry`). The ballot's code `Err(Cancelled) => lsp_server::ErrorCode::RequestCanceled` and its surface line "stops running work at its next check" are both wrong. Sources: github.com/rust-lang/rust-analyzer `lib/lsp-server/src/{stdio.rs,req_queue.rs}`, `crates/rust-analyzer/src/{global_state.rs,handlers/dispatch.rs}`. |
| gopls | jsonrpc2 reads on its own goroutine. `CancelHandler` parses the id as a number (`NewIntID`) or a string (`NewStringID`) and cancels that request's context. A reply after the cancel becomes RequestCancelled: `if ctx.Err() != nil && err == nil { err = RequestCancelledError }`. | Running work stops at its next `ctx` check; the answer is -32800 even when the work finished. | A comment in the code says the reply comes only when the handler returns: "See if we can generate a reply for the request to be cancelled at the point of cancellation rather than waiting for gopls to naturally reply." Source: golang/tools `gopls/internal/protocol/protocol.go`. |
| clangd | `onCancel` looks up `RequestCancelers[to_string(ID)]` (the JSON-serialized id) and invokes it. An unknown id is ignored. "If a client reuses an ID, the last wins and the first cannot be canceled"; a cookie keeps cleanup from removing an overwritten entry. TUScheduler also invalidates queued `InvalidateOnUpdate` reads when a content-changing update arrives, with reason ContentModified. | The precise id rule (JSON value, last wins) that the ballot lacks. | Its InvalidateOnUpdate goes further than the specification advises (rule 2 above). Sources: llvm-project `clang-tools-extra/clangd/ClangdLSPServer.cpp` (`onCancel`, `cancelableRequestContext`), `TUScheduler.cpp` (`ASTWorker::startTask`). |
| tsserver | Single-threaded. The client starts it with `--cancellationPipeName`; to cancel, it creates a named pipe whose **existence** tsserver checks (`isCancellationRequested: () => pipeExists(...)`, per request with a trailing `*`). Long operations call `cancellationToken.throwIfCancellationRequested()` and throw `OperationCanceledException`. | **The direct precedent for B in a single-threaded server.** It reads the cancel out of band, not from the message stream. | The ballot cites the token but not the side channel, which is what lets a single thread see a cancel while busy. Sources: TypeScript v5.8.3 `src/tsserver/nodeServer.ts` (`createCancellationToken`), `src/services/services.ts` (`CancellationTokenObject`). |
| vscode-languageclient (the client in Mia's story) | `documentSymbol.ts:98`: `if (token.isCancellationRequested \|\| data === undefined \|\| data === null) { return null; }`. `handleFailedRequest` (client.ts:2335-2363) swallows RequestCancelled and ServerCancelled for a cancelled token. ContentModified is turned into a CancellationError for semantic tokens and resolve calls; for other requests it returns the default value. | Late results of cancelled requests are dropped by the client. | That refutes the story's "an old outline can show up after the new text". The real cost of D is wasted checks and latency. Source: github.com/microsoft/vscode-languageserver-node `client/src/common/{documentSymbol.ts,client.ts}`. |

## 2. Claims and verdicts (round 0, the text as received)

### C1. Situation: "a cancel arrives only after the request it names has already been answered, and the server ignores it"
p1 sends the costly case's messages 0-6 in one write. The cancel for 50 is in the server's input before request 50 runs, and the server answers `id=50 result count=1026 … marker_new`. So the cancel is *read* late; it does not *arrive* late. p2: the late-cancel messages answer normally. The ignoring is confirmed.
**Verdict: holds within limits** ("arrives" should be "is read").

### C2. Current code and case facts
`read_until` plus `take` is right. Three errors:
- The known-failing row is line 358, not 359.
- `PENDING-SEAM(read_bytes)` is the test harness's child-read seam (the late-cancel case's header names the nonexistent D-CHILD-READ-BYTES1). It is not the server's missing Core read.
- The ballot misses the defect every option sits on. `take` is one read (RouteTerm.jet:245-249), and Loop.jet:34-36 treats a short body as end of input. p3b: a 6,759-byte body written in two parts 0.2 s apart gives exit 1, `[jet-lsp] input ended inside a message body`. p3d: a 170 KB stream forwarded in 16 KiB writes fails 5/5. p4: a realistic client (one `write` per burst) kills the server on the 3,000-line `didOpen` 3/3 times, and after 2-4 keystrokes at 1,000 and 1,500 lines.

**Verdict: fails** (F1, F15).

### C3. "Core has no way to ask whether standard input holds more bytes without blocking"
True for `core.term` (Term.jet), and StdinHandle has no readiness call either.
**Verdict: holds.**

### C4. LSP rule: "A cancelled request that never ran must still get an answer, and that answer should be error -32800"
The specification requires an answer. It *advises* -32800 if the answer is an error, and it allows a normal or partial result. The ballot leaves out three spec rules: the `$/` ignore rule, which is D's strongest ground, the Implementation Considerations rule against cancelling because of a queued state change, and the ContentModified rule about unprocessed messages.
**Verdict: holds within limits** (F4, F10).

### C5. "An edit that is already waiting is applied before the republish step, so the server never publishes diagnostics for a version the editor has already replaced"
This fails three ways:
- Edits queued **after** a request cannot be applied before it is answered (C8).
- didOpen publishes inside the notification handler (Requests.jet:58-62), not through the flush.
- An edit that arrives while the check runs is not seen until the next read.

What can hold is narrower: the publishing step before a request does not *send* diagnostics for a document with a queued newer edit. It must still check that document, because the request reads `server.published` (Facts.jet:51).
**Verdict: fails** (F5).

### C6. A's seam: `input_ready: fn() -[IO]> Bool`, "poll with a zero timeout on fd 0; on Windows, PeekNamedPipe or the console wait"
- **Misses the buffer.** `read_until` leaves bytes past the header in the 64 KiB runtime buffer, and a burst usually lands in one `term_input_fill` read. Polling fd 0 then says "not ready" while a cancel sits in user space. In p1 all seven frames (about 99 KB) were in the pipe before the server read; the first fill takes 64 KiB at once.
- **Not journaled.** Every stdin read is journaled (`rt_prove_stdin_read`, `replay_read`). A readiness answer is nondeterministic input, and replay without it diverges.
- **Wrong shape.** D-CORE-TERMRAW1 dropped poll-then-read for one-step timed reads ("A wait of zero checks without waiting"), and D-TERM-INCREMENTAL1 requires every stdin reader to take bytes from the one buffer.
- **Unbacked platforms.** Only the Linux pack has stdin code. The Windows and macOS sentences describe nothing that exists. "`jet dev` and the REPL can use it too" has no caller or card behind it.

**Verdict: fails** (F2, F3).

### C7. A: "reads every message already waiting on standard input without blocking"
`input_ready` true means at least one byte. `lsp_read_message` then blocks in `read_until` or `take` until the whole frame arrives, and a short `take` kills the server (C2). So "without blocking" is false for a partial frame. It holds only with A's own frame buffer over a timed byte read.
**Verdict: fails** (F1, F2).

### C8. A: "Edits among them are applied first"
Read literally, edits queued behind a request are applied before that request is answered. p5 runs the passing overlapping-edits case's messages 0-7 in one write. Today request 2 answers `markers=['marker_one']` while the v11/v12 edits wait behind it, which is what the case (lines 19-21) and the specification require. Under the literal A, request 2 would answer `marker_new`, and a passing case would regress. That is a position-shifting reorder the specification forbids (section 1, rules 2 and 3).
**Verdict: fails** (F4).

### C9. A: "A cancel for an id already answered, or never seen, is ignored … a cancel applies only to the request waiting under that id at drain time"
In the late-cancel case the cancel for 2 and request 2 can both be waiting at drain time, cancel first. "Waiting under that id at drain time" then cancels the request, and that passing case regresses. The sketch's `lsp_cancelled(queue, message)` looks only at messages *after* the request and would get it right, so text and sketch disagree.

Also unstated:
- JSON id types (gopls and clangd treat 2 and "2" as different).
- Duplicate in-flight ids (clangd: last wins).
- A cancel naming a notification, `initialize` or `shutdown`.
- Which wins among -32800, -32002 (not initialized) and -32600 (shutting down).
- That partial results and progress do not apply.

**Verdict: fails** (F6).

### C10. B: "a running check looks for a cancel between its stages … A check run to publish diagnostics is never cancelled, because no request owns it"
The costly check *is* the publishing step (Facts.jet:51, Loop.jet:96), so B as written stops almost nothing: only the per-request stages of Completion, Hints and Tokens. Its driver change is also unnecessary. Under D-CANCELMODEL1, a check run as a task stops at its next loop or call check when cancelled. p6 shows this at O0, O1 and O2: `check stopped: Canceled; 0 ms after the cancel`. B's strongest form is "run the check before a request as a task and cancel it when that request is cancelled". It needs no driver change.
**Verdict: fails** (F8).

### C11. C: "This chooses a worker count of two and a scheduling policy, which D-LSP1 and card #6903 say must not be chosen silently"
- A reader task is not a request worker. Requests still run one at a time, in order.
- The card forbids choosing *silently*, and a ballot is not silent, so the whyNot is circular.
- C needs **no** new Core call. p6: a reader task blocked in `read_until` receives its line while another task computes, and the cancel stops the computing task at once.
- C's real costs go unnamed: the server becomes concurrent, replay must reproduce when a message reached the loop relative to the check, and the reader owns stdin.

**Verdict: fails** (F9).

### C12. D: "Editors keep getting stale answers during fast typing"
VS Code's client drops results of cancelled requests (documentSymbol.ts:98). The real cost is wasted checks and latency: p4 at 30 ms per keystroke, answer lag rose from 39 ms to 340 ms (100 lines) and from 73 ms to 746 ms (300 lines). The ballot also omits D's best argument, that the specification lets a single-threaded server ignore `$/cancelRequest`.
**Verdict: holds within limits** (F10).

### C13. Comparisons
- The Rust row has the wrong error code and the wrong trigger (section 1).
- The TypeScript row omits the side channel.
- No row has a source.
- clangd, which has the id rules A needs, is missing.

**Verdict: fails** (F11).

### C14. Story: "The outline lags further behind with each keystroke, and an old outline can show up after the new text"
The lag half holds when check time exceeds the keystroke gap (p4). The old-outline half is false for VS Code (section 1). Today, Mia's 3,000-line file does not lag: the server exits on the first full-text message that arrives in pieces (p4, 3/3).
**Verdict: fails** (F12).

### C15. Recommendation why: "Editors cancel almost always in the same burst as the edit that made the request stale"
The cancel does ride in the burst of the next keystroke. But the request it names was sent one keystroke earlier. On an idle single-threaded server, that request is already answered (p4: every cancel is "late" while checks fit in the gap) or running (p4: 1,000 lines at 300 ms, 4 of 19 running, 0 waiting). A catches cancels only under backlog (p4 at 30 ms: 16 of 19 and 18 of 19 waiting).
**Verdict: holds within limits** (F13).

### C16. Gains: "No worker count, scheduling policy or driver change is chosen"; "the known-failing case can pass"
A does choose a scheduling policy: skip cancelled queued requests, and skip sending diagnostics for documents with queued newer edits. That is fine because the ballot states it, but "none chosen" is false. "The case can pass" is a race: the case sends edit, request and cancel in three writes, and A passes only if the third write is in the buffer when request 50 is taken.
**Verdict: fails** (F7, F13).

### C17. Losses and mitigations
- "B only adds safe points to the driver": wrong mechanism (C10).
- "a zero-timeout poll on Linux and macOS and a peek on Windows, and `jet dev` and the REPL can use it too": unbacked (C6).
- Missing loss: a cancelled request skips `lsp_flush_dirty`, so diagnostics for the edit that preceded it wait for the next non-cancelled request.

**Verdict: fails** (F3, F14).

### C18. Surface
It mirrors the body, errors included: the wrong rust-analyzer line in `trio.wild` (F11) and `input_ready` (F2). It also says nothing about the short-body defect (F1).
**Verdict: holds within limits.**

### C19. Coverage: nothing ratified decides this
Holds; see section 0. The constraints are on the seam's shape (F2), not on the question.
**Verdict: holds.**

### Checklist (AGENTS.md "Adversarial design review" and ballot-review.md)

| Item | State in the draft |
|---|---|
| Every site of the mechanism named (read loop, runtime buffer, flush, didOpen publish, replay journal, cases) | Fails (C2, C5, C6; F1, F3, F5) |
| Siblings (Core timed reads, `term.keys()`, net/TLS `ready`, task cancel) | Fails (F2, F8, F9) |
| Ratified text respected (D-CORE-TERMRAW1, D-TERM-INCREMENTAL1, D-CANCELMODEL1) | Fails (F2, F8) |
| Strongest prior art, sourced | Fails (F11) |
| Each option at its strongest | Fails for B, C and D (F8, F9, F10) |
| Edge cases: late cancel, reused ids, cancel of a notification, cancelled didChange-triggered recheck, partial results, progress tokens | Fails (F6, F14); partial results and progress do not apply but are not stated |
| What the recommendation gives up | Fails (F13, F14) |
| Cases deterministic for the chosen option | Fails (F7) |
| Code facts and line numbers | Fails (F15) |

## 3. Findings (round 0)

- **F1 Short-body defect, owed by every option.** `take` is one read (RouteTerm.jet:245-249) and `lsp_read_message` treats a short body as end of input (Loop.jet:34-36). Reproductions, all from `outputs-tip18.txt`:
  - p3b: a 6,759-byte body in two writes 0.2 s apart; exit 1, `[jet-lsp] input ended inside a message body`.
  - p3d: a 170 KB stream in 16 KiB writes 5 ms apart; 5/5 fail.
  - p4 with 3,000 lines: the server dies on `didOpen`, after only the `initialize` answer.
  - p4 with 1,000 lines at 100 ms and 1,500 lines at 300 ms: the server dies after 2-4 keystrokes.

  Fix: name it in the ballot as a prerequisite of A, B, C and D, and loop until the body is complete (A's frame buffer does this). Code work for card #6903.
- **F2 Seam shape and buffer.** Replace `input_ready()` with a timed byte read in D-CORE-TERMRAW1's shape that takes from the runtime's one buffer (D-TERM-INCREMENTAL1). For example, `core.term.take_within(n: Int, wait: Duration) -[IO, Time.Wait]> [U8]? IOError!`: None when no byte arrives in time, an empty list at end of input, "a wait of zero checks without waiting". Say why a descriptor poll is wrong: it misses buffered frames.
- **F3 Replay and platforms.** State that the timed read is journaled like every stdin read, None answers included. State that only the Linux pack has stdin today. Drop PeekNamedPipe, macOS, `jet dev` and the REPL.
- **F4 Order.** Replace "Edits among them are applied first" with this: a request is answered on the text that the messages before it left, and edits queued after it are applied after its answer. Cite the passing overlapping-edits case (p5) and the specification's Implementation Considerations and ContentModified texts.
- **F5 Publication claim.** Narrow it to this: before a request, the publishing step still checks a document with a queued newer edit (the request reads that check) but does not send those diagnostics. didOpen still publishes at once. An edit arriving during a check is seen at the next read. Show this in A's sketch.
- **F6 Cancel matching.** State the rule:
  - A cancel applies to the latest earlier unanswered request with the same id, compared as JSON (2 ≠ "2").
  - A cancel for an answered id, an unknown id or a notification is ignored.
  - A cancel never applies to `initialize` or `shutdown`, and the -32002/-32600 answers keep precedence.
  - No partial results or progress exist, so neither needs a rule.

  Say that the late-cancel case passes because its cancel precedes request 2.
- **F7 Case determinism and facts.** The costly case's three `send`s race with A. Say that the case must send the burst in one write. That is a case or harness change for card #6903. Correct the PENDING-SEAM reading.
- **F8 B at its strongest.** Rewrite B as "run the check before a request as a task; cancel it when that request is cancelled (D-CANCELMODEL1); nothing from it is stored or published". No driver change is needed (p6). Note that the checker keeps no state between checks (`JetCLILSPChecker.check` returns an `LSPChecked`), and that half-written driver files or caches are not audited.
- **F9 C at its strongest.** C is a reader task with today's blocking reads and needs no Core call (p6). Requests stay one at a time. Its costs are concurrency, replay of message timing, and stdin ownership. Drop "worker count of two", and make the whyNot a real cost.
- **F10 D at its strongest.** Quote the specification's `$/` ignore rule. Replace "stale answers" with wasted checks and latency (p4 lag numbers).
- **F11 Comparisons.** Fix the rust-analyzer row: immediate RequestCanceled from `req_queue`; salsa write-cancellation → ContentModified or retry. Fix tsserver: out-of-band pipe. Add clangd for id rules. Give sources for every row.
- **F12 Story.** Drop "an old outline can show up"; VS Code drops cancelled results. Say that today the server exits on such a file (F1).
- **F13 Why and gains.** Ground the why in p4: A catches cancels only when the server is behind, and a cancel meets a running request when the check time is close to the keystroke gap. Replace "no scheduling policy is chosen" with the stated policy and arrival order.
- **F14 Missing loss.** Diagnostics for an edit whose request was cancelled wait for the next non-cancelled request, as they do today after any edit not followed by a request.
- **F15 Line facts.** known-failing.tsv:358; the checkInstructions likewise.

## 4. Re-review log

- 2026-11-24, round 0: findings F1-F15, from the text as received (kept as `D-LSP-CANCEL1.json.before-review`).
- 2026-11-24, round 1. Per the assignment, the reviewer repaired the draft directly, and then re-checked each repair against code, probes and sources.
  - F1 closed in the text: a "defect every option fixes first" paragraph with p3/p4. Each option, D included, carries the fix. The code fix stays open on #6903.
  - F2 closed: `core.term.take_within(n: Int, wait: Duration) -[IO, Time.Wait]> [U8]? IOError!`, with D-CORE-TERMRAW1 and D-TERM-INCREMENTAL1 quoted and the buffer reason given. A's frame buffer "blocks nothing" on a partial message. I checked that `take_within` collides with no name in Core/Term/Term.jet.
  - F3 closed: the Replay paragraph and the A technical text. The Windows, macOS, `jet dev` and REPL claims are gone.
  - F4 closed: the "Order" paragraph cites p5 and the specification. A's detail says "Messages are still handled in arrival order".
  - F5 closed: the narrowed paragraph. Gain 2 now reads "Before a request, diagnostics are not sent for a document whose newer edit is already queued".
  - F6 closed: "Cancel matching" paragraph. A's technical text names the late-cancel order.
  - F7 closed in the text: the "case" paragraph and gain 1 ("once it sends its burst in one write"). The case change stays open on #6903.
  - F8 and F9 closed: B and C rewritten as task cancel, with p6, the stateless-checker note and the unaudited driver files. C's costs are concurrency, replay timing and stdin ownership. Their whyNot lines cite p4's 1-4 of 19 running cancels.
  - F10 closed: D quotes "is free to ignore" and "there is little a server can do". I checked the second quote against the 3.17 text after a first draft paraphrased it.
  - F11 closed: four sourced rows (Go, Rust, TypeScript, C++), each code line copied from the cited file. The Rust row now says unwinding happens "only when a change is applied to the database". The surface `trio.wild` shows the `req_queue` code.
  - F12 closed: the story now has the crash, the lag condition and VS Code dropping cancelled answers.
  - F13 closed: why and whyNot cite the p4 numbers from `outputs-tip18.txt`. A first draft quoted an earlier p4 run (15 of 19; 39 ms to 202 ms), and I corrected it to the recorded run (16 of 19; 39 ms to 340 ms). The detail adds the 1,000-line, 300 ms row, where 4 cancels met the running request and none a waiting one. The Scope paragraph now says the ballot states each option's scheduling.
  - F14 closed: third loss, with mitigation.
  - F15 closed: 358 in the detail and checkInstructions. The checkInstructions also name the take defect, the two ratified decisions and the overlapping-edits case.
  - Limit of this round: the reviewer wrote the repairs it re-reviewed. G2Editor, the author, should read `diff <(jq -S . D-LSP-CANCEL1.json.before-review) <(jq -S . D-LSP-CANCEL1.json)` before posting.
  - `Validate.jet` on the written ballot, run with the tip18 compiler:

    ```
    $JET_SCRATCH_ROOT/tmp/orch/ballots/D-LSP-CANCEL1.json (D-LSP-CANCEL1): 1 gap
      - posting payload must set draft:false
    ```

    The text as received had 8 gaps. They included "seam" jargon in four surface fields, an A technical paragraph of 128 words and a checkInstructions sentence of 37 words.

## 5. Overall verdict

Round 0: **fails** (15 findings). The recommendation (A) answers the right question with the right instinct: catch the cancels that are still queued, keep order, and stay single-threaded. But as written:
- Its seam misses the runtime buffer and replay, and contradicts the ratified timed-read shape.
- "Edits among them are applied first" would break a passing case and the specification.
- Its publication claim overreached.
- B and C were stated far below their strongest. The language's task cancel makes both cheaper than the ballot says, and C needs no Core call.
- The comparisons misstated rust-analyzer.
- It missed that the server dies today on any message body that arrives in pieces.

Round 1 (final): **holds within limits**. Every claim holds, or holds within these named limits:
- The short-body fix comes first under every option (F1; card #6903).
- The timed read must be journaled for prove and replay, and exists only on the Linux pack until other packs have stdin (F2, F3).
- The costly case must send its burst in one write to be deterministic (F7; card #6903).
- B and C carry an unaudited question: does a driver compile stopped mid-way leave files or caches half-written?
- A catches cancels only while the server is behind. When a check takes about as long as the gap between keystrokes, cancels meet the running request, and only B or C stop those (p4).
- The reviewer repaired the text it re-reviewed; the author should read the diff.

| Claim | Round 0 | Round 1 |
|---|---|---|
| C1 situation | holds within limits | holds |
| C2 code and case facts | fails | holds |
| C3 no non-waiting read | holds | holds |
| C4 LSP rule | holds within limits | holds |
| C5 publication | fails | holds within limits (didOpen and in-flight edits named) |
| C6 seam | fails | holds within limits (journaling, Linux pack) |
| C7 non-blocking drain | fails | holds (frame buffer) |
| C8 order | fails | holds (p5) |
| C9 cancel matching | fails | holds |
| C10 B | fails | holds within limits (driver unwind not audited) |
| C11 C | fails | holds within limits (replay timing not shown) |
| C12 D | holds within limits | holds |
| C13 comparisons | fails | holds |
| C14 story | fails | holds |
| C15 why | holds within limits | holds within limits (A helps only under backlog, stated) |
| C16 gains | fails | holds within limits (case needs one write) |
| C17 losses | fails | holds |
| C18 surface | holds within limits | holds |
| C19 coverage | holds | holds |

`reviewPasses.adversarial` is set in the ballot to the text below, which passes `Validate.jet` with only the draft gap left. The validator counts "The adversarial review attacked the recommendation." as one of the two sentences it allows after the model-family prefix, so one sentence of at most 32 words follows it.

> Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: LspCancelReview. The adversarial review attacked the recommendation. Probes p1-p6 ran on tip18; findings F1-F15 are fixed or named, and A holds within its limits: a short-body fix first (card #6903), a journaled timed read, a one-write case.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 | card | #6903 |
| F2 | no-action | archived: fixed in the ballot text before posting |
| F3 | no-action | archived: fixed in the ballot text before posting |
| F4 | no-action | archived: fixed in the ballot text before posting |
| F5 | no-action | archived: fixed in the ballot text before posting |
| F6 | no-action | archived: fixed in the ballot text before posting |
| F7 | card | #6903 |
| F8 | no-action | archived: fixed in the ballot text before posting |
| F9 | no-action | archived: fixed in the ballot text before posting |
| F10 | no-action | archived: fixed in the ballot text before posting |
| F11 | no-action | archived: fixed in the ballot text before posting |
| F12 | no-action | archived: fixed in the ballot text before posting |
| F13 | no-action | archived: fixed in the ballot text before posting |
| F14 | no-action | archived: fixed in the ballot text before posting |
| F15 | no-action | archived: fixed in the ballot text before posting |
<!-- /audit-dispositions -->
