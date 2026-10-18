# Mining V for Jet (2026-10-16)

**Verdict.** V is worth studying for its compiler architecture, its developer
loop and the ambition of its showcase projects. Its headline claims do not
hold up on this machine. Compiling itself (539k lines), V reached ~132k LOC/s
to C on 16 threads and ~77k LOC/s to an executable with tcc; its own
million-line benchmark ran at ~30k LOC/s, failed with default flags, and
three other shapes timed out or ran out of memory. Nothing came near 700k. Per-function hot reload has no
effect in the current compiler. The advertised browser has no published code.
Its C interop is "zero cost" because V emits C and trusts hand-written
declarations, not because it reads C. Jet's designed answers are already
stronger on safety, C bindings and targets. Jet loses today on the things
that run: compile speed (about 1k LOC/s on bench30k), a C source translator,
and a game/graphics/audio platform layer. Those losses now have cards.

Scope: owner-requested 1:1 comparison of V and Jet (syntax, features,
hierarchy, architecture), compile speed toward 500–700k LOC/s, C interop, UI
editor, hot reload, cross compilation, web/browser, Vinix and DOOM, plus a
Jet DOOM feasibility plan. Normal mining mode (report plus Tower).

## Sources and capture

| ID | Kind | Identity | Version | Retrieved |
|---|---|---|---|---|
| vlang.io | site | https://vlang.io | page as served | 2026-10-16 |
| github.com/vlang/v | repo | https://github.com/vlang/v | `9e0a651` (2026-10-16), V 0.5.2 / V3 | 2026-10-16 |
| github.com/vlang/c2v | repo | https://github.com/vlang/c2v | `08112fb` | 2026-10-16 |
| github.com/vlang/doom | repo | https://github.com/vlang/doom | `74995f7` (2024-07-08) | 2026-10-16 |
| github.com/vlang/vinix | repo | https://github.com/vlang/vinix | `151a5e0` | 2026-10-16 |
| github.com/vlang/ui | repo | https://github.com/vlang/ui | `2f6fd14` (2026-05-04) | 2026-10-16 |
| github.com/vlang/ui2 | linked | https://github.com/vlang/ui2 | `293e845` | 2026-10-16 |
| github.com/vlang/ved | repo | https://github.com/vlang/ved | `092035e` | 2026-10-16 |
| github.com/vox-browser/vox | linked | https://github.com/vox-browser/vox | `0369ba8` (2023-11-22) | 2026-10-16 |
| mawfig 2022 review | audience | https://mawfig.github.io/2022/06/18/v-lang-in-2022.html | 2022-06-18 | 2026-10-16 |
| vlang/v issues | audience | #26961 (106 comments), top-reaction issues, #30198 | live | 2026-10-16 |

Jet was read at dev `c47c980fd` (2026-10-15) in
`.agent-worktrees/Main-dev-collectall` and at Tower state of 2026-10-16. Jet
was not executed: the owner reported it is mid-cutover to the self-hosted
compiler, so Jet numbers come from recorded Tower evidence and are labeled.

Capture problems: shallow clones (depth 1), so commit cadence comes from the
GitHub API. The host has no system C toolchain; V was bootstrapped with nix
gcc 15.3, and nixpkgs TinyCC 0.9.27 replaced V's bundled TinyCC, whose crt
search path is compiled in relative to the V root (`thirdparty/tcc/lib`), so
V cannot link with it when run from a temp directory. xeiaso's 2022 V review
returned 404. The machine was shared with other agents (load average 7–10 on
32 threads); timings were pinned to 16 cores and are single samples, labeled
contended. Audience coverage is a sample (top-reaction issues, the largest
recent thread, one long-form review), not the whole tracker.

## Compile speed: what V really does, and the road to 500–700k LOC/s

Claims: vlang.io says ≈700k LOC/s with arm64 and tcc backends on an Apple
M5 and a 0.5 s self-compile; the README says ≈500k on an i5-7500. Neither
names the input. V's own guide measures "V to C" with retired instructions
(`AGENTS.md:601-658`), which is a good protocol.

Measured here (V 0.5.2, V3 `9e0a651`, compiler built `-prod` with gcc 15.3,
`-nocache`, 16 pinned cores of a 32-thread Linux host shared with other jobs,
one sample each; contended):

