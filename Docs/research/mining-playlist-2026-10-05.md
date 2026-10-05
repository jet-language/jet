# Mining run: 50-video playlist (2026-10-05)

This note is dated evidence. It records what a 2026-10-05 mining run over one
YouTube playlist found, how deep the reading went, and what it put on Tower.
It owns no plan or status: work lives on the Tower cards and ballots linked
below, and none of the findings proves how Jet behaves after that day.

- Playlist: <https://youtube.com/playlist?list=PLXgiJybdlzvk&si=NPuWg6a16_fUmzt8>
- Sources: 50 videos, 134,288 s (37.3 h), listed in `/mnt/jetscratch/mine/new.tsv`.
- Scratch record (not committed): batch reports `/mnt/jetscratch/mine/reports/*/report.md`,
  deep re-mines `/mnt/jetscratch/mine/v3/*/report.md`, distill output
  `/mnt/jetscratch/mine/distill/{POST-PLAN.json,FINAL-LEDGER.tsv,post-state.json}`,
  and the fresh-context review `/mnt/jetscratch/mine/distill/review/AUDIT.md`.

## Method

The run went through four stages.

1. **Batch mining.** Six batches, each mined by one worker under the
   `mine-for-jet-v2` skill. Every worker read each transcript in full and
   recorded claims (one concept per claim, with locator, lane, scale, Jet
   alignment and a proposed disposition). After Main passed on the owner's
   objection that the first pass was too thin, every batch did a second pass.
   For example, cpp-craft grew from 18 to 183 claims and learning-compilers
   from 29 to 285.
2. **Depth gate.** The v2 skill's checker (`check_coverage.mjs`) requires
   5-minute segments, a ledger of every concept read (about 1.4–2.4 items per
   minute in the passing runs), schema-valid claims, and a per-segment floor.
   All six first-pass batches failed it (`/mnt/jetscratch/mine/checks/batches.md`).
   The failures were mostly schema errors (enum values, missing `segment`,
   `scale`, `incremental_contribution`), and the first pass kept no concept
   ledger.
3. **v3 re-mine.** Twelve shards re-read their sources to the depth contract.
   Partway through, the owner ordered a stop on re-mining sources that had
   already been mined. Shards that had not passed by then left a `STATUS.md`,
   and their first-pass claims went to distillation after mechanical schema
   fixes.
4. **Distill and review.** Two distill workers (DistillLang and
   DistillSystems) split the 2,275 claims with no overlap and no gaps, and gave
   each claim one disposition. A fresh-context reviewer (MineReview) then
   audited the no-action reasons, attach notes, new cards and ballots, and
   wrote the final post plan. Main posted it: 62 Tower operations.

### What passed the depth gate

Ten sources passed: 15.7 h of the 37.3 h (42%). Every other source rests on
first-pass claims that were read in full but never ledgered against the gate.

| Shard | Source | Range | Ledger | Claims | Gate |
|---|---|---|---|---|---|
| H1-http-a | FknTw9bJsXM *From TCP to HTTP* | 0:00–2:20:00 | 302 | 51 | PASS |
| H2-http-b | FknTw9bJsXM | 2:20:00–4:38:30 | 235 | 60 | PASS |
| P1-python-a | 4M87qBgpafk *Python for Beginners* | 0:00–3:35:00 | 293 | 110 | PASS |
| P2-python-b | 4M87qBgpafk | 3:35:00–7:08:29 | 341 | 125 | PASS |
| S1-win32 | DaJWWePhRsM *Reviving Win32 C code* (+2 linked pages) | full | 313 | 82 | PASS |
| S2-systems-short | 7 short talks: vO5Ykk7O9r0, jF3W7EkXd4k, PbVHBToltXk, A2U_GMWl9l4, cKyrT8OVbDo, hD0fyLtWIIE, eP3DFpsWABc | full | ≈186 | 75 | PASS |

The shards that stopped without passing were A1, A2, B1 and B2 (languages),
C1 (craft and web), and L1 (learning). A1 and A2 had read all six language
interviews in full, and their `STATUS.md` files hold verified corrections that
the distill used, for example that the orphan rule is confirmed and that
Unison's comment-edit rehash does not affect Jet importers. S2 also has an
independent old-schema deep pass over all 14 systems-perf sources (206 claims,
the `systems-perf-deep` ledger rows).

