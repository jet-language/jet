# Mine for Jet: owner playlist plus the Linear sync-engine video (2026-09-28)

## Verdict

Jet already ratified most of what these 39 videos ask for. It infers
effects. It makes mutation visible at the call site. Contracts sit on
signatures. `#CLI` parsing works with no registration step. It has live
queries, CRDT sync, and a compile-time `@` phase. The playlist's lesson is
execution, not new ideas. Live probes found a security hole, broken flagship
examples, and signatures that say less than the compiler knows:

1. **A script with no package.jet can spawn any process** without asking,
   because the ambient floor grants the whole `Exec` root so that `argv()`
   works (#3697).
2. **The sync-engine foundation does not run.** The shipped live-query example
   fails to compile. A two-line `app.live` program is an internal compiler
   error. Core/app takes footprints and data as strings, not the typed
   query functions D-LIVEQUERY1 ratified (#3700). `app` and `core.web` export
   the same names with different behavior (#3701).
3. **Signatures claim less, or more, than is true.** A pure function still
   carries the default `Err` route, though D-FAILURE-FOUNDATION1 says sema
   removes it (#3708). A `#Pre` condition may print (#3699). Hover shows only
   written effect rows (#3710).

The owner's four notes became three ballots and one planning card. The owner
ratified D-SCRIPT-CONFIRM1=A (#3705) and D-MARKER-MODULE1=A (#3707) the same
day. D-HONEST-SIG1 (#3710) is open, and the sync-engine design sits on #3709.

## Scope, sources, and capture

- **Primary source:** the owner's playlist
  `https://youtube.com/playlist?list=PLXgiJybdlzvk`. `yt-dlp --flat-playlist`
  on 2026-09-28 listed 38 videos, about 22 hours in total. **Linked source in
  scope:** `https://youtu.be/pRf8_40EDtM`, which the owner described as a Convex
  video. It is Tom Delalande's "How to beat a monopoly - Linear Case Study".
  Convex's own runtime rules were read from
  `https://docs.convex.dev/functions/runtimes` so the owner's Convex request
  had a primary source.
- **Registry:** `check_sources.py` reported all 39 video IDs `new` against
  `Docs/spec/reference/prior-art.md`. The checker cannot read a playlist URL,
  so each video was checked by ID. One clip, `0nwZ2jdFrMg`, is cited in
  `Docs/research/jai-vs-jet-syntax-briefing-2026-09-16.html`, but the registry
  did not track it. Its original interview is `1blhmslxkWg`. All 39 videos, the
  playlist, and the Convex and Deno pages are now in the prior-art source
  index, and `check_sources.py --verify-tracked` reports all of them tracked.
- **Captions:** every video had only YouTube auto-captions (`en-orig`); no
  creator subtitles. Names, code, and jargon are uncertain. Any claim that
  depends on exact spelling is marked medium or low confidence. Comments were
  not captured: the outcome asked for ideas, not audience reception.
- **Review:** nine read-only reviewers read every transcript in full. Their 707
  timestamped claims were then screened against Jet. I read the Linear
  video and the Jonathan Blow compile-time clip (`R6rH8IGtrTI`) myself; that
  clip's reviewer returned a summary but no claims. The ledger of 47
  recommendation-bearing claims was validated against the skill's enums.
- **Jet state:** `target/debug/jet` built 2026-09-28 13:07 from `d4bb652df`
  with 127 dirty paths from concurrent work. Core/http, Core/app, Core/web,
  and Core/process were unmodified. Probes ran in
  `~/.cache/jet-mine-pl-probe`, outside the two scratch roots. Both roots
  hold stray `package.jet` files from other tasks, which would have changed the
  authority result.
- **The honest/dishonest video is Logan Smith's "How to write the perfect
  function"** (`https://www.youtube.com/watch?v=2OMRWPOSw9s`). It was already in
  the prior-art index under Functions and API design. The first version of this
  report wrongly said it was not found: I searched only the playlist and the
  web, not the registry. Its auto-captions were read for 10:01-26:00.
- **No question tool.** The owner asked for questions through the omp question
  tool. This session had no such tool, so the owner-only choices became Tower
  ballots.

## Verified defects (live contrasts)

| # | Contrast | Card |
|---|---|---|
| D1 | `jet run spawn.jet </dev/null` with no package.jet runs `process.cmd(["touch","spawned.txt"]).run()`, prints `spawned code=0`, and creates the file. The same setup refuses `files.read` with E1803 (`granted_effects=Exec, IO, Mem.Alloc`). | #3697 |
| D2 | `--allow=FS` runs a `files.read` script and prints 32. `--allow=FS.Read` fails E1803 with `undecided_effects=FS`, because the read requires both the root and the leaf. | #3698 |
| D3 | `#Pre(noisy(n), ...)`, where `noisy` prints, passes `jet check` and prints `side effect` at run time. D-PREPOST1 says conditions are pure. | #3699 |
| D4 | `Examples/features/tooling/app_live.jet` fails with two E2402 errors. `use app; app.live("users","[alice]"); app.live_show(..)` hits an internal compiler error (exit 101, Cranelift MIR operand mismatch). | #3700 |
| D5 | `web.live_show` returns `LiveQuery(id=1,...)` from a runtime route, but its Jet body says `"{key}@{generation}"`. `web.invalidate` hashes the key while `app.invalidate` returns Unsupported. Pure `core.web` calls also require `Browser` authority. | #3701 |
| D6 | `#Get` from an imported file works, but L0103 calls the import unused. Removing it gives E0927. `#routes.Get` is E0927, reported twice. A `pub marker` error is drawn against the importing file's text. | #3702 |
| D7 | A Core `#Error` value cannot be printed three ways (E0302, E0112, E0915). E0915's fix suggests the spelling that fails as E0112. Every message shows `<corelib>/Core/app::Core/app/app.jet::AppError`. | #3703 |
| D8 | `jet inspect expand --facts callable-signature` shows `twice ... effects=[] errors=[Err] failure=Int (implicit default !Err)` for `n * 2`. | #3708 |
| D9 | `http_serve_entry.jet` and `http_routes.jet` fail with E0104 inside `Core/http/http.jet:457`, a 2-argument call to a 4-parameter `server.serve`. The serve example also wraps a blocking call in `#Unsafe`. | #3065 log |
| D10 | Six browser examples print API names ("This example only documents the API") instead of calling the API. | #3704 |

Papercuts logged: E0113's fix for `n / 2` says only "Use Int here". E0107
suggests `use core as core`. The E1803 Why line and prompt show raw facts and
a JSON array.

## Owner notes, answered

### FastAPI-style nested markers (D-MARKER-MODULE1, #3707, full ballot)

The source evidence conflicts. FastAPI (`v6G_JJK01zU` 0:02:53, 0:05:10) puts
`@app.get` beside each handler and builds docs from type hints. Blow's Jai
console (`R6rH8IGtrTI` 0:06:12) wins the same way: tag a function and it
registers itself, with typed argument parsing. The Axum tutorial
(`iPNEhKMIqNg` 0:31:53) disagrees and praises a router that is "simply a
value", with no decorator step. Both frameworks reject bad path input before
the handler runs. Jet already does this: `mux.get("/orders/:id", (id: Int) ...)`
in `Examples/features/web/battery/run.jet:135`, checked in sema.

Jet has the pieces. D-META-USER1 lets packages declare markers with bodies,
and `#Job` already turns marked functions into subcommands. The missing piece
is naming: only bare imported markers resolve, `#web.Get` is E0927, and
`pub marker` does not parse. The adversarial reviewer showed that marker naming
and HTTP route exposure are separate choices. A package-wide route scan could
also expose private or dependency handlers. So the ballot now asks only how
library markers are named. Its recommendation: always qualified (`#web.Get`),
so a bare marker is always a language rule. The exposure question comes next,
as a second ballot planned on #3707. Its options include explicit route
collection, a `#HTTP` root struct modeled on `#CLI`, and value routes only.

### Allow by default with one confirmation (D-SCRIPT-CONFIRM1, #3705, short ballot)

Today a script with no package.jet gets a tiny floor. In a terminal it is
asked `authority required for ["FS","FS.Read"] — choose once, project, or deny`,
and `project` fails without a manifest. Without a terminal it stops with
E1803. Sema knows every reached authority before the program runs. The ballot
recommends one upfront list with call sites, where Enter runs the script and
`always` writes the grant into the script's own package header. Without a
terminal the run stops with the exact `--allow` command. The "allow silently
and report" reading is option B. It loses because a downloaded script would
read files before anyone saw the list. Deno's docs make the same point about
subprocess permission (`https://docs.deno.com/runtime/fundamentals/security/`).
D1 must be fixed whichever option wins: an ambient floor that can spawn `sh`
makes any prompt meaningless.

### A Convex-grade sync engine (#3709, planning; blocked by #3700 and #3701)

Linear's engine (`pRf8_40EDtM` 4:12-8:26) keeps the whole workspace local. One
observable state serves both local and remote changes. Writes become
transactions that the server accepts or rejects, and the client rolls back on
reject. Scale forced lazy loading and partial bootstrap. Sync can be switched
off to prototype locally. It costs several full-time engineers, and there is
no plug-and-play equivalent. Convex requires queries and mutations to be
deterministic, with seeded randomness and a frozen clock per call, so it can
retry them. Only actions touch the network.

Jet's advantage is structural: effect rows can prove these kinds at compile
time. A query is `-[DB.Read]>` (D-EFFDBREAD1 shipped this proof). A mutation
is a `#Transact` block with no irreversible effect (E0746). An action is
anything else. The same Jet validation code runs on client and server (I9).
Convex enforces its rules at run time in JavaScript and cannot adopt this
without a typed effect system. The ratified laws cover most of the mechanism:
D-LIVEQUERY1, D-SYNC1, D-DBPOLICY1, D-DX-QUERY1 optimistic rollback,
D-DX-SERVERFN1, and D-QUERY-LIVE1. The implementation does not (D4, D5). The
card sequences the repair first, then the design. Its ballots cover the owner
choices: a frozen clock and seeded rng versus explicit Clock/Rng injection,
the conflict policy, and client persistence.

### Honest versus deterministic functions (D-HONEST-SIG1, #3710, short ballot)

Source: Logan Smith, `2OMRWPOSw9s` (auto-captions):

- 10:49: a function is honest when it touches the outside world only through
  its signature. Mutating memory the caller explicitly hands over stays honest.
  A function like `get_time`, which reads outside state its signature does not
  show, is dishonest.
- 12:25: dishonesty is infectious. Any caller of a dishonest function becomes
  dishonest, so honest functions sit at the leaves of the call tree.
- 18:20 and 19:31: keep the business logic honest, and put the dishonest
  "skin" at the top of the program.
- 22:43-25:05: a pseudo-random generator is honest and deterministic given
  its state; only seeding from the clock is dishonest. Pass a seeded generator
  to the function instead of reading a global one, and the whole system
  becomes reproducible.

Supporting playlist evidence: the Axum tutorial (15:57, "a function that can
fail says so in its return type out in the open") and the Google C++ pointer
video (4:25, code must "tell a story" at the call site). Blow's podcast
(1:08:53) calls a program a mathematical statement that includes its
non-determinism.

Smith's taxonomy maps onto Jet exactly:

- An empty effect row with `&` parameters is his honest function.
- An effect row names the dishonesty.
- Effect inference already spreads dishonesty to callers, as he says it must.
- D-DET1's injectable `Rng` and `Clock` are his seeded-generator fix.

Jet is honest in some places already: E0202 forces `&c` at the call site. The
gaps are D3, D8, and hover showing written rows only
(`crates/jet-semindex/src/Build.rs`, lines 2863-2877). The ballot, now updated
to cite Smith, recommends naming the principle. Every signature view would
then show effects, failure, and an honest, deterministic, or dishonest label
computed from the existing row walker. Beginners write nothing new.

## Jet alignment by category

| Area | Evidence | Jet state |
|---|---|---|
| Call-site mutation | `-gPpUIUMrwg` 2:46-6:03 | Shipped: E0202 requires `&c` (probe). |
| Honest failure in types | `iPNEhKMIqNg` 15:57 | Shipped typed `!E`; default `Err` not removed (#3708). |
| Contracts on declarations | `oitYvDe4nps` 14:43 | Shipped `#Pre/#Post`; purity unenforced (#3699). |
| Contract observe mode | `oitYvDe4nps` 15:52, 40:16 | Rejected: always-enforce plus proof erasure is safer (D-PREPOST1, D-FAIL-TIER1). |
| Visible compile-time phase | `e6crOMC9WCE` 22:43-27:40 | Shipped: `@` mark and `@if`. |
| User-level typed rewrites | `0nwZ2jdFrMg` 4:55-8:46 | Ratified: D-DX5-HOOK1 proposed edits; `jet inspect codemod`. |
| Module parameters | `ccV9aae8DIc` 1:40 | Ratified: D-CONF-MODULE1. |
| Copy cost visible | `5SntrSo8VMw` 9:38 | Shipped: `~` deep copy; `jet inspect audit copies`. |
| Deterministic map order | `e6crOMC9WCE` 32:02 | Shipped: three runs gave identical key order (probe). |
| CLI help and did-you-mean | `Cqd4tMX3yxI` 7:21, 23:12 | Shipped: `#CLI` struct gives help, suggestion, exit 2 with no registration (probe). |
| Hot reload | `v6G_JJK01zU` 8:29 | Shipped: `jet dev` swap (D-LIVE1). |
| OpenAPI from routes | `v6G_JJK01zU` 6:53 | Shipped export `web.openapi(router)`; interactive docs UI not checked. |
| Undo history | `YTe-cpDgyKs` 3:23-2:07:47 | Ratified store history (D-DX-STORE1). Tsoding's cursor, truncation, grouping, and navigation-without-snapshot rules are the acceptance bar. |
| Typed flag sets | `i-h95QIGchY` 18:22-20:36; `z7wVUfnm7M0` 9:49 | Gap: no flag-set type (#3711). |
| Secret redaction defaults | `29k3eay4Lr4` 6:38-7:12 | Shipped: `#Credential` taint and capture consent. |

## Avoid list

- **Implicit struct field promotion** (Jai `using`, `YTe-cpDgyKs` 1:17:33,
  with its own name collision at 1:19:20): it hides where a name comes from,
  against the reading-first slate. S62 `impl Trait using field` already covers
  forwarding.
- **Observe-and-continue contracts in production** (`oitYvDe4nps` 24:50 shows
  the undefined-behavior hazard of split checks in observe mode).
- **Unrestricted compile-time execution** (`0nwZ2jdFrMg` 9:55;
  `e6crOMC9WCE` 4:24): Jet's tiered, recorded compile-time authority is the
  better trade for auditability.
- **Silent allow-all for scripts** (option B of D-SCRIPT-CONFIRM1): Deno's
  docs show why subprocess access equals full access.
- **Print-only examples as proof** (D10).

## Beat vectors

- **Compile-time proven sync kinds.** Query, mutation, and action checked by
  effect rows, where Convex checks at run time. Ratified, but unbuilt at the
  typed surface (#3700, #3709).
- **One upfront authority list.** Sema knows the whole set before running;
  Deno asks per call at run time. Partly shipped: the prompt exists, the UX
  is balloted (#3705).
- **Signatures that state determinism.** The `#Replayable` row walker already
  exists; showing it is balloted (#3710).
- **Mutation visible at the call site** without pointer syntax. Shipped (E0202).

## Agent-optimality

D6 and D7 hurt repair determinism most: following L0103's fix breaks the
build, and E0915's fix points at another failing spelling. D8 produces
unrepairable-looking E0403 errors inside Core, far from the user's edit.
D-HONEST-SIG1 option A would improve context economy for agents, because a
signature's effects and failures appear without reading the body.

## Evidence gaps and what was not done

- Comments and audience reception were not captured.
- The terminal prompt was observed in a pseudo-terminal, but answering it was
  not tested (scripted input did not reach it).
- Ordering of parallel results (`para_map`), missing-generation diagnostics,
  and the interactive OpenAPI UI were not probed.
- Long talks on math (gingerBill), low-level representation (Reece), and
  Casey's history of Clean Code yielded technique and process guidance, not
  Jet surface gaps. Their claims stay in the reviewers' extraction only;
  outside the flag-set card, no card was warranted.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 ambient Exec spawn | card | #3697 |
| F2 leaf grant cannot cover a read | card | #3698 |
| F3 effectful contract conditions | card | #3699 |
| F4 live queries broken and string-keyed | card | #3700 |
| F5 app and core.web duplicate surface | card | #3701 |
| F6 imported marker diagnostics | card | #3702 |
| F7 Core error values unprintable | card | #3703 |
| F8 print-only examples | card | #3704 |
| F9 script authority confirmation | decision | D-SCRIPT-CONFIRM1=A |
| F10 module-owned markers and HTTP routes | decision | D-MARKER-MODULE1=A |
| F11 default Err route not removed | card | #3708 |
| F12 sync engine design | card | #3709 |
| F13 honest signatures principle | card | #3710 |
| F14 typed flag sets | card | #3711 |
| F15 HTTP serve examples fail inside Core | card | #3065 |
| F16 struct field promotion | no-action | rejected: conflicts with the reading-first priority; S62 covers trait forwarding |
| F17 contract observe mode | no-action | rejected: weakens D-PREPOST1 always-enforce safety |
<!-- /audit-dispositions -->

## Links

- Probes: `~/.cache/jet-mine-pl-probe/` (removed at close; each contrast is
  quoted above and on its card).
- Jet code cited: `Core/app/app.jet`, `Core/web/web.jet`,
  `Core/process/process.jet`, `Core/http/server.jet`, `Core/http/http.jet`,
  `Source/CmdCompile.rs`, `crates/jet-semindex/src/Build.rs`,
  `crates/jet-codegen/src/Prelude/Markers.jet`.
- Primary sources: every `https://www.youtube.com/watch?v=<id>` listed in the
  coverage table below; `https://docs.convex.dev/functions/runtimes`;
  `https://docs.deno.com/runtime/fundamentals/security/`.

## Coverage

| video | title (channel) | length | claims | Jet takeaway |
|---|---|---|---|---|
| [-gPpUIUMrwg](https://www.youtube.com/watch?v=-gPpUIUMrwg) | Why Google C++ Guide Recommends Pointers Over References (Suboptimal Engineer) | 0:07:29 | 10 | Mutation must show at the call site. Jet ships E0202 (`&c`); honesty evidence for #3710. |
| [0nwZ2jdFrMg](https://www.youtube.com/watch?v=0nwZ2jdFrMg) | Jonathan Blow on Jai Release Plans and Compile-Time Code Rewriting (Jonathan Blow Clips) | 0:19:56 | 17 | Typed compile-time rewriting migrated 300k lines. Jet: D-DX5-HOOK1 proposed edits, `jet inspect codemod`. |
| [29k3eay4Lr4](https://www.youtube.com/watch?v=29k3eay4Lr4) | Man, I love Java... (ForrestKnight) | 0:10:28 | 17 | Safe defaults without code change (compact headers, redaction, hybrid TLS). Jet: redaction shipped. |
| [2jIQJ2GuCMU](https://www.youtube.com/watch?v=2jIQJ2GuCMU) | Jonathan Blow on Why Modern Software is So Slow (Jonathan Blow Clips) | 0:15:19 | 8 | Fast purpose-built editors; accepted latency is too high. DX latency principle. |
| [5SntrSo8VMw](https://www.youtube.com/watch?v=5SntrSo8VMw) | The Problem with Clones in Rust - Why Functional Rust is Slower Than You Think (And How to Fix It) (HAMY LABS) | 0:13:49 | 24 | Cheap and deep clones share one spelling in Rust. Jet: `~` deep copy is explicit, and copies are audited. |
| [8-VZoXn8f9U](https://www.youtube.com/watch?v=8-VZoXn8f9U) | C++ Super Optimization: 1000X Faster (Dave's Garage) | 0:15:33 | 15 | constexpr folds work to compile time (1000x). Jet: value-level `@` comptime. |
| [ApmD4IQP-Ac](https://www.youtube.com/watch?v=ApmD4IQP-Ac) | Jonathan Blow on LSP (Jonathan Blow Clips) | 0:16:08 | 15 | GC is a spectrum; LSP overhead; sandboxed plugin bytecode. Jet: scoped GC opt-in, WASM plugins. |
| [Cqd4tMX3yxI](https://www.youtube.com/watch?v=Cqd4tMX3yxI) | Command Line Parsing with System.CommandLine (IAmTimCorey) | 0:25:58 | 30 | Typed options, generated help, did-you-mean. Jet `#CLI` already does this with no registration. |
| [FdshdE-5D1U](https://www.youtube.com/watch?v=FdshdE-5D1U) | New in Python 3.13: TypeIs (Indently) | 0:06:18 | 11 | TypeIs narrows both branches. Jet pattern tests narrow; user predicates not checked. |
| [GIt0b-95Fr4](https://www.youtube.com/watch?v=GIt0b-95Fr4) | Why Go Is Beautiful (Low Level) | 0:00:30 | 2 | Go moves an escaping local to the heap. Jet: ownership rules make the case explicit. |
| [HoPAlzaj3Tc](https://www.youtube.com/watch?v=HoPAlzaj3Tc) | 3 Great NEW Features Coming in Python 3.15 (Indently) | 0:06:49 | 9 | Lazy imports, starred unpacking in comprehensions, frozendict. Jet: compiled imports; `::` bindings are immutable. |
| [KBny6MZJR64](https://www.youtube.com/watch?v=KBny6MZJR64) | CONSTANTS in C++ (cazz) | 0:08:30 | 8 | const, constexpr, consteval, constinit. Jet: one `@` compile-time mark. |
| [N2ZTVAavNqY](https://www.youtube.com/watch?v=N2ZTVAavNqY) | How The C++ STL Works Under The Hood (Carter) | 0:06:10 | 11 | How the STL uses templates and type erasure. Background only. |
| [R6rH8IGtrTI](https://www.youtube.com/watch?v=R6rH8IGtrTI) | Jonathan Blow on Running Code at Compile Time (Jonathan Blow Clips) | 0:16:40 | 3 | Tagged functions self-register with typed args; sorted registration; generation placeholders. -> #3707. |
| [UUwDyPpQ3n4](https://www.youtube.com/watch?v=UUwDyPpQ3n4) | 16 C++ Habits That Make Your Code Better (Salar) | 0:09:05 | 18 | Prevent silent conversions, hidden ownership, ignored results. Jet: `#MustUse`, no implicit narrowing. |
| [UzD_Ze6zFKA](https://www.youtube.com/watch?v=UzD_Ze6zFKA) | Why Debuggers Frontends Are So Bad (TheStandupPodClips) | 0:15:25 | 9 | Debugger frontends must show dense editable state fast. Jet DAP shipped; no new gap. |
| [X40rcpLfMdY](https://www.youtube.com/watch?v=X40rcpLfMdY) | C Has 5 Integer Types But Zig Has 65,535! #clanguage #coding #programming (Code Guild) | 0:00:35 | 3 | Zig has arbitrary-width integers. Noted on #3711. |
| [XpvUiL0B42c](https://www.youtube.com/watch?v=XpvUiL0B42c) | Python's Biggest Bottleneck Is Finally Getting Fixed (Coding Together) | 0:01:17 | 5 | The GIL serializes Python threads. Jet: no GIL; nothing to take. |
| [YNtoDGS4uak](https://www.youtube.com/watch?v=YNtoDGS4uak) | gingerBill – Tools of the Trade – BSC 2025 (Better Software Conference) | 1:13:34 | 46 | Math as a craft tool: integrators, matrices as graphs. Library and teaching material; no surface gap. |
| [YTe-cpDgyKs](https://www.youtube.com/watch?v=YTe-cpDgyKs) | Reusable Undo System - Tsoding (Tsoding Daily) | 2:09:05 | 49 | Cursor undo, truncate on new edit, grouping, no snapshot for navigation. Acceptance bar for the store history. |
| [YrnAAp_Z16I](https://www.youtube.com/watch?v=YrnAAp_Z16I) | Modern C++ is a Mess (And I Love It) (ByteClocker) | 0:03:52 | 10 | Plain loops for readability, parallel policies and lazy ranges for speed. Jet: iterator and parallel laws already ratified. |
| [Zmx0Ou5TNJs](https://www.youtube.com/watch?v=Zmx0Ou5TNJs) | Nesting "If Statements" Is Bad. Do This Instead. (Flutter Mapp) | 0:01:00 | 3 | Guard clauses instead of nested ifs. Jet: `?? return`, `if c -> return`. |
| [ccV9aae8DIc](https://www.youtube.com/watch?v=ccV9aae8DIc) | Jonathan Blow on Solving Dependency Hell and Memory Management in Jai (Jonathan Blow Clips) | 0:15:37 | 15 | Module parameters with beginner defaults; replaceable allocator hooks. Jet: D-CONF-MODULE1. |
| [e6crOMC9WCE](https://www.youtube.com/watch?v=e6crOMC9WCE) | Jonathan Blow on Programming Language Design (Software Unscripted Podcast) | 1:41:43 | 109 | Explicit compile-time `if`, determinism as a spectrum, content-hashed vendoring, program as math. -> #3710. |
| [fbLfaFH_R_Q](https://www.youtube.com/watch?v=fbLfaFH_R_Q) | Python TypeVar and Generic Types explained (Just a Dev) | 0:20:06 | 14 | Generics relate input and output types; Python hints are advisory. Jet: generics are checked. |
| [gQwe1Fq_MQA](https://www.youtube.com/watch?v=gQwe1Fq_MQA) | Protected Module Members: An Interesting Feature in Python (Indently) | 0:06:05 | 6 | Underscore hides module names from star imports. Jet: `pub` is explicit; nothing to take. |
| [hMvWIYmdxNQ](https://www.youtube.com/watch?v=hMvWIYmdxNQ) | Jonathan Blow on Java Script and Web Assembly (Jonathan Blow highlights) | 0:02:24 | 6 | Replace JavaScript with WebAssembly; the DOM gap; 64-bit wasm cost. Jet web target law exists. |
| [i-h95QIGchY](https://www.youtube.com/watch?v=i-h95QIGchY) | Andrew Reece – Assuming as Much as Possible – BSC 2025 (Better Software Conference) | 2:44:08 | 184 | Explicit assumptions, arenas, packed flags, exponential arrays. Flag sets -> #3711; the rest is technique. |
| [iPNEhKMIqNg](https://www.youtube.com/watch?v=iPNEhKMIqNg) | Learn Axum in 2026: Rust REST API for Beginners (Codynn) | 1:27:27 | 51 | Router as a value, typed extractors, fallible types in the open. Counter-evidence on #3707; honesty evidence on #3710. |
| [jHLbL1Eg4gM](https://www.youtube.com/watch?v=jHLbL1Eg4gM) | Casey Muratori: The Anatomy of a 35-Year Mistake, "Clean Code" Horrible Performance (Ryan Peterman) | 1:57:51 | 71 | Measure before abstracting; composition over inheritance; bottom-up API design. Process guidance. |
| [jJJm2nQVolY](https://www.youtube.com/watch?v=jJJm2nQVolY) | Rust++ is here... (Let's Get Rusty) | 0:05:34 | 9 | Valen, a Rust-interop language exploring other memory models. Watch item; no action. |
| [lBDd26b0Gtw](https://www.youtube.com/watch?v=lBDd26b0Gtw) | Succumbing to LLVM - Building A Programming Language (Dylan Falconer) | 1:56:20 | 12 | LLVM for speed to market; keep your own IR and symbol maps. Jet keeps its own MIR. |
| [oitYvDe4nps](https://www.youtube.com/watch?v=oitYvDe4nps) | The Joy of C++26 Contracts - Myths, Misconceptions & Defensive Programming - Herb Sutter (CppCon) | 1:02:49 | 65 | Contracts: no side effects, not for input validation, four modes. -> #3699; observe mode rejected. |
| [pRf8_40EDtM](https://www.youtube.com/watch?v=pRf8_40EDtM) | How to beat a monopoly - Linear Case Study (Tom Delalande) | 0:18:29 | 6 | Linear sync engine: local-first, transactions, rollback, partial bootstrap. -> #3709, #3700. |
| [pfWjbtTQwdo](https://www.youtube.com/watch?v=pfWjbtTQwdo) | Stop Using Dictionaries In 2026 (Coding Together) | 0:00:51 | 4 | Validate data with a schema instead of dicts. Jet: `validate` blocks, typed decode. |
| [v6G_JJK01zU](https://www.youtube.com/watch?v=v6G_JJK01zU) | Learn FastAPI In Only 10 Minutes (Indently) | 0:10:09 | 20 | Decorator routes, typed path params, generated docs, reload. Owner note -> D-MARKER-MODULE1 (#3707). |
| [xP6fIK5g9EQ](https://www.youtube.com/watch?v=xP6fIK5g9EQ) | Why Rust's Iterator Uses an Associated Type Instead of Generics (Semicolon) | 0:09:01 | 6 | Iterator uses an associated type so each iterator has one output. Jet: one Iter model ratified. |
| [y1SGTYh1_Xs](https://www.youtube.com/watch?v=y1SGTYh1_Xs) | Who calls main(), and who gets its return value? (Premature Abstraction) | 0:09:27 | 15 | Startup code calls main and forwards its result. Jet: entry and exit are explicit. |
| [z7wVUfnm7M0](https://www.youtube.com/watch?v=z7wVUfnm7M0) | Why Some Low-Level Projects Are Full of Weird Code Like This (Core Dumped) | 0:18:25 | 16 | Masks to set, clear, toggle, and read bits; packing. -> #3711. |

Strongest unverified assumption: that D-MARKER-MODULE1 option A needs only
module identity in the marker registry and a small grammar addition. The
adversarial review found marker rows keyed by simple package-wide names with
no visibility (`Compiler/JetSema/Source/Sema/ScopeMembers.jet`,
`Compiler/JetAst/Source/AST/Items.jet`). I did not measure how many marker
consumers that change reaches.