| Input | Lines | Stage measured | Result | LOC/s |
|---|---|---|---|---|
| `cmd/v` (V compiling itself) | 539,080 | V→C, 16 threads | 4.09 s | ~132k |
| `cmd/v` | 539,080 | V→C, 1 thread | 14.07 s | ~38k |
| `cmd/v` | 539,080 | full build, tcc 0.9.27 | 7.01 s | ~77k |
| `cmd/v` | 539,080 | full build, gcc -O0 | 58.4 s | ~9k |
| `gen1m.v` (V's 1M benchmark, 100k uncalled fns) | 1,000,001 | V→C, default flags | fails: 100k notices hit the 10 GB limit at 29 s | — |
| `gen1m.v` | 1,000,001 | V→C, `-message-limit 5` | 33.9 s (transform 29.5 s); C output 7,738 lines | ~29.5k |
| 100k called functions | 900,002 | V→C | 12.2 s | ~74k |
| 1M statements in one function | 1,000,000 | V→C | timeout > 900 s in check | < 1.1k |
| 500k empty functions | 1,000,000 | V→C | timeout > 900 s | < 1.1k |
| 65k sum types + functions | 259,994 | V→C | fails: 10 GB limit at 15 s | — |
| FastC self-host (no checker) | `vlib/v/v.v` | scanner→C | C in 137 ms, then invalid C (`')' expected`) | n/a |
| warm `v run hello.v` | 24,619 parsed | full loop | 86–91 ms | — |

Corroboration: issue #26961 (contributors on Linux) shows V3 self-compile at
32.7 s serial / 8.6 s `-parallel-cc` versus 6.4 s / 1.6 s for V 0.5.1, and
`-prod` self-builds over 53 minutes; the 2022 review found 387k to 5.7k LOC/s
depending on shape.

Per-thread phase rates on `cmd/v` (1 thread): parse ~1.0M LOC/s, check ~128k,
transform ~140k, C generation ~115k. With 16 threads: parse 1.9M, check
590k, transform 300k, cgen 960k; overall ~132k because the stages are
sequential and transform scales poorly.

Jet today (recorded, not rerun): bench30k front end 29.37 s for 30,100 lines,
~1,025 LOC/s, 5.8 GB RSS (#4551, 2026-10-09); about 18M instructions per line,
39% in Rust-text emission (`Docs/research/speed-frontend-hotspots-2026-10.md`).
V's realistic front end is ~130x faster than Jet's on this evidence.

What it takes to reach 500–700k LOC/s on realistic code:

1. V shows that a flat AST parses at ~1M LOC/s per thread. Jet's budget
   (1M/core, fast-compiler-architecture §2) is proven reachable.
2. V checks and lowers at only ~130k LOC/s per thread despite flat AST,
   structured types and arenas. Jet's budgets (check 300k/core, lower
   500k/core) are 2–4x beyond V. That is the real risk and where the design
   must win: integer IDs and hash-consed types (R1–R2), side tables instead of
   rewritten trees (R3), and lowering straight from checked data with no
   separate transform and re-annotate passes (V spends 45% of its front end
   in transform).
3. Scaling must be near linear with no serial stage: V's 16-thread run is
   only 3.4x its 1-thread run. Use V's atomic largest-first work claiming
   and keep every stage parallel (#4310).
4. Never chase a number with an unchecked path (FastC) or with dead code
   (gen1m). Track two metrics per shape (owner direction): front end to IR,
   and full source to executable. The full build needs Jet's own native
   backend and linker; V reaches 77k end to end only with tcc and 9k with gcc.
5. Diagnostics must stay linear: V dies rendering 100k notices.

Arithmetic for the target: 500k LOC/s on 16 threads at 75% efficiency needs
~42k LOC/s per thread for the whole front end, so every phase together must
cost ≤ 24 µs per line per thread. V's whole front end costs ~26 µs per line
per thread (14.07 s / 539k). Jet must match V's per-line cost and then scale
four times better, or cut per-line cost below V's. Cards: #5039 (peer cells
and shape matrix), #4999 (techniques), #4551 (gate), #4310 (parallelism).


## C interop, translation and the on-ramp

What V actually does (vlang/v `doc/docs.md:10403-11157`):

- V has no C parser. `#include` is copied into the generated C, and every
  function and struct is redeclared by hand (`fn C.strlen(s &char) int`,
  `struct C.Foo { … }`, `@[typedef]`). Declarations may be partial and are
  never checked against the header.
- `#flag`, `#pkgconfig`, `#preinclude`/`#postinclude`, `@[export]`,
  `-o file.c`/`-o file.o` and `-shared` cover build integration and calling V
  from C.
- Live probe p9: a wrong `fn C.strlen(s &char, extra int) f64` passes V's
  checker; the user sees raw TinyCC output on generated `src.c`
  (`src.c:5002: error: too many arguments to function`) under a page of
  header warnings.
- c2v (Clang JSON AST → V) is the real on-ramp: it translated Chocolate Doom's
  game logic and the Doom 3 engine. Translated files relax V's rules
  (`@[translated]`: globals, pointer writes without `unsafe`, C conversions).

Jet today: one binder model for every language (`use c.png as png`,
`jet bind header.h --pkg … --overlay …`), descriptors with ownership,
callbacks, async and close-on-`defer` (`Examples/ffi/*`), inline `#FFI(c)`
gated by `#Unsafe("reason")`, unsupported signatures rejected before emission,
foreign errors behind E3208 (`Docs/spec/spec.md:2711-2747`). Gaps: real
headers such as sqlite3 and SDL3 are not yet imported through the shared Clang
extractor (#4174, open), and there is no C source importer by ratified law
(D-ADOPT-TIER1=A, `migration-tier-map.md:28`).

Verdict: Jet's design already beats V on checking. V wins on reach: it
translates whole C programs. Ballot D-CSRC-IMPORT1 (#5034) asks whether Jet
should translate C function by function into safe Jet, keeping unprovable
functions in C behind the binder; V's relaxed-file model is not offered
because it needs an I1 exception.

## Safety claims, rechecked live

| Probe (V 0.5.2) | 2022 review | Today | Jet law |
|---|---|---|---|
| `Node{123, 0, 0}` null reference field | segfault | rejected | no null |
| `[]&int{len: 1}` then read | segfault | warning, then "invalid memory access" (exit 255) | write-before-read (D-UNINIT1) |
| `voidptr(123)` cast to `&T`, no `unsafe` | segfault | compiles, crashes (exit 255) | raw pointers only in `#Unsafe` (D-CAP9) |
| `i32` max + 1 | UB | wraps silently to −2147483648 (`-fwrapv`) | traps by default (D-NUMOPS1) |
| divide by zero | SIGFPE | panic | trap |
| immutable → mutable alias | mutated | rejected | access sigils (D-MEM1) |
| out of bounds | panic | panic | trap |
| wrong C declaration | — | raw C compiler error | registered diagnostic (I2) |
| `@[noalloc]` vs interpolation | — | caught | `!Mem.Alloc` (E0921) |

V has improved since 2022, but "no null" and "no undefined behavior" still do
not hold without `unsafe`. Card #5037 proves each case is rejected or trapped
in Jet on every tier.

Hidden allocation (Vinix `AGENTS.md:22-81`): in a 248k-line kernel built with
`-gc none -manualfree`, V allocates where source shows nothing (interpolation
temporaries leak, interface conversion boxes 384 bytes, escaping locals move
to the heap) and `-warn-about-allocs` reports little of it. This is the
strongest evidence for Jet's priority 3 (no hidden runtime cost); logged on
#4566.

## Feature-by-feature comparison

Status words: **shipped** (code and examples), **ratified** (decided,
unbuilt or partial), **gap**.

| Area | V (evidence) | Jet (evidence) | Gap side |
|---|---|---|---|
| Bindings | `x := 1` immutable, `mut x := 1`, no shadowing | `x :: 1`, `x := 1`, `=` reassign | none |
| Functions | `fn f(a int) int`, multi-return, `mut` args marked at call | `fn f(a: Int) -> Int`, labels/defaults/variadics, `&`/`^` at call | Jet richer |
| Errors | `?T` option, `!T` result, `or {}` blocks, `!` propagate | `Int Err!`, `?T`, `??`, implicit propagation, typed error rails | equal; Jet has typed errors |
| Sum types | `type S = A \| B`, `is`/`as`, smartcast | enums with payloads, exhaustive `if x == {…}` | equal |
| Generics | `fn f[T Named](x T)`; constraints by interface, sum type (`T Number` with `Number = int \| f64`) or struct (docs:5664-5708; ROADMAP still lists constraints as 1.0 work, stale) | `<T: Trait>`, traits, dynamic dispatch via trait type | worth checking: sum-type and struct bounds |
| Interfaces | structural, may hold fields; boxing on conversion | named traits (S28), explicit impl | design choice |
| Memory | tracing GC default, `-autofree` experimental, `-gc none`, `-prealloc` arenas, optional `-ownership` | ownership and access sigils always on, arenas `mem.Arena`, regions, `!Mem.*` denials | Jet stronger by default |
| Unsafe | `unsafe {}` block, no reason | `#Unsafe("reason")`, audited | Jet stronger |
| Concurrency | `spawn`, channels, `select`, `shared`/`lock`, coroutines (not Windows) | `task`, `.join()`, `task.all/race/any/group` | equal; V has shared-lock |
| Comptime | `$if`, `$for` fields/methods, `$embed_file`, `$tmpl`, `$env`, `$compile_error` | `comptime` values, `embed_file`, reflection via facts; no type creation (S26) | V reflects more; deliberate |
| Overflow | wraps (`-fwrapv`) | traps; explicit `wrapping_/saturating_/checked_` | Jet stronger |
| Globals | `__global` behind `-enable-globals` | none in safe code | equal intent |
| ORM / SQL | `sql db { select from T where … }` built into the language | `core.db`, checked SQL, `Query<T>` | V syntax-level; Jet library |
| JSON | codegen, no reflection | `#[Codable]` derive | equal |
| Testing | `test_` functions, `assert` | `#Test`, property tests, doctests | Jet richer |
| Formatter / docs / vet | `v fmt`, `v doc`, `v vet` | `jet fmt`, `jet doc`, `jet lint` | equal |
| Package manager | VPM, `v install`, versioning not done (ROADMAP) | Jetpack: lock, registry, trust | Jet stronger |
| REPL / scripts | `v repl`, `.vsh` shell scripts | `jet repl`, `jet eval` | V has shell-script mode |
| Agent tooling | `v mcp`, `v skills`, `-json-errors` | `jet fix`, `jet explain`, `--json`, typed edits | equal |
| Hot reload | `@[live]` currently no effect (docs:11366); `v watch` restart; warm `v run` 86–91 ms measured | `#Persist` state reload (#765), `jet dev` 289 ms edit-to-result (#2756) | V faster loop; neither beats Vite |
| Cross compile | external C cross compiler; no macOS target; `-os cross` portable C | target machines, sysroots, boards, wasm (#1058-1060, #758) | Jet stronger |
| Backends | C (primary), FastC, ARM64 native via SSA (macOS), WASM integer core, JS early | own native backend and linker (JetBackend), web JS+WASM partition | equal ambition |
| C interop | hand declarations, no header reading | checked binder, overlays, inline `#FFI(c)` under `#Unsafe` | Jet stronger, unfinished (#4174) |
| C → source | c2v (C and C++) | none for C (D-ADOPT-TIER1) | Jet gap, ballot #5034 |
| GUI | `gg`/sokol, `ui`, `ui2` (native widgets + gg renderer, VML, Studio designer), `gui` immediate mode | `core.ui`, native UI host, Canvas, raylib bridge | V broader today |
| Web | veb (picoev, compiled templates, live reload), Gitly | `core.web`, routes, web backend | equal intent |
| Browser engine | Vox: README only | none | neither |
| OS | Vinix: 248k-line kernel, desktop, runs Firefox/Chromium | jetos cards frozen; freestanding runtime planned (#4922) | V ahead |
| Games / audio | gg, sokol, DOOM via c2v + C | `core.game` shell, raylib bridge, no audio | Jet gap, #5035 |

## Architecture (hierarchy)

V3 pipeline (`vlib/v/README.md:747-757`, `AGENTS.md:660-686`):
`scanner → flat parser → imports → check → transform → annotate types →
markused → gen C → cc`, with side branches SSA → ARM64 → built-in Mach-O
linker, WASM, and FastC (scanner → C, no checker). One driver, in process. The
old compiler is removed from the tree but auto-downloaded and used as a silent
retry target (`README.md:111-139`).

Jet: lexer → parser → sema (owns all checks, I3) → comptime → TIR → MIR →
optimizer → own native backend and linker (Rust-emitting path being retired),
with the evaluator only for compile time (I9, D-EXEC2=A). The structural
difference that matters for speed is not the stage list but the data:
V3 already has flat AST, structured types and arenas; Jet's `Compiler/`
still carries string-keyed symbols, recursive types and cloned trees
(fast-compiler-architecture R1–R4, R11).

## Hot reload, UI editor, web, Vinix

- **Hot reload:** the advertised feature is dormant in V3. Nothing to copy;
  Jet's target stays Vite/Bun latency (#2756).
- **UI editor:** UI2 Studio (Delphi-style designer, ui2 README:522-534) is
  the best idea here, but it edits VML, a second language with its own
  expressions. Jet should copy the workflow on Canvas while editing Jet source
  (#4171, I8).
- **Web:** veb is solid and pragmatic. The "native browser" (Vox) has no
  code, so there is nothing to mine.
- **Vinix:** a real stress test and the strongest V proof point, built with
  heavy agent help (`AGENTS.md`, `CLAUDE.md` in-tree). It also exposes V's
  weak spots: hidden allocation, manual freeing with `-gc none`, unresolved
  SMP faults (README, LibreOffice section). Jet's equivalent (jetos) stays
  frozen; the prerequisite is the freestanding runtime (#4922).

## DOOM

vlang/doom translates Chocolate Doom's `src/doom` only and links the C
platform objects and SDL (`build_whole_project.sh:10-36`); sound effects are
broken and constants inlined (README). Owner direction (2026-10-16): a Jet
DOOM lives in a separate project under the license DOOM needs (GPL-2.0), Jet
stays MIT, and the port is Jet rather than C FFI.

Feasibility: the game is fixed-point integer code, tables and a software
renderer, all expressible in safe Jet with fixed-width wrapping arithmetic.
What Jet lacks is the platform layer: a window that presents a pixel buffer,
PCM audio, and raw input from Core rather than SDL or raylib (#5035, blocks
#5036). Correctness is provable: demo playback must stay in sync tic for tic
with Chocolate Doom. Peers with source: Chocolate Doom and doomgeneric (C),
gore (Go), managed-doom (C#), iron-doom (Rust), zigDOOM (Zig), vlang/doom (V).

## Avoid list

| Mistake | Evidence | Jet exposure |
|---|---|---|
| Unchecked fast path for speed | FastC skips the checker (`vlib/v/README.md:250-297`) | Immune by I3 |
| Silent fallback to an older compiler or C compiler | `README.md:111-139, 202-205` | Immune by greenfield cutover and I8 |
| Speed claims without the input shape | 700k vs 500k vs measured ~30k | Exposed if headline numbers drop shape; #4551 card mitigates |
| Benchmarks that the dead-code pass erases | gen1m output is 7,738 C lines | Exposed; shape matrix in new card |
| One full diagnostic per item at scale | gen1m dies at 10 GB on notices | Unmeasured; new cell |
| Relaxed rules for translated code | `@[translated]` | Avoided in D-CSRC-IMPORT1 |
| Advertising features that do not run | `@[live]`, Vox | Exposed only by process; Tower status discipline |
| Closing live issues to hit zero | #26961 comment 5954564154 | Immune by card law |
| Hidden allocation | Vinix AGENTS.md | Exposed; #4566 |
| Second UI language | VML | Avoid (I8), #4171 |

## Agent optimality

V moves verdict latency well (sub-second warm builds when caches hit) and
ships agent aids (`v mcp`, `v skills`, `-json-errors`, `AGENTS.md` with a
measurement protocol). It is weak on verdict fidelity: wrong C declarations,
reference-list init and pointer casts reach runtime, and backend errors reach
the user. Jet is strongest on fidelity and actionability (I2–I4, `jet fix`
typed edits) and weakest on latency, the same gap the speed section measures.

## Findings and dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 V speed claims need same-machine shape-matrix cells; two metrics | card | #5039 |
| F2 V3 front-end techniques missing from the Jet speed plan | card | #4999 |
| F3 C source translation on-ramp (c2v) | card | #5034 |
| F4 Real C header import; mismatched declarations must be Jet diagnostics | card | #4174 |
| F5 V safety failures must be proved rejected or trapped in Jet | card | #5037 |
| F6 Hidden compiler-inserted allocations | card | #4566 |
| F7 Hot reload: V has no working target; Vite/Bun remain peers | card | #2756 |
| F8 Visual UI designer without a second UI language | card | #4171 |
| F9 Jet DOOM in a separate GPL-2.0 project, mostly Jet | card | #5036 |
| F10 Core pixel surface, PCM audio and raw input | card | #5035 |
| F11 Cross compilation sysroot resolution | card | #758 |
| F12 OS stress test needs a freestanding runtime | card | #4922 |
| F13 Generic bounds by closed type set or struct shape | card | #5038 |
| F14 Unchecked fast compile path (FastC) | no-action | rejected: structurally excluded by I3 and I2 |
| F15 Silent fallback to an older compiler or C compiler | no-action | rejected: excluded by greenfield cutover and I8 |
| F16 Closing open issues to reach a zero count | no-action | archived: Jet card law already forbids closing live work |
| F17 Vox browser claim | no-action | archived: repository holds no code to mine |
<!-- /audit-dispositions -->

Strongest unverified assumption: single contended samples. The ranking is
robust (two orders of magnitude), but each V number may move ±20% on a quiet
machine; #5039 requires paired same-session receipts before any gate verdict.