### Claims by batch and final disposition

| Batch | Videos | Claims |
|---|---|---|
| languages-a (Nix, Gleam, Elixir, Roc, Gren, Unison) | 6 | 403 |
| languages-b (ReScript, Riot, actors, DreamBerd, CURSED, lambda calculus) | 6 | 243 |
| learning-compilers (+ v3-P1, v3-P2) | 9 | 285 + 110 + 125 |
| systems-perf (+ deep pass, v3-S1, v3-S2) | 14 | 319 + 206 + 82 + 75 |
| cpp-craft | 10 | 183 |
| web-data-net (+ v3-H1, v3-H2) | 5 | 133 + 51 + 60 |
| **Total** | **50** | **2,275** |

| Disposition | Claims |
|---|---|
| no action (confirmation, out of scope, or evidence gap with reason) | 1,707 |
| attach as a note on an existing or new card | 448 |
| new card | 94 |
| ballot | 26 |

Most claims are confirmations. For example, learning-compilers has 226 of its
285 claims as confirms-choice, and systems-perf pass 2 has 228 of 290. A v3
rule applied in S1, S2 and the H and P shards: a confirmation counts as a
claim only if it names a Jet mechanism and a witness. Otherwise it stays a
ledger row.

## Strongest findings by area

Locators are `video-id mm:ss` or the claim ID in the scratch claims. Unless a
row says "probe", the Jet side comes from reading source, not from running it.
Probes ran on release `jet dev-05ea86f65`.

### Names and text

| Finding | Evidence | Outcome |
|---|---|---|
| Identifier rule has holes. Hindi `नमस्ते` fails E0001 on U+094D, and NFD `café` fails on U+0301. Latin `a` and Cyrillic `а` bind two silent names. Hidden direction marks compile in comments and strings. | languages-b probes A, C, D, E. Lexers: `Scan.jet:47-55`, `Scan.rs:772`. DreamBerd and CURSED as prompts. | D-IDENT-CHARS1, #4623; note on #4426 (self-hosted lexer uses approximate `is_alphabetic`) |
| Plain strings cannot spell CR or a Unicode scalar. Core builds CR with `String.from_bytes([U8]{13})` in three places, and Debug prints `"a\r\u{1b}"`, which Jet refuses to read back. | FknTw9bJsXM throughout (CRLF); v3-H1 A12, v3-H2 B51 | D-STR-ESCAPE1, #4631 |
| Core text search returns Int −1 for "absent" while list `index_of` returns `None`. The course's 10:05–11:52 crash is this hazard. | v3-H1 A06; `Core/text/parse.jet:78, 627` | note on #3262 |

### Silent loss and beginner diagnostics

| Finding | Evidence | Outcome |
|---|---|---|
| Duplicate map-literal keys silently keep the last value, for written and computed keys, although Jet's JSON reader refuses repeated names. | 4M87qBgpafk 5:54–5:56; probes p02, p12; v3-P2 PYB-79 | D-MAPLIT-DUP1, #4626 |
| Exact division by zero stops at "line 0" with E3001 "invalid exact quotient", while `/%` reports E3010 at the right line. The list-position stop says "1 items", has no column, and gives a generic fix. | 18tFu7eLC5o 3:00–4:51; probes p04, p18, p19; v3-P2 PYB-15/16, probe p3 | #4627 |
| Diagnostics name the symptom, not the cause: print instead of return, a name bound inside ended branches, Int/Int into Int, qualified nested patterns, implicit multiplication, stray prose lines, and the E0303 hint that invents fields. | 4M87qBgpafk 1:13–1:16, 1:23, 3:01; probes p01, p10, p11, p14; web-data-net F9 | #4628 |
| A constant countdown with the wrong direction is silently empty. Equal bounds flip meaning: Python `range(5,5)` is empty, Jet `5..5` has one item. S19 states only positive strides. | 4M87qBgpafk 3:37; p17; PYA-110; PYB-3, PYB-61 | #4629 |
| `jet learn` has no zero-experience arc (3 repair katas, 4 advanced tasks). The course supplies an order, a misconception list, and 11 task seeds. | whole 4M87qBgpafk; v3-P2 drafts | #4630 |
| Core still ships alias names (`append`, `index`, `discard`, `take`, `remove`) against D-CORE-ONE-NAME1. `len(xs)` suggests `run`. Slices and list `+` get generic errors. | v3-P2 PYB-14, 20, 48, 49, 51, 99; probes p1, p2, p7–p9 | note on #4022 |
| Editor completion could teach the Jet name when a user types a foreign member name. ReScript removed its largest onboarding wall this way. | yKl2fSdnw7w; languages-b F4 | #4625 |
| Fields never read and enum cases never built get no warning (ReScript reanalyze and Rust `dead_code` catch them). | languages-b probe F | D-MEMBER-LIVE1, #4624 |

### Tasks and runtime

| Finding | Evidence | Outcome |
|---|---|---|
| Busy tasks ignore deadlines, cancellation and fairness. `task.race` drains a cancelled busy loser by blocking on it. Plugins already get Wasmtime epoch interruption, so ordinary compiled Jet is the only code a loop can hold forever. | RntfkL8lUY4 12:28–12:48 (Gleam); IxQ586TS8Gw (Riot); Go proposal 24543 (7.8% geomean loop-check cost), Wasmtime PR #3699, HotSpot loop strip mining; `TaskGroup.rs:485-498`, `Scheduler.rs:3956-3959` | D-TASK-SLICE1 on #4637 |
| The ratified strong pause (D-TASK-PAUSE-TIER1=E) is unbuilt: `pause(mode:)` is E0764 and no engine emits the back-edge check. | languages-b probe B | #4636 |
| `http.serve(addr, lambda)` and router serve run an older server: one unbounded thread per connection, no panic isolation, no shutdown, `exit(1)` on bind failure. Accept errors stop or spin the server. | v3-H2 B20, B21; `HTTPServer.rs:6898-6941`, `2280-2288` | #4635 (refs #3665, #3078) |
| Every stdout write flushes, even into files and pipes, so `stdout.flush()` does nothing. The cost is unmeasured. The native image runtime prints text and newline as two syscalls. | i_wDa2AS_8w 0:43; `Term.rs:152-177`; `Runtime.jet:289-308` | #4634; note on #4013 |

### HTTP and Core libraries

The TCP-to-HTTP course works as an acceptance oracle for the Core-in-Jet
HTTP/1 port. It drew 87 claims onto #3665.

| Finding | Evidence | Outcome |
|---|---|---|
| The parser needs a typed step result, `NeedMore(consumed) \| Done(consumed, msg) \| Error`, so a dropped consumed count cannot be represented. The course's 136:47 bug is that exact drop. | v3-H1 A49, A20, A31 | note on #3665 |
| Status-line deviations: RFC-valid `HTTP/1.1 200 ` and `404 Not  Found` are rejected, and the invalid `HTTP/1.1 200` is accepted. Four reason tables disagree, so a 503 can go out as `503 OK`. | FknTw9bJsXM 187:45–188:57, probe; v3-H2 B25 | note on #3665 |
| A bodiless request followed by more bytes is `InvalidFraming`, so keep-alive cannot work. This corrects the first pass. | v3-H1 A13; `http.jet:548-549` | note on #3665 |
| `crypto.Hasher` buffers all input until `digest()`. | v3-H2 B53; `crypto.jet:76-86` | note on #3667 |
| `parse_response` refuses non-UTF-8 bodies. | FknTw9bJsXM 272:24–275:20 | note on #4428 |
| Matching a payload-less `HTTPError` aborts default `jet run` with a JIT ICE. | web-data-net F4, probe | note on #4134 |
| A pure `parse_response` program needs `--allow=Net,Time.Wait`. | web-data-net F10, probe | note on #3418 |

### Games, layout and profiling

| Finding | Evidence | Outcome |
|---|---|---|
| The `core.game` ECS does not exist: the scene query returns a hard-coded row (`Game.rs:378-400`). The Unreal talk measures 22 trivial ticks at 44 µs, and aggregating 18 actors takes 2.8 ms down to 0.95 ms, which supports D-SYSSCHED1's whole-slice systems. | KxREK-DYu70 | note on #238 (66 claims); the #3645 note records that D-SYSSCHED1 had no linked build card |
| Game input takes Strings and a misspelled key returns `false` forever, against ratified typed input (D-GAME-INPUT1, D-GAME3). The window pump must not depend on drawing (raylib polls events only in `EndDrawing`). | eopps3YF6aE 14:20–17:04, 18:55–22:59 | note on #238 |
| Games cannot move between screens or scope assets to a screen. | eopps3YF6aE 7:07–17:39; Bevy, Godot, Unity, raylib | D-GAME-SCREENS1 on #238 |
| Jet's raylib bridge lacks `draw_line` and text measurement and takes only `Int` coordinates, so the stream's four-primitive overlay cannot be written against it. | v3-S1 DAJ-57; `Core/game/raylib.jet:34-106` | carried in the #238 note |
| Ordinary types cannot show their byte size: `T.$layout` is null for the default layout. | 7_o-YRxf_cc 22:00–29:00 | D-LAYOUT-SHOW1, #4633; note on #3142 |
| `jet perf` should prove itself on a game frame: wait attribution, inclusive time, recursion, scoped spans. `core.log` spans have no scope-bound form. | KxREK-DYu70 (WaitForTasks); v3-S1 DAJ-12, `Core/log/log.jet:75-79` | #4632 |

### Compiler and tooling

- Jai's `-perf` report (A2U_GMWl9l4) answers "why is my compile slow?" with
  phase times, every compile-time run, and a polymorphism report (515 calls,
  427 reused). It found a duplicated metaprogram pass. Note on #3961.
- ReScript checks each file in isolation behind interface cutoff
  (yKl2fSdnw7w 32:08–34:13). Note on #4607.
- Elixir targets the Erlang abstract format so existing tools keep working
  (IGmwiyines0 41:43–43:58). The lesson for Jet is to emit native debug line
  tables. Note on #4136.
- Wrong-case `#include`s compile on Windows and fail on Linux
  (DaJWWePhRsM 32:20–32:41). Evidence for the casing ballots on #4613.

## Tower outcome

Main posted the reviewed plan on 2026-10-05 (`post-state.json`, all 62
operations done). Every new card carries the tag `mine-2026-10-05`.

| Card | Kind | Title (short) | Ballot |
|---|---|---|---|
| #4623 | decide | Names in every script, one stored form, look-alike warnings | D-IDENT-CHARS1 |
| #4624 | decide | Unused-name warnings reach fields and enum cases | D-MEMBER-LIVE1 |
| #4625 | implement | Completion teaches the Jet name for a foreign member name | — |
| #4626 | decide | Map literals never lose an entry silently | D-MAPLIT-DUP1 |
| #4627 | implement | Division-by-zero and list-position stops name line, column and a working fix | — |
| #4628 | implement | Beginner mistakes get cause diagnostics | — |
| #4629 | implement | Constant ranges that can never run: advisory lint and stride law | — |
| #4630 | implement | `jet learn` zero-experience beginner arc | — |
| #4631 | decide | Plain strings cannot spell CR or a Unicode scalar | D-STR-ESCAPE1 |
| #4632 | implement | Prove `jet perf` on a game-frame workflow | — |
| #4633 | decide | Show each type's compiled representation in inspect and hover | D-LAYOUT-SHOW1 |
| #4634 | implement | Measure, then fix, the per-write stdout flush | — |
| #4635 | implement | Lambda and router `serve` bypass the supervised server | — |
| #4636 | implement | Build the ratified strong pause | — |
| #4637 | decide | Busy tasks honour cancellation, deadlines and fair turns at loop turns | D-TASK-SLICE1 |

The seven ballots are open. D-GAME-SCREENS1 sits on the existing card #238
rather than a new card.

| Ballot | Question | Recommended |
|---|---|---|
| D-IDENT-CHARS1 | Which characters a name may use, and how look-alikes are flagged | A: Unicode's standard name rule (UAX #31), one stored form (NFC), non-blocking look-alike warning |
| D-STR-ESCAPE1 | Carriage return and Unicode escapes in plain strings | A: add `\r` and `\u{…}` |
| D-MEMBER-LIVE1 | Do unused-name warnings reach fields and cases? | A: yes; encoding and foreign code count as use |
| D-MAPLIT-DUP1 | What a map literal does with two equal keys | A: error at check time for written keys, stop at the literal for computed collisions |
| D-LAYOUT-SHOW1 | Do tools show this build's byte sizes? | A: in inspect and hover, labeled "this build only"; the language promises nothing |
| D-TASK-SLICE1 | Do loop turns become stop points? | A: one cheap check per loop turn, on by default only after a speed gate shows no slowdown |
| D-GAME-SCREENS1 | How a game moves between screens | A: screens named in an enum, built on entry, freed on exit, plus one always-on shared scene |

Each ballot passed `validate.mjs`. Both full ballots (D-IDENT-CHARS1 and
D-STR-ESCAPE1) also had a beginner reader pass and two adversarial passes. The
D-IDENT-CHARS1 review reconciled it with D-CASE-LAW1 (combining marks would
otherwise fail E0357) and added invisible-character and math profiles.

The 40 attach notes went to 37 existing cards and 3 of the new cards. The
largest are #3665 (87 claims, compressed to 1,928 characters), #238 (66 claims),
#3961 (32), #4195 (23), #3027 (21) and #2279 (20).

Five ballots were already posted before the review and were excluded from the
plan: D-GAME-ENTITY1, D-AUTH-AMBIENT2, D-LINT-VISIBILITY3, D-CLI-ALIAS1 and
D-WIRE-MARKER1. The systems-perf batch had flagged D-GAME-ENTITY1 as planned
on #820 but never drafted. The review re-targeted one attach note to #3420 as
evidence for D-LINT-VISIBILITY3: the handler-order bug from the course
(4M87qBgpafk 6:59) shows only on `jet check`.

### What the review changed

- **No-action audit.** The reviewer drew one claim per stratum (seed 20261005,
  98 strata, 98 of 1,239 claims). 90 had a correct reason. 5 had the right
  outcome but a wrong reason; those reasons were rewritten, including two that
  cited a nonexistent #441. 3 were wrong and now attach to #3665. The fixes
  were applied to every row that shared the same reason.
- **Attach notes** went from 201 to 40. Of 153 automatic notes, 107 only
  confirmed done work and 43 had been matched to unrelated cards. For example,
  a C++ virtual-destructor claim had landed on #4567 (destructuring copies).
- **Cards.** Fifteen candidates became fifteen keys after one rejection, two
  merges and two splits. NC-DIVZERO and the P2 bounds-stop draft merged into
  #4627. The strong-pause build (#4636) was split from the D-TASK-SLICE1 host
  (#4637), so ratified work does not wait on an open vote.

## Confirmations of Jet choices

These were recorded with a mechanism so they need not be mined again. They
are not new work.

- **Beginner traps already designed away** (4M87qBgpafk): inclusive ranges
  with the E0364 teacher, written signature types (the instructor misjudges
  which untyped parameters are Booleans, PYA-103), Bool-only conditions,
  file-wide declarations, `debug()` to stderr rejected in release, the casing
  law, exact division, typed lists, visible `&` writes, optional lookups, and
  no catch-all handler.
- **Value-returning HTTP handlers** (FknTw9bJsXM second half): the presenter
  forgets the final CRLF, Content-Length or Content-Type five times in about
  70 minutes, and a returned `HTTPResponse` makes that impossible. In the first
  half, 19 confirmations cover Optional lookups, the result rail, exhaustive
  enums, generators that cancel on drop, and an empty read meaning clean EOF.
- **Language design** (languages-a/b): the orphan rule; explicit
  metaprogramming opt-in (D-META-OPTIN2), which is stricter than Elixir's
  `require`; comment edits that do not invalidate importers (Unison
  43:41–44:10, early cutoff); the supervised service plane (D-SERVICE1,
  delivery, snapshots, generation handoff instead of hot reload); `#Context`
  providers instead of full effect handlers; and no async colouring.
- **Systems** (systems-perf, S1, S2): atomics without memory-order arguments
  (Blow, D-ATOMIC-WIDTH1), inline assembly that names parameters, DCE-safe
  benchmarks (D-BENCH-KEEP1), declaration-driven reflection, typed time
  units, no textual macros or headers, the debug-map mismatch error E2235
  (D-DBG-DIAG1), and static no-libc Linux binaries (D-OS-FLOOR1).
- **C++ habits** (cpp-craft): no cast syntax, left-to-right argument
  evaluation, defined-not-declared functions, wildcard imports rejected, named
  tuple returns, one `equal` hook backing `!=`, and generated C++ shims.

## Rejected ideas

| Idea | Source | Reason |
|---|---|---|
| Research card for networked game state (N-NETGAME) | KxREK-DYu70 | Premature: the ECS it would replicate is unbuilt, and one Unreal talk is the only evidence. Kept as a lead line in the #238 note. |
| Implicit remote calls so files skip import lines | IGmwiyines0 38:25–39:05 | Conflicts with the owner's manual named imports (D-NAME-FILES1=C). |
| Multimethods | IGmwiyines0 (E23) | Its own author's conflict analysis refutes it; Jet uses traits with no global open dispatch. |
| Shipping a suspended computation to another node | zHzpoVgqgc4 | Conflicts with the authority and sendability laws. |
| Dictionary pattern matching | languages-b | A second decode path beside `decode<T>()`. |
| Emoji names, keyword skins or localization | DreamBerd, CURSED | One spelling; C++ P1949 and D-EXT-EMOJI1. |
| Core TOON codec | nTMP_rLZOYM | Owner cut of AI/LLM Core (#3955). |
| C++-style memory-order model; per-entity `tick` API | Blow; KxREK-DYu70 | Blow and D-ATOMIC-WIDTH1 reject the first; the Unreal measurements argue against the second. |
| Padding or large-variant warning lint | 7_o-YRxf_cc | Clippy and Go document that this advice often misleads, so D-LAYOUT-SHOW1 only informs. |
| AI tutor and solution-peek penalties in `jet learn` | 4M87qBgpafk | Out of scope; D-LEARN-FEEDBACK1 chose no penalty. |
| Stepped or open-ended slice syntax | 4M87qBgpafk | Only the teaching target is in scope. |
| HTTP/3, server push, reverse-proxy helper | FknTw9bJsXM | No demand in the source; server push is an anti-lesson; no owner wants a proxy yet. |
| A ballot for PYB-114 (foreign-teaching supersession) | v3-P2 | Not an owner choice by the clarity rule; it is a spec edit plus a criterion on #4022. |
| Per-request memory budget on `http.serve` | DzhIprQan68 (Roc NIA) | Kept as an evidence gap: response-body escape, streaming and cancellation need research before any ballot. |

## Limits

- **Depth.** Only 10 of 50 sources (42% of the hours) passed the depth gate.
  The other 40 rest on first-pass claims. Those were read in full, but they
  have no ledger, so missed concepts cannot be counted.
- **Transcripts.** Most sources used auto-captions, and no frames were pulled,
  so on-screen code is known only from narration. For two sources, every
  caption download failed with HTTP 429, so they were transcribed locally with
  whisper (`ggml-base.en`, uncorrected): HfRQjix9tU4 and A2U_GMWl9l4. The
  latter passed the depth gate on that whisper text. Rolling captions in two
  languages-b videos put locators within about ±5 s.
- **Jet-state digest.** `jet-state.mjs` exited 2 (INCONSISTENT) for almost
  every batch and shard, because concurrent agents changed worktree identity
  during collection. The miners read code at dev `91a438ce4` or `3f64b4fb6` on
  master `3e5349196`. Only the web-data-net digest was consistent. The review
  refreshed the digest afterwards (HEAD `3e5349196`, Tower revision 74871,
  consistent). Three in-flight changes could stale its citations: worktrees
  editing `Core/http/http.jet` (the #3665 note's line numbers), build-overlay
  editing `Sema/Expressions/Literals.jet` (cited by #4626), and the casing
  proposal.
- **Recall audit.** The review ran no recall audit (`check_coverage.mjs
  --sample`) on v3-P2; only its claims were dispositioned. The H1, H2, P1 and
  S1 reports also list their own recall audits as pending, and AUDIT.md does
  not record one for them.
- **Audit denominator.** The no-action sample was drawn from 1,239 claims, but
  the final ledger has 1,707 no-action rows. The difference matches the
  review's conversion of 150 automatic attach notes to no-action
  [INFERENCE: the sample predates that conversion].
- **Execution.** Exercised behavior comes from probes on one release binary
  (`dev-05ea86f65`), mostly on the default `jet run` tier. Everything else is
  source reading at the identities above.
