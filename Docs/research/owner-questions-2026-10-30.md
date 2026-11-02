# Answers to the owner's questions of 2026-10-30

Ten questions, one section each, in card order. Each section opens with a short direct answer, then gives the evidence, the cards and the ballots it produced. Ballots wait on the Tower Decisions page; cards are linked by number.

- #6376 — What five years of language release notes teach Jet
- #6377 — Jonathan Blow on error reporting, source locations and stack traces
- Should Jet index from 1? (#6378)
- Why is the type called String and not Text? (#6379)
- Making Jet the best language for extensions, mods and config (#6380)
- SOLID: what it gets right for Jet, and where it fails (#6381)
- Memory freed too late: scope-end cleanup and tail recursion (#6382)
- Should `fn dev` cover environments, or should there be an env entry point? (#6383)
- Bit operations as sigils, and what `~` should mean (#6384)
- Zig's `@` builtins, and what Jet should take from them (#6385)

## #6376 — What five years of language release notes teach Jet

**Plain answer.** From 2021 to 2026, the language changes that worked were safe by default, had a measured cost, and came with a mechanical migration. The ones that failed put annotations on correct code, blocked the runtime in ways users could not see, or offered safety that a second path could skip.
Jet already follows most of these lessons. Probes found five real gaps: SQL and HTML injection through public fields, tasks stalled by OS waits, 192 MiB worker stacks that crash 32 trivial tasks under a 4 GB limit, a `jet fix` that breaks code, and one wrong error text.
One owner choice remains: how `jet fix` should move callers when a deprecated function's replacement takes different arguments (ballot D-DEPRECATE-INLINE1).

Full catalogue: `Docs/research/2026-10-30-language-release-notes.md`. It covers 25 languages, with a table per language: version, change, how it went in practice with its source, Jet today, and Jet's action and why.

### Source and method

- Official release notes, JEPs, PEPs and team blogs for C, C++, Rust, Go, Zig, Swift, Kotlin, Java, C#, Python, JS/TS, Ruby, PHP, Dart, Elixir/Erlang, Haskell, OCaml, Julia, Odin, Nim, V, Lua, Gleam, Mojo and Carbon, 2021–2026.
- Jet state: published `jet` 03c64ad0c4, dev worktree at 59213de00f, and Tower decisions and cards read on 2026-10-30.
- 18 probe files in the card's scratch probe set (rel/, not kept in the repository), all run on 03c64ad0c4 (`small/` holds the 8-, 16- and 32-task variants of r08, and `fixcopy/` the file `jet fix` edited). r07, the C-call variant of r08, could not resolve its C header in this environment. The shell's soft address-space limit is 4 GB; the two runs that raised it to 16 GB are marked.

### Lessons, with Jet's position

1. **Safe defaults win when the cost is measured.**
   - Go 1.22's per-iteration loop variables caused no production problems at Google.
   - Hardened libc++ cost Google 0.30% and found over 1,000 bugs.
   - Jet already captures per iteration (probe r04 prints `1`) and checks bounds by default.
2. **Breaks survive only with a mechanical fix.** Rust 2024 shipped `cargo fix`, Go 1.26 added `//go:fix inline`, and Mojo 1.0 shipped deprecated aliases and fix-its. Zig 0.15's "Writergate" broke every I/O caller with only hand migration.
   - Jet has the rule (#6068), but `jet fix` turned a compiling `double(4)` into a broken `scaled(4)` (r11).
3. **Withdrawn features died of cost or leaks.** Java string templates were withdrawn, Dart macros cancelled, Go error syntax abandoned, V's autofree is still "avoid using it", and Python 3.14.5 reverted its new GC.
   - Jet already avoids these: metaprograms cannot rewrite code, there are no glob imports, and there is no `async`.
4. **Concurrency must not punish correct code or block silently.**
   - Swift 6's checks flagged correct globals, and 6.2 changed the defaults.
   - Java virtual threads pinned their carrier threads until JEP 491.
   - Jet has the same blocking hole for OS waits (r08: a ready task waited 3.02 s).
5. **A safety type needs one door.** Java refused plain interpolation into templates, and Python's t-strings are not strings.
   - Jet refuses a plain String as SQL (E0149), but `SQL{text: s, values: []}` and `HTML{markup: s}` build both from any runtime string. r01f returns every row for `x' OR '1'='1`.

### Gaps found (cards in `answers/6376-cards.json`)

1. **Checked SQL and HTML built through their public fields** (r01e, r01f, r10). Make the fields private and keep `.raw` as the single, listed door.
2. **OS waits and foreign calls hold the worker** (r08, `HostProcess.jet:166-177`). This goes against D-ASYNCRT1=A's "blocking calls yield automatically".
3. **At least 192 MiB of stack per worker** (`Thread.jet:353-370`). Failing to start one is reported as a program panic (r09: 32 trivial tasks, exit 70 under a 4 GB limit; `done` at 16 GB).
4. **`jet fix` applies a rename that breaks the call**, and L2001 promises automatic migration anyway (r11).
5. **E0111 on a loop variable** says it "was made with `::`" and suggests `i := ...` (r06).

Criterion for an existing card: **#5026** (general defer). `return`, `break`, `next` and `?` inside a defer must get their registered codes. Today r03b gets E0003 "Expected a value", plus a follow-on error. Evidence: C's defer TS 25755 and Python's PEP 765.

Evidence for an open ballot: **#6332 / D-MODULE-WRITE1**. Swift 6 → 6.2 and Lua 5.5 show that the rule must not make single-task code carry concurrency annotations. r05 prints `2` today with no diagnostic.

Already filed, re-confirmed: #6374 (`String + String` gives a number-type error, r01b).

### Owner choice

- **D-DEPRECATE-INLINE1** (card #6405): `answers/ballots/D-DEPRECATE-INLINE1.json`, PLAIN OK and READY. When a deprecated function's replacement takes different arguments, how does `jet fix` move callers?
  - A (recommended): three steps in order. Inline the old body when it is a one-line wrapper, as Go 1.26 does. Otherwise use the author's call pattern from the marker. Otherwise leave the call and print the exact edit.
  - B: rename only.
  - C: a call pattern for every migration.

## #6377 — Jonathan Blow on error reporting, source locations and stack traces

**Plain answer.** Jet already does much of what the talk asks for. It has no error cascades from one unknown name, it reports all files, every error ends with a help link, and failure trails show each hop.
The probes found four real gaps. A missing `}` gets blamed on the wrong block. A compile-time stop points at the `comptime` block, not at the failing line. A panic shows only its innermost frame. One error message suggests a spelling the compiler refuses.
Two owner choices come from the talk's metaprogram example: whether a house rule's report can be a real error at the bad line, and whether a library error may carry its own link.

Full catalogue: `Docs/research/2026-10-30-blow-error-reporting-talk.md`. It has 33 timestamped rows, each with what Jet does today and what Jet should do.

### Source and method

- Talk: "Why Compilers Bury the Real Error", Jonathan Blow Clips, 2026-10-15, 12:37 (<https://www.youtube.com/watch?v=ZF1m1DsW3Rg>), a clip of <https://youtube.com/watch?v=MnctEW1oL-E&t=8847s>.
- Mined with mine-for-jet-v2. Capture root: the mining run for #6377 (local capture, not kept). It holds 4 segments, 37 ledger items and 29 claims (23 micro, 6 macro).
  `check_coverage.mjs` passes. Density is 2.93 ledger items per minute against a floor of 1.
- Jet state: digest at master 1eef4640ad, dev 03c64ad0c4, Tower revision 86436.
  Probes are in the card's scratch probe set (trace/, not kept in the repository) and ran on the published `jet` 03c64ad0c4.
- Recall audit: PASS by a fresh reviewer. Seed `6377-r1-kestrel` sampled segments 01 (00:00–03:40) and 02 (03:40–07:36).
  The reviewer re-mined 22 concepts with 0 misses and 0 disputed mappings, and `check_coverage.mjs --audit` exited 0 (`recall-audit.json`).

### What Jet already does well (probe evidence)

- **No cascade from one unknown name** (talk 00:30–01:53, "poison values"). t01 uses an unknown name in three later lines and gets exactly one E0107.
- **Errors from every function, not just the first per file** (talk 02:33). t02 reports both unknown names in two functions of one file. The owner wants all errors by default (#4914).
- **Parallel checking is no excuse** (talk 02:53). Bodies are checked on compiler threads, each with its own diagnostic list (`CheckProgram.jet:2699-2702`).
- **Failures carry a full trail** (talk 11:23). t07c passes a failure up three functions and prints the root cause, "failed here", and every hop with file, line and source text.
  Tower already records #6014 criterion 1 as met (landed 08f32b2d24).
- **Help links** (talk 07:57). Every compiler error ends with `More: jet-lang.dev/e/<code>` (D-DIAG-URL1=B).

### Gaps found (new cards #6397–#6400)

1. **#6397: a missing `}` is blamed on the outer block** (talk 00:44, 06:25–07:29). In t03 the `if` on line 9 never closes.
   The first error printed is a follow-on E0003 at line 15. The real E0083 comes second, blames `fn run() {` on line 7, and suggests adding `}` at the end of the file.
   The `}` on line 13 sits at column 1 but closes a block opened at column 5. Blow's indentation idea fits Jet especially well, because Jet's formatter keeps indentation reliable.
2. **#6398: a compile-time stop points at the block, not the line** (talk 04:36, 08:24). In t04, `check_size` panics on line 4 and is called on line 10. E3010 points only at `comptime {` on line 9.
   The comptime stop-point work is card #6393, which #6382 found.
3. **#6399: a panic shows one frame** (talk 09:47–11:56). In t06 the panic is three calls deep. `jet run` and a `--backtrace=full` build both print only `inner` at line 4.
   Jet's own frame walk (`Fault.jet`) serves only native faults. Failures print the whole chain, while panics print the least.
4. **#6400: E1321 recommends a refused spelling.** It says an entry "returns `()` or `() ?`" (`Outputs.jet:307`). Writing `-> () ?` gets the same E1321 (t07d), and `-> () LoadError!` gets E0003 (t07b).
   The accepted form is `fn run() LoadError!` (t07c).

### Criteria added to existing cards

- **#3520 (reflection), C24.** Compile-time code can read any declaration's source location as a SourceLocation record (talk 07:30, 08:36). #3520 decides the spelling.
  `$origin` is not that fact: it is the ratified `#Track` value-origin fact (D-COMPILER-NS1), so t10b's `null` for `Point.$origin` is correct today. The gap is that no declaration location fact is ratified.
- **#5025 (implicit context), C6 and C7.** `context.trace` has no contract beyond the words "stack trace" (D-JAI-CONTEXT1=A). Two criteria were added:
  it is an owned list of the ratified SourceLocation record from the same frame source as panics and faults, printed the same on every target;
  and it costs nothing measurable when unread, against the pre-change commit as baseline (talk 09:56 claims this but gives no number).
- **#6014 (failure trails), C4.** The trail header says "via ?" when no `?` was written (t07c).
- **#3720 (readability sweep), C24.** E0047 doubles the backticks around a type name (t10: "``OriginInfo``").
  E3001's Why contains "(I2)" and "shared report boundary". #3720's existing CI guard against invariant tags already covers this, and t06 is a witness.

### Owner choices (ballots, short profile, `recommendation.losses` = [])

Both hang on #6388, the card that builds `reject`. Both pass PLAIN OK and READY as standalone files in `answers/ballots/`.

- **D-REJECT-AT1 — one report channel for a library's compile-time checks.** Blow's codebase rule (07:57–08:35) reports an error at the offending node.
  Jet already ratifies the channel for codebase rules: D-HOOK-STAGES1=A hooks such as `#On(compiler.declared)` return `compiler.finding(d, msg)`, and D-JAI-EVENTS1=A adds the `settled` and `finished` stages.
  Those findings are message-only L1401 warnings on a whole declaration, with no link. Only a team's deny policy makes them errors (E1293).
  `reject` (D-MARKER-DIAG1=A) raises a full library error, but only at the marker's use site. D-META-COMPILER1's `Error` event observes diagnostics and does not raise them.
  Recommend A: hook findings may carry the same Diagnostic value `reject` takes, and both take an optional `at:` span from their own payload. B keeps the two ratified channels, and C gives only `reject` a location.
- **D-DIAG-LIBLINK1 — whether a library's error can carry its own link.** Blow's example links to a web page.
  Jet promises a link on every error (D-DIAG-URL1=B), but the library `Diagnostic` trait has no place for one.
  Recommend A: an optional `more` link printed beside `jet explain <code>`, which still works offline or after the page dies. Hook findings built from a Diagnostic print the same line; message-only findings keep the L1401 link.
  The alternatives are B, no link, and C, one docs address per package.

### Owner questions answered

| Owner asked about | What Jet has today | What this run adds |
| --- | --- | --- |
| Debug info | Jet walks its own debug info for native faults (`Fault.jet`). The debugger has write watches on Jet paths (`Dap03.jet:315-321`). | Nothing new; a language-level data breakpoint is not adopted (below). |
| Stack traces | Failures print every hop (t07c). Panics print one frame (t06), and `backtrace` covers native faults only. | #6399 for full panic traces; #5025 C6/C7 for a typed, free `context.trace`. |
| Metaprogram access to trace and error info | D-META-COMPILER1's `Error` event hands handlers each diagnostic. `context.trace` is ratified only as "stack trace". No declaration location fact exists. | #5025 C6 types the trace. #3520 C24 asks for a declaration location fact. |
| Metaprograms adding errors | `reject` is ratified but not built (E0102 today, #6388). Hook findings are ratified warnings with no link. | Ballots D-REJECT-AT1 (one channel, any span, real errors) and D-DIAG-LIBLINK1 (own link). |

### Not adopted

- **Jai's first-error-per-file rule** (talk 02:33). Jet already reports more, and the owner wants all errors (#4914).
- **Jai's linked list of trace nodes on every call** (talk 10:23). Jet's frame walk over its own debug info costs nothing until it is read. It gives the same file and line output on every target.
- **A language-level data breakpoint** (talk 12:00). The debugger already has write watches on Jet paths (`Dap03.jet:315-321`). A language write hook would add a hidden effect to every write. No card.

Cards and criteria as JSON: `answers/6377-cards.json` (now posted by Main). Ballots: `answers/ballots/D-REJECT-AT1.json` and `answers/ballots/D-DIAG-LIBLINK1.json`.

## Should Jet index from 1? (#6378)

**Answer.** No. Jet should keep counting from 0 and stop beginners from needing position numbers at all. Counting from 1 suits languages for scientists and scripters (Lua, Julia, MATLAB, R, Fortran). Jet talks directly to C, machine memory, files and editor protocols, and all of those count from 0. A switch would touch 3,261 of Jet's 17,685 source files, including the compiler itself. Every position that crosses into C or a file format would then need a hand-checked ±1. The beginner problem is real, though. The fix we recommend: use words (`first`, `last`, paired loops, open slices), and have the compiler explain `xs[xs.len()]` before the program runs. Ballot: **D-INDEX-BASE1** (A stay 0 and teach, B 1 everywhere, C 1 for lists and text but 0 for bytes and memory).

### Evidence

**Languages that count from 1.**

- Lua, Julia, MATLAB, R, Fortran (default lower bound 1, can be declared), COBOL and Smalltalk count from 1. SQL `SUBSTRING` and XPath do too.
- Pascal and Ada make you declare the range (`array[1..10]`), so the base is part of the type.
- Lua's history paper (Ierusalimschy, de Figueiredo, Celes, "The Evolution of Lua", HOPL III 2007, lua.org/doc/hopl.pdf §3–4):
  - Its first users were Petrobras engineers, and the authors designed for "engineers with a Fortran background".
  - Array constructors already started at 1 in Lua 1.1.
  - That audience is the reason Lua counts from 1.

**Languages that count from 0.** C, C++, Java, C#, JavaScript, Python, Go, Rust, Swift, Zig and Odin.

**Dijkstra, EWD831 "Why numbering should start at zero" (1982).**

- Of the four ways to write a range, he argues for `a ≤ i < b`, because adjacent ranges then share bounds and the length is `b − a`.
- If you start at 1, N items run `1 ≤ i < N+1`. If you start at 0, they run `0 ≤ i < N`, with no +1 anywhere.
- Jet already uses this convention: `0..<n` (D-RANGE-EXCL1=C).

**Off-by-one costs on both sides.**

- Counting from 0 makes the last item `len − 1`, and leaving out the −1 crashes.
- Counting from 1 makes every half-open range `1..<n+1` and every offset computation `base + (i − 1)·size`.
- Julia added OffsetArrays for other bases. Generic code that assumed 1 then gave wrong results on offset arrays, a widely reported source of correctness bugs. Any choice of base leaks somewhere.

**Interop.**

- C arrays, pointers, byte offsets in files, LSP columns (Jet's own LSP), JSON Pointer, WASM memory and GPU `at.linear` (ratified GPU decisions) are all counted from 0.
- Lua shows the cost: its C API stack is indexed from 1 while C arrays are indexed from 0, and binding authors convert at every crossing.

### Migration census (dev 03c64ad0c4, `census/index-census.mjs` in this lane)

| Spelling whose meaning depends on the base | Compiler | Core | Examples | tests | Jetpack | Tools | Total |
|---|---:|---:|---:|---:|---:|---:|---:|
| literal `[0]` | 3,320 | 597 | 290 | 1,036 | 982 | 536 | 6,763 |
| other literal `[N]` | 1,467 | 620 | 237 | 883 | 537 | 655 | 4,404 |
| `[x ± 1]` | 948 | 416 | 15 | 167 | 288 | 152 | 1,986 |
| `.len() - 1` | 661 | 197 | 17 | 52 | 261 | 60 | 1,248 |
| ranges from 0 | 2,327 | 1,015 | 239 | 841 | 670 | 221 | 5,316 |
| `..<` (half-open) | 3,113 | 1,225 | 219 | 548 | 883 | 312 | 6,301 |
| slices `xs[a..b]` | 563 | 188 | 80 | 378 | 128 | 60 | 1,398 |
| paired loops `loop (i, x) in` (printed numbers change) | 1,033 | 24 | 13 | 52 | 8 | 61 | 1,191 |
| position-taking calls (`insert`, `remove`, `swap`, …) | 329 | 103 | 93 | 173 | 473 | 13 | 1,184 |
| foreign/unsafe pointer sites | 828 | 291 | 162 | 862 | 67 | 57 | 2,273 |

3,261 files contain at least one of these spellings. Literal indexes and `.len() - 1` can be shifted by `jet fix`. Positions computed from data cannot. Byte offsets read from files, LSP positions, C offsets and GPU positions each need a reviewed ±1 at the boundary. The compiler is Jet code, so a switch also rewrites the compiler that builds the switch, followed by a bootstrap.

### What Jet already does for beginners

- Paired loops `loop (i, x) in xs` and `xs.indexes` (D-RANGE-EXCL1=C).
- A teaching error for `0..xs.len()`.
- Open slices `xs[2..]` and `xs[..<3]` (D-SLICE-OPEN1=A).
- `xs.first()` and `xs.last()`, which return an Option.

Most everyday code never writes a position. The gap is that indexing exactly one past the end, `xs[xs.len()]`, has no compile-time lesson today: no such diagnostic exists in Diagnostics.jet or JetSema.

### The same program under each option

```jet
names :: ["ada", "bo", "cy"]
// A (today + lesson): names.first(), names.last(); loop prints 0: ada; names[..<2]
//                     names[names.len()] → compile-time error, fix: names.last()
// B (1 everywhere):   names[1], names[names.len()]; loop prints 1: ada; names[..2]; bytes[1] is the first byte
// C (split):          names[1] is ada, but "hé".bytes()[0] is the first byte
```

### Ballot

Ballot D-INDEX-BASE1. PLAIN OK, READY. Recommendation A, `losses=[]`.

**Card draft if A wins:**

```json
{"title": "Compile-time lesson for one-past-the-end indexes",
 "body": "D-INDEX-BASE1=A keeps 0-based positions and teaches the classic mistake. When sema can prove an index equals the receiver's length (`xs[xs.len()]`, a literal equal to a fixed array's size), it reports a registered error naming `xs.last()` and `xs[xs.len() - 1]` as fixes. Guides lead with paired loops, first/last and open slices before raw positions.",
 "crit": ["Registered diagnostic fires on xs[xs.len()] and fixed-array literal index == size, with fix lines naming .last() and len() - 1", "No false positive on xs[xs.len() - 1] or on unproven indexes; examples cover both", "Getting-started guide shows first/last, paired loops and open slices before any numeric index"]}
```

## Why is the type called String and not Text? (#6379)

**Answer.** "String" is the programmer's word for a row of symbols. Logicians used it before computers did, and the first text-processing languages made it standard in the late 1950s and 1960s. Almost every language since has kept it. `Text` is plainer English, but other languages mostly use it to tell a packed text type apart from a slower list-based `String`. Jet has only one text type, and it is already packed, so there is nothing for the new name to distinguish. We recommend keeping `String`. The vote is D-TEXT-NAMES1 v3, on card #6342.

### Where the word came from

- **Before computers.** "String" has long meant things in a row, as in a string of beads. In 19th-century typesetting, a compositor's "string" was a measured length of set type, and his pay was based on it.[^wp]
- **Logic and linguistics.** These fields took the word for a row of marks studied apart from their meaning. In 1918, C. I. Lewis described a mathematical system as "any set of strings of recognisable marks".[^wp] Formal language theory still defines a string as a finite sequence of symbols.
- **Early computing.** Jean Sammet calls COMIT (Victor Yngve, MIT, late 1950s) "the first realistic string handling and pattern matching language".[^wp] SNOBOL followed: D. J. Farber, R. E. Griswold and I. P. Polonsky, "SNOBOL, A String Manipulation Language", *JACM* 11(1), 1964. The Revised Report on ALGOL 60 (Naur, ed., 1963) calls quoted literals strings (§2.6).
- **It became the type name.** PL/I had character strings and bit strings. BASIC had string variables. C++ (`std::string`), Java and C# (`String`/`string`), Python (`str`), Go, Swift, Kotlin, Rust, Elm and Lean all use the word. The term was established before any of them existed, so each designer chose the word their readers already knew.

### Where Text is used, and why

| Language | Name | Why |
|---|---|---|
| Pascal | `text` | The type of a text *file*, not a text value (ISO 7185 required type `text`). |
| Haskell | `Data.Text` | The Prelude's `String` is `[Char]`, a linked list. The `text` package adds a packed type and needs a different name for it. |
| Unison, Dhall | `Text` | Newer languages that chose plain words. |
| SQL | `TEXT` | A column type for unbounded text, as opposed to `VARCHAR(n)`. |

The pattern is that `Text` usually marks a contrast: a file versus a value, or packed versus list. Jet's `String` is already packed UTF-8, and D-TEXT-FIXED1=A (ratified 2026-10-30) keeps a single text type. A rename would therefore not distinguish anything.

### What each name would gain or lose in Jet

- **Keep `String`.** Nothing is renamed. The 81,344 mentions across 17,636 Jet files stay as they are (Compiler 36,082, Core 6,730, Examples 2,169, tests 9,488, Jetpack 24,309, Tools 2,566). A newcomer's first guess is right, and `to_string()` keeps the same name as the type. The one cost is that `String` is jargon. Diagnostics already gloss it as `String (text)`.
- **Rename to `Text`.** It is a plain English word. The cost is a cutover of all 81,344 mentions plus the spec, guides and diagnostics. Newcomers from almost every other language would type `String` first and get an error. `to_string()` and related names would also need their own rename vote.
- **Accept both names.** Rejected, because Jet keeps one spelling per thing.

### What changed in the ballot

D-TEXT-NAMES1 is now at v3 (Ballot D-TEXT-NAMES1).

- **Removed:** the branch that also named a second buffer type, because D-TEXT-FIXED1=A ruled out that type.
- **Added:** the history above.
- **Unchanged:** the options (A keep `String`, B rename to `Text`) and the recommendation (A). The history supports A, because nothing in it shows `Text` meaning more than `String` does in Jet.

[^wp]: "String (computer science)", Wikipedia, section History, citing Sammet, *Programming Languages: History and Fundamentals* (1969), and Lewis, *A Survey of Symbolic Logic* (1918).

## Making Jet the best language for extensions, mods and config (#6380)

**Answer.** Jet can offer something no current choice does: static types, effects checked in every signature, and bit-identical math, all in one language that runs from config up to full programs. It loses to Lua today on one thing that matters most: a host program cannot load a Jet script at run time. Every route needs an ahead-of-time build, and the `jet` binary is 81 MB, against Lua's 293 KB. Winning needs four things:

- a small host runtime for checked, portable Jet (the owner's choice, ballot **D-EXT-HOST1**);
- a Neovim bridge;
- working value evaluation for config files (existing cards #6195 and #6029);
- a stated determinism guarantee for pure code.

The full proposal is `Docs/proposals/extension-and-config-language-2026-10-30.md`.

### Evidence in brief

- **The field** (proposal §2). Each incumbent wins on different axes:
  - Lua: embedding, size and startup.
  - Nix: hermetic environments.
  - Starlark, Dhall, CUE, Nickel, KCL and Pkl: typed or total config.
  - WASM hosts: isolation.

  None combines static types, effect-checked sandboxing and a small in-process runtime. That combination is the opening for Jet.
- **What Jet has** (proposal §3). These are ratified, and some are already built:
  - Effects in every signature, with an E3401 purity walk.
  - The sandboxed `core.plugin` with fuel, memory and time limits.
  - Correctly rounded math everywhere (D-MATH-BITS1=A).
  - Typed option priorities (D-JOS-PRIORITY1=A).
  - Environments as typed package.jet data (D-MANIFEST-ONE1=A).
  - `jet config export` to JSON, YAML and TOML (D-CONFIG-EXPORT1=A, #6029).
  - The portable verified form with an interpreter (D-PORTABLE1=A).
- **Measured on dev 03c64ad0c4** (probes in `probes/m6380/`):
  - `jet run` on an 8-line config program takes 286 ms cold and 8 ms with a warm cache.
  - `jet eval` rejects a value-returning `fn run() -> Int` with E1321, rejects a printing `run` with E3401, and reports E2104 "not built yet" for `jet eval '1 + 2'`. A Jet file cannot be evaluated to a value today; #6195 tracks this.

### Owner choice → ballot

Ballot D-EXT-HOST1 passes PLAIN OK and READY, with `losses=[]`. It asks how a host program runs Jet extensions:

- **A (recommended):** a small runtime for checked, portable Jet. An installed `jet` rechecks edited scripts into a cache.
- **B:** the whole compiler inside the host.
- **C:** out-of-process only.

### Gap cards (new)

These do not duplicate #6195, #6029, #6023, #5476, #4195, #6071 or #3600, which the proposal cites as dependencies.

```json
[
 {"title": "Embeddable Jet script runtime: a foreign host loads checked portable Jet at run time",
  "body": "Gap 1 of the #6380 proposal (Docs/proposals/extension-and-config-language-2026-10-30.md). Today D-EMBED1/2 export Jet to a host only after an ahead-of-time jet build; no host can load a .jet script or portable bundle at run time, which is Lua's core appeal. Shape depends on D-EXT-HOST1 (recommended A: small runtime running D-PORTABLE1 portable MIR, with an installed jet rechecking changed sources into a cache). The host API is a Jet trait; grants follow D-EMBED2's single capability contract; reload of a changed script without restarting the host is part of the runtime.",
  "crit": ["C API with create/load/call/reload and a Jet-trait host API; a C host example loads init.jet, calls a hook, edits the file and reloads it", "An effect outside the host API is refused before the script runs, with a test", "Runtime library size and warm load-to-first-call time are measured and published against Lua 5.4 (293 KB / 484 KB) on the same machine", "Native library, component and script routes share one export and type table (D-EMBED2), with a parity test"]},
 {"title": "Neovim bridge: typed Neovim API for Jet and a Jet plugin host",
  "body": "Gap 2 of the #6380 proposal. Neovim embeds LuaJIT and serves every other language over msgpack-RPC with metadata from `nvim --api-info`. Ship a first-party `nvim` package whose bindings are generated from that metadata (typed, completed in the editor) and a plugin host that runs Jet plugins over RPC with event-based lazy loading. After the embeddable runtime lands, add the in-process route through a small Lua shim.",
  "crit": ["Bindings generated from `nvim --api-info` for the installed Neovim, regenerated by one command; a type error in an API call is caught by jet check", "Example plugin (format on save, a user command, a floating window) runs in stock Neovim via the Jet host", "Startup and per-call latency measured against the same plugin written in Lua, results in the package docs"]},
 {"title": "State and test the determinism guarantee for pure Jet code",
  "body": "Gap 4 of the #6380 proposal. Mods in lockstep multiplayer, reproducible config and caches need same-input-same-output beyond math. D-MATH-BITS1=A covers core.math; the spec states nothing about map/set iteration order, hash seeds, float-to-text, sort stability or integer overflow across tiers and platforms (no such statement in spec.md or core-library.md at 03c64ad0c4). Write the guarantee for code with no effects, find and fix any violation, and add a cross-tier, cross-platform test corpus.",
  "crit": ["Spec section states which results of effect-free code are identical across platforms, tiers and compile time, and lists any deliberate exception", "Audit of Core collections, hashing, formatting and sorting with every violation fixed or listed as an exception", "Corpus run on o0/o1/o2/comptime and two platforms with byte-identical output"]},
 {"title": "Guide: Jet for config and extensions, with migrations from Lua, Nix and YAML",
  "body": "Gap 5 of the #6380 proposal. Show the typed Jet result beside each incumbent: an init.lua plugin, a flake.nix/devenv.nix environment (via jetpack import into package.jet environments), and a YAML/TOML app config with its `jet config export` output. Every example is a tested file.",
  "crit": ["Guide page with the three migrations, each example compiled in CI", "Each migration shows what the effect signature and typing catch that the original did not", "Linked from the getting-started index and the Jetpack environment reference"]}
]
```

### Dependencies (existing cards, no new card)

| Card | What it covers |
|---|---|
| #6195 | eval/repl: a Jet file must evaluate to a value |
| #6029 | config export, D-CONFIG-EXPORT1=A |
| #6023 | libjet and component port to the self-hosted toolchain |
| #5476 | hello startup ≤ 2 ms, which matters for plugin managers |
| #4195 | portable sandboxed format, D-PORTABLE1 |
| #6071 / #3600 | beating Nix, and the Nix evaluator port |

## SOLID: what it gets right for Jet, and where it fails (#6381)

**Plain answer.** SOLID is five rules for designing classes. Robert C. Martin
wrote them up in 2000, and Michael Feathers coined the acronym around 2004.
The ideas underneath are sound: keep related code together, keep interfaces
small, make every implementation honour its contract, and depend on what a
thing does, not on how it is built. Jet should keep those ideas. It should
get them from the language and from checkable API rules, not from SOLID's
class rituals. Jet has no inheritance, small traits and effect rows, so most
of SOLID is already built in. The known weaknesses are vague wording, a
class-only view, and a habit of piling on indirection that costs speed and
clarity. One real gap turned up: a user trait cannot be implemented for a
built-in type.

### Where SOLID came from, and the main critiques

- Martin, *Design Principles and Design Patterns* (2000), written about
  "software rot". Feathers later arranged five of its principles into the word
  SOLID [WP-SOLID].
- Martin later restated the single-responsibility rule as: "Gather together
  the things that change for the same reasons. Separate those things that
  change for different reasons." He calls this "just another way to define
  cohesion and coupling" [UB-SRP].
- The open–closed rule comes from Bertrand Meyer's *Object-Oriented Software
  Construction* (1988): modules "should be both open and closed" [UB-OCP].
- The substitution rule comes from Liskov and Wing (1994). A property you can
  prove about a value of type T must also hold for a value of any subtype S
  [WP-LSP].
- **Critique 1: vague.** Martin himself notes the question "What defines a
  reason to change?" Readers asked whether a bug fix or a refactor counts
  [UB-SRP].
- **Critique 2: inside-out.** Dan North's CUPID (2022) replaces SOLID with
  properties a user can see: composable, Unix philosophy, predictable,
  idiomatic, domain-based. He contrasts "doing one thing well", an outside-in
  view, with SRP's "inside-out perspective: it is about the organization of
  code" [NORTH].
- **Critique 3: cost.** Casey Muratori measured "clean code" polymorphism
  against a plain `switch` on one shape-area task. Dropping the virtual calls
  was 1.5× faster, and a table lookup was about 10× faster. When the problem
  grew, the gap reached about 15× [MURATORI].
- **Critique 4: dated.** In 2020 Martin answered a manager who said OCP and LSP
  were out of date "because we don't focus on inheritance nearly as much as we
  did 20 years ago". Martin defended the principles [UB-REL]. The question
  itself shows how much of SOLID was about inheritance.

### Each principle, applied to Jet

Probes ran on dev `59213de00f`. The files are under the lane's
`probes/solid/`.

| Principle | The sound idea | Jet today | What Jet should do |
|---|---|---|---|
| **S**ingle responsibility | Code that changes together lives together | The unit is the module and the API area, not the class. Core API Law 8 says "one way to mean it". Pillar 9 (#6367) splits every area into one primitives layer plus a few wrappers, so the mechanism and the convenience change for different reasons. | Apply the rule to API areas through pillar 9's checks. Do not apply it per type: "one reason to change" cannot be checked, and per-class splitting produces the fragments North describes. |
| **O**pen–closed | You can add behaviour without editing working code | Jet picks per type. Enums are closed: `if x == { … }` must be exhaustive, so a new variant makes the compiler list every site to update. Traits are open: `[Shape]` dispatches dynamically (`s02_dyn.jet` prints `19`). Markers and derives add members without editing the type. | Keep "closed by default, open where a trait is written". It is the honest answer to the expression problem. Fix the gap below, which blocks the most common open extension. |
| **L**iskov substitution | Every implementation keeps the contract callers rely on | Jet has no inheritance, so the rule becomes "every impl keeps the trait's contract". The effect part is checked: an impl that prints inside a `-[]>` trait method is E0742 (`s01_lsp_effect.jet`). Behaviour is not checked: nothing tests that `compare` is a total order or that `equal` is reflexive. | Keep the mechanical checks; #6281 C11 already asks impls to inherit the trait's effect row. Later, Core traits could ship law tests, such as `Comparable` being transitive. No card yet: Jet has no property-testing tool to hang them on. |
| **I**nterface segregation | Callers depend only on the methods they use | Built-in traits are tiny: `Equatable` (`equal`), `Comparable` (`compare`), `Close` (`close`), `Add`/`Sub`/`Mul`/`Div` (one method each), `Encode`/`Decode`. Go makes the same choice ("Interfaces with only one or two methods are common in Go code" [GO]). | Make it a review question for new traits: "Can a caller need only part of this trait?" It fits beside pillar 9's checklist. |
| **D**ependency inversion | High-level code depends on an abstraction, not on a concrete driver | Two Jet tools do this without frameworks. A trait parameter swaps the provider: `stamp(clock: Clock, …)` runs with a `Fixed` clock (`s04_clock_inject.jet` prints `42 hi`). Effect rows state what a function may touch, and `#Replayable` forces time and randomness through a capability (D-REPLAY1). Jet has no runtime effect handlers: "effects are checked and erased". | Keep it explicit: pass the capability or the trait value. Do not add a container or an ambient injection system; Jet's Law 4 already makes dependencies visible in signatures. |

### What to avoid

1. **Indirection by default.** In Jet, writing a trait in type position means
   dynamic dispatch "with invisible boxing" (spec, Traits). Muratori's numbers
   show the cost of making that the default shape. Teach generics
   (`fn f<T: Shape>`) and enums first, and dynamic traits when the set is
   truly open.
2. **Rules nobody can check.** Jet's API rules are "lints first"
   (D-STDRUBRIC1=A). Each SOLID idea that Jet keeps is already, or can be,
   a check: exhaustiveness, E0742, pillar 9's `api` lint group, trait size in
   review. "One reason to change" cannot be checked, so it stays guidance.
3. **SOLID as a named doctrine.** Jet's documents should name the properties
   (cohesion, small traits, substitutability, explicit dependencies) and their
   checks. Citing SOLID as authority would import its class-and-inheritance
   framing, which Jet does not have.

### Link to the API composition principle (#6367)

Pillar 9, "primitives first, pareto wrappers on top", is SOLID's
single-responsibility and open–closed ideas stated for an API area:

- Primitives are the stable mechanism.
- Wrappers can be added without touching the primitives.
- Every wrapper expands into primitives a user can also call.

Unlike SOLID, pillar 9 comes with machine checks (its rules P1–P8 and the
`api` lint group). It is awaiting the owner ballot D-API-COMPOSE1.

### Gap found (card draft in `answers/6381-cards.json`)

- `impl Int.Named { … }`, where `Named` is the module's own trait, is rejected
  with `E0301 The impl target 'Int' is not a registered type`
  (`s03_foreign_impl.jet`). The spec's orphan rule allows it, because the
  module owns the trait. Built-in operator traits on `Int` already work
  (`impl Int.Mul(Price)` in `Examples/features/operators/mixed_types.jet`).
  Without this, the only way to give a built-in type new behaviour is a
  wrapper type. That is exactly the boilerplate #6021 is trying to remove.

No owner ballot is needed. The gap is a defect against the existing orphan
rule, and everything else above is review guidance that fits D-API-COMPOSE1.

### Sources

- [WP-SOLID] "SOLID", Wikipedia (history: Martin 2000; Feathers c. 2004):
  <https://en.wikipedia.org/wiki/SOLID>
- [UB-SRP] R. C. Martin, "The Single Responsibility Principle" (2014):
  <https://blog.cleancoder.com/uncle-bob/2014/05/08/SingleReponsibilityPrinciple.html>
- [UB-OCP] R. C. Martin, "The Open Closed Principle" (2014), quoting Meyer
  (1988): <https://blog.cleancoder.com/uncle-bob/2014/05/12/TheOpenClosedPrinciple.html>
- [UB-REL] R. C. Martin, "Solid Relevance" (2020):
  <https://blog.cleancoder.com/uncle-bob/2020/10/18/Solid-Relevance.html>
- [WP-LSP] "Liskov substitution principle", Wikipedia (Liskov and Wing 1994):
  <https://en.wikipedia.org/wiki/Liskov_substitution_principle>
- [NORTH] D. North, "CUPID — for joyful coding" (2022-02-10):
  <https://dannorth.net/blog/cupid-for-joyful-coding/>
- [MURATORI] C. Muratori, "'Clean' Code, Horrible Performance" (2023):
  <https://www.computerenhance.com/p/clean-code-horrible-performance>
- [GO] "Effective Go", Interfaces: <https://go.dev/doc/effective_go>
- Jet: `Docs/spec/spec.md` (Traits; effect bounds, E0742; runtime effect
  handlers excluded); `Docs/spec/syntax-decisions.md` (D-EXTMETH1 orphan rule,
  `impl Type.Trait`, D-REPLAY1); `Docs/spec/stdlib-api-laws.md` (Laws 4, 8, 9);
  proposal `Docs/proposals/api-primitives-and-wrappers-2026-10-30.md` (#6367).

## Memory freed too late: scope-end cleanup and tail recursion (#6382)

**Plain answer.** The owner is right, and Jet is not doing what its own rules
say yet. D-DROP-ASAP1 (ratified) says a value that holds only memory is freed
right after its last use. Today Jet frees it only at the end of its scope, on
o0, o1, o2 and in the default run. So a dead 200 MB string stays in memory
while the next one is built. A recursive function that hands a new buffer to
itself keeps every old buffer until the whole recursion unwinds, about 390 MB
for 20 buffers of 20 MB. Tail calls are still never jumps (already carded as
#6358). Files, locks and other values with visible cleanup close at scope end,
in the right order, as the rule requires. No rule needs to change. The
implementation needs to catch up, plus two refinements listed below.

### The problem in plain terms

"RAII" means a value is cleaned up when its block ends. That is predictable,
but it holds memory longer than needed. A big value that is no longer used
still occupies memory until the closing brace.

Recursion makes it worse. Each call that passes a new value down keeps its
own old value until the call returns. Nothing returns until the deepest call
finishes, so every generation of the value is alive at once. That pending
cleanup is also what stops the call from reusing its stack frame. The
function cannot jump to itself while it still owes a free after the call.
This is the "overwritten before it is cleaned up" effect the owner describes.

### What other languages do

| Language | When memory is freed | Evidence of how it works in practice |
|---|---|---|
| C++ | Destructors run at "end of scope, for objects with automatic storage duration" [CPP] | Predictable, holds memory to the brace. Tail calls are not guaranteed; Clang added `[[clang::musttail]]` so a call "will not cause unbounded stack growth … even when optimizations are disabled" [CLANG] |
| Rust | "When an initialized variable … goes out of scope, its destructor is run." Assignment drops the old value [RUST-REF] | Smarter borrow checking (non-lexical lifetimes) did not move drops. The explicit tail-call proposal `become` exists because pending drops block the jump. With `become`, "dropping necessarily happens before entering the called function" [RFC3407]. It is still unstable |
| Mojo | "ASAP destruction": "At compile time, Mojo determines the last use of each value. After its last use, the value's lifetime ends" [MOJO] | The model Jet ratified. Even temporaries inside one expression die early |
| Swift | ARC may end a lifetime after the last use. Swift 5.7 set "new rules to shorten the lifetime of variables when optimization is allowed" [SWIFT57] | The same release made "the most common programming patterns that depend on extended variable lifetimes … safe without requiring programmers to explicitly use `withExtendedLifetime`" [SWIFT57]. Early freeing broke code that relied on a visible side effect, so Swift had to fence it |
| Go | Garbage collected. An object can be freed as soon as it is unreachable, even inside its scope | `runtime.KeepAlive` exists "to ensure that the object is not freed … before the point in the program where KeepAlive is called". The documented case is a raw file descriptor read after the last use of its owner [GO] |
| C# / .NET | Same as Go | `GC.KeepAlive` makes an object "ineligible for garbage collection from the start of the current routine to the point where this method is called" [DOTNET] |
| Zig | Manual; `defer` runs at scope exit | `@call(.always_tail, …)` is a request the programmer must write [ZIG] |
| Lua, Scheme | Proper tail calls are part of the language: "the called function reuses the stack entry of the calling function", and "a tail call erases any debug information about the calling function" [LUA] | The debugging cost is why D-TAIL-CALL1 keeps the last 16 tail calls for stop reports |

**The lesson.** Freeing at last use is safe when nothing can observe the free.
The failures seen in Swift, Go and .NET all come from one of two things. Either
a destructor has a visible side effect, or raw code still holds an address the
language does not track. Jet's ratified rule already covers the first: Close,
`#MustFinish`, guards and `defer` keep scope-end timing. The second needs one
more rule (refinement 2 below).

### What Jet does today (probes on dev `59213de00f`)

The probes are in the lane at `probes/drop/`. Each memory probe reads its own
`VmRSS` and peak (`VmHWM`) from `/proc/self/status`. They were built with
`jet build --optimize=none|basic|full` and run directly, so the peak belongs to
the program and not the compiler. The default `jet run` gave the same
`VmRSS` figures. All numbers are kB.

| Probe | What it does | Rule requires | o0 | o1 | o2 | Verdict |
|---|---|---|---|---|---|---|
| `m01_last_use` | `big :: "x".repeat(200000000)`, `n :: big.len()`, then a second 200 MB string | `big` freed after `.len()`: peak ≈ 196,000 | peak 391,636 | 391,632 | 391,668 | **Late**: freed at scope end |
| `m02_live_control` | Same, but `big` is used again at the end | Both alive: ≈ 392,000 | 391,636 | 391,644 | 391,668 | Matches `m01`, so last use is ignored |
| `m03_block` | `big` lives in a helper function that returns `big.len()` | Freed on return: ≈ 196,000 | 196,320 | 196,324 | 196,352 | Correct; also proves freed memory returns to the OS |
| `m04_tail_overwrite` | `step(n, data)` makes a new 20 MB `next` and calls `step(n - 1, next)`, 20 deep | Each dead `data` freed: ≈ 20,000–40,000 at the bottom | 391,648 | 391,652 | 391,684 | **Late**: all 20 buffers alive at once |
| `m07_loop_overwrite` | `buf = "b".repeat(100000000)` four times in a loop | Old value is dead before the new one is built: peak ≈ 98,000 | peak 195,996 (old + new) | not run | 195,808 | **Late** by one value at each overwrite |
| `m05_close_scope_end` | Two Close values `a`, `b` | Closed at scope end, last first | `close b`, `close a` after `end of body` | | | Correct |
| `m06_tail_blocked_lint` | A tail call with a Close value `r` still open | Stays a call, and a lint names `r` | runs, prints `close r1 … r3`; no lint, even with `--verbose` | | | **Missing lint** |
| `m08_walk` | `walk(10000000, 0)`, `is_even(10000000)` | Both finish (tail calls jump) | E3012 | E3012 | E3012 | **Late**: already #6358 |
| `m09_comptime_walk` | `comptime { walk(100000, 0) }` | finishes | `5000050000` | | | ok at this depth |
| `m09b_comptime_walk` | `comptime { walk(10000000, 0) }` | finishes (D-TAIL-CALL1 covers comptime) | E3010 whose message embeds a raw ``Stop [E3012]: Stack overflow in `??` at 0x2000004d628a`` | | | **Late, and a bad diagnostic** |

Key repro (`m04_tail_overwrite.jet`, the owner's case):

```jet
fn step(n: Int, data: String) -> Int {
    if n == 0 {
        mem("bottom")          // prints VmRSS from /proc/self/status
        return data.len()
    }
    next :: "z".repeat(20000000)
    step(n - 1, next)          // `data` is dead here, but is freed only on return
}
```

### Where Jet should go (no rule change needed)

1. **Build D-DROP-ASAP1 for memory-only values.** The liveness information
   exists: `Compiler/JetSema/Source/Sema/Ownership/LastUse.jet` already
   decides moves at last use (D-COPY-DEFAULT1). The drop must move to the same
   point. This also clears the pending drops that block tail calls, which is
   exactly the dependency D-TAIL-CALL1 names.
2. **Refinement 1: overwrite frees first.** For `buf = make()` where the
   right side does not read `buf`, the old value is dead before `make()` runs.
   The rule ("freed right after last use") already requires the free to come
   first. Rust and C++ free it after, so peak memory doubles. Jet can halve it.
3. **Refinement 2: raw addresses keep a value alive.** Go and .NET needed
   `KeepAlive` because raw code held an address the collector did not see.
   Jet's equivalent is `core.mem.address_of` and the string-data leaves inside
   `#Unsafe`. A value whose address is taken there must stay alive until that
   `#Unsafe` block ends. That keeps raw code safe without a new keyword. It is
   an implementation rule inside D-DROP-ASAP1's "view and borrow uses", not a
   new choice.
4. **Finish D-TAIL-CALL1** (#6358). Also add the blocker lint and comptime
   tail calls, which #6358's single criterion does not cover.
5. **Keep scope-end timing for visible cleanup.** Swift's 5.7 correction shows
   why: code that can observe a destructor must see it at a predictable point.
   `m05` shows Jet already gets this right.

### Cards (drafts in `answers/6382-cards.json`)

- **New:** free memory-only values at last use on every level (`m01`, `m04`,
  `m07`), with refinements 1 and 2 and the debug-info free point.
- **Criteria for #6358:** the blocker lint (`m06`), and comptime tail calls
  (`m09b`).
- **New:** comptime stack overflow is reported as E3010 wrapping a raw E3012
  text with `??` and an address (`m09b`).
- **New (found while probing):** `tag` is accepted as a parameter name but
  cannot be read (`fn show(tag: String) { print(tag) }` gives E0003 "Expected
  a value, found `tag`"; `probes/drop/k01_tag_param.jet`).

No ballot: both governing rules are ratified, and the two refinements follow
from their text.

### Sources

- [CPP] cppreference, "Destructors":
  <https://en.cppreference.com/w/cpp/language/destructor>
- [CLANG] Clang attribute reference, `musttail`:
  <https://clang.llvm.org/docs/AttributeReference.html#musttail>
- [RUST-REF] The Rust Reference, "Destructors":
  <https://doc.rust-lang.org/reference/destructors.html>
- [RFC3407] Rust RFC PR #3407, "Explicit tail calls":
  <https://github.com/rust-lang/rfcs/pull/3407>
- [MOJO] Mojo manual, "Death of a value":
  <https://docs.modular.com/mojo/manual/lifecycle/death/>
- [SWIFT57] "Swift 5.7 Released!", Automatic Reference Counting Improvements:
  <https://www.swift.org/blog/swift-5.7-released/>
- [GO] Go `runtime.KeepAlive`: <https://pkg.go.dev/runtime#KeepAlive>
- [DOTNET] .NET `GC.KeepAlive`:
  <https://learn.microsoft.com/en-us/dotnet/api/system.gc.keepalive>
- [ZIG] Zig 0.15.2 reference, `@call` and `CallModifier.always_tail`:
  <https://ziglang.org/documentation/0.15.2/#call>
- [LUA] Lua 5.4 reference manual §3.4.10, "Function Calls":
  <https://www.lua.org/manual/5.4/manual.html>
- Jet decisions D-DROP-ASAP1=A (card #4620), D-TAIL-CALL1=A (card #6343),
  S63 / D-SHAPE-RESOURCE1 (resource cleanup), D-COPY-DEFAULT1.

## Should `fn dev` cover environments, or should there be an env entry point? (#6383)

**Answer.** Neither. Keep two homes with one rule:

- **package.jet's `environments:` field** says what the machine has: tools, services, secrets, and named environments such as `dev` and `ci`. It is data, so Jet can show it, lock it and ask for trust before any project code runs.
- **`fn dev` in dev.jet (optional)** says what `jet dev` does with *your program*: watch, serve, reload.

Folding environments into `fn dev` (devenv-style) or adding a `fn env` entry point would make opening a shell run project code. Each would also split or move data that the one-project-file ruling (D-MANIFEST-ONE1=A, today) just put in package.jet.

This fits D-ENTRY-HOME1 unchanged: a data-only package.jet beside build.jet, test.jet and dev.jet. Ballot: **D-DEV-ENV1** (v2).

### Today in Jet (dev 03c64ad0c4 plus today's rulings)

- **`jet dev` is not a web feature.** It is the general watch loop: it reruns a script or hot-swaps a resident program (D-DEV4/D-DEVMODE1, syntax-decisions.md:6356). An app's `fn run() App` is served by the same loop. `fn dev(d: Dev)` is the expert override (D-ENTRY-VALUE1=B, :6844). For example, `Examples/features/devloop/dev_context/dev.jet` calls `d.watch("assets/")`, `d.serve(port: 4321)` and `d.reload(.Restart)`.
- **Environments are data.** Each named environment has tools, services with readiness probes and `after` ordering, secrets, and a prompt (jetpack-epoch5.md:372). Until today they lived in `module env.<name>` blocks in env.jet. D-MANIFEST-ONE1=A (ratified 2026-10-30) makes them the `environments:` field of package.jet, with `packages:` renamed to `tools:`. `jet fix` moves 159 env.jet files, and `configs:` lets a large team split environments into other files.
- **`jet env` and `jet dev` do different things (D-JPK-DEVCOMPOSE1=D).** `jet env [name]` opens a shell and never runs project functions, behind a first-entry trust gate. `jet dev`, `jet test` and `jet build` run code, each inside the base environment plus the one named after the verb, when it exists.
- **Imports produce data.** `jetpack bridge flake` and `jetpack import` turn flake.nix and devenv.nix into environment data.

One correction to the owner's framing: `fn dev` is not only for web servers. It overrides the watch loop for any program.

### The same project under each option

**A: data in package.jet, loop in dev.jet (recommended).**

```jet
// shop/package.jet: data, never runs
name: "shop"
environments: {
    dev: Environment{ tools: ["nodejs_22", "postgresql"], services: { db: postgres() } }
    ci: Environment{ tools: ["nodejs_22"] }
}

// shop/dev.jet: what jet dev does with the program
fn dev(d: Dev) {
    d.watch("assets/")
    d.serve(port: 4321)
}
// $ jet env    shell with dev tools, nothing runs
// $ jet dev    dev tools, services up, then fn dev
```

**B: `fn dev` covers the dev environment (devenv-style).**

```jet
// shop/package.jet
name: "shop"
environments: { ci: Environment{ tools: ["nodejs_22"] } }

// shop/dev.jet
fn dev(d: Dev) {
    d.tools(["nodejs_22", "postgresql"])
    d.service("db", postgres())
    d.watch("assets/")
    d.serve(port: 4321)
}
// jet env must run fn dev to learn the tools
```

**C: an env entry point.**

```jet
// shop/env.jet
fn env(e: Env) {
    dev :: e.environment("dev")
    dev.tools(["nodejs_22", "postgresql"])
    dev.service("db", postgres())
    e.environment("ci").tools(["nodejs_22"])
}
// dev.jet keeps fn dev; jet env now runs fn env first, so it asks for trust
```

The `postgres()` service preset is illustrative. The registry of built-in presets is described in jetpack-epoch5.md under "Services and readiness".

### Trade-offs

| | A data + loop | B fn dev covers env | C fn env entry |
|---|---|---|---|
| `jet env` on a fresh clone runs project code | no | yes | yes |
| Tools can be locked and shown before trust | yes, data | only after evaluating | only after evaluating |
| CI / test / library with no dev loop | `environments.ci` | stays in package.jet, so it is split | in fn env |
| Target for flake and devenv imports | data, as today | must generate code | must generate code |
| Per-platform tools | a data field | an `if` | an `if` |
| Files touched for a web project | package.jet + dev.jet | package.jet + dev.jet | env.jet + dev.jet |
| Coherent with D-ENTRY-HOME1 and D-MANIFEST-ONE1 | unchanged | package.jet loses the dev environment | reopens D-MANIFEST-ONE1 (env.jet returns) |

### Edge cases

- **A library with no server.** It still needs tools such as a C compiler or `tree-sitter`. A and C have a place for them. B would put them in a `fn dev` for a project that has no dev loop.
- **CI.** It wants the tools without the loop, which is `environments.ci` under A. The `tests/cases/env_hook` fixtures already exercise a CI environment.
- **Trust.** D-JPK-DEVCOMPOSE1=D and D-TRUST-LAUNCH1 promise that entering a shell runs nothing. B and C break that promise, or move the trust prompt in front of `jet env`.
- **One-file scripts.** They have no package.jet and no dev.jet. `jet dev script.jet` still watches and reruns the script under every option.
- **Misplaced setup.** Under A, a tool or service call inside `fn dev` gets a teaching error that names the `environments:` field. `jet dev` also prints the environment it entered. These are A's only additions.

### Ballot

Ballot D-DEV-ENV1 is v2. The v1 file `D-DEV-ENV1.v1.json` placed environments in env.jet, which D-MANIFEST-ONE1=A has since retired. v2 passes PLAIN OK and READY, with `losses=[]`. Options and recommendation are the same as v1.

**Card draft if A wins:**

```json
{"title": "State the package.jet environments / dev.jet rule and teach misplaced setup (D-DEV-ENV1=A)",
 "body": "package.jet's environments: field holds environments as data (tools, services, secrets); dev.jet's optional fn dev configures how jet dev runs the program. Write the rule into the dev-loop guide and the Jetpack environment reference, add a teaching error when a Dev call tries to declare tools or services (naming the environments: field), and have jet dev print which environment it entered.",
 "crit": ["Guide and jetpack-epoch5 reference state the rule with one same-project example of both files", "Registered teaching diagnostic for tool/service setup attempted in fn dev, with an example", "jet dev prints the entered environment (base or base+dev); covered by a CLI test"]}
```

## Bit operations as sigils, and what `~` should mean (#6384)

**Answer.** Yes, bring back short bit operators for integers. We recommend Odin's set:

- `a & b` (and), `a | b` (or), `a ~ b` (exclusive-or).
- `~a` (complement) and `a &~ b` (and-not).
- The update forms `&=`, `|=`, `~=`, `&~=`.

Bool logic keeps `!`, `&&`, `||` and `!=`. One tilde serves for both complement and exclusive-or, because complement is exclusive-or with all ones.

We chose `~` over `^` for exclusive-or because ported Lua, Julia or MATLAB code that uses `^` for power would otherwise silently change meaning. This choice reopens two recent rulings: D-SIGIL-ALGEBRA1=B (bit methods, 2026-10-04) and D-SIGIL-FREED2=A (`~` stays free, 2026-10-11). The card also names D-MATH-BITS1, but that ruling covers correctly rounded `core.math`, and no option touches it. Ballot: **D-BIT-SIGILS1**.

### Today in Jet (dev 03c64ad0c4)

- **Bit operations are methods.** Every integer type has `bit_and`, `bit_or`, `bit_xor` and `bit_not` (spec line 227; syntax-decisions.md:1022). Shifts stay `<<` and `>>`.
- **Infix operators are rejected.** Infix `&`, `|` and `^` on numbers give E0398, whose fix rewrites them as method calls.
- **Other uses of the symbols.** Prefix `&` and `^` are ownership marks. `!` negates Bool only. Copying is `.copy()`, so `~` no longer copies, and today `~` is a plain E0003.
- **Where single `|` is used.** It belongs to alternatives: or-patterns and type unions (D-SHAPE-PIPE1=C).
- **How much code uses the methods.** 2,595 method calls in 465 files: Compiler 1,523, Core 669, Jetpack 195, tests 107, Examples 91, Tools 10. `jet fix` can rewrite them by reversing E0398's structured repair, which keeps the grouping.

### Survey: spelling bit operations

| Language | and | or | xor | complement | and-not | notes |
|---|---|---|---|---|---|---|
| C, C++, Java, C#, JS | `&` | `\|` | `^` | `~` | `a & ~b` | `^` was never power |
| Python | `&` | `\|` | `^` | `~` | `a & ~b` | `**` is power |
| Rust | `&` | `\|` | `^` | `!` | `a & !b` | `!` is both not and complement |
| Go | `&` | `\|` | `^` | `^a` | `&^` | one sign for xor and complement |
| Odin | `&` | `\|` | `~` | `~a` | `&~` | one tilde for xor and complement |
| Lua 5.3+ | `&` | `\|` | `~` | `~a` | — | `~=` is not-equal |
| Julia | `&` | `\|` | `⊻` / `xor` | `~` | — | `^` is power |
| Zig | `&` | `\|` | `^` | `~` | — | |
| OCaml, Ada, Fortran | `land`/`and`/`iand` | `lor`/`or`/`ior` | `lxor`/`xor`/`ieor` | `lnot`/`not`/`not` | — | words |
| Jet today | `bit_and` | `bit_or` | `bit_xor` | `bit_not()` | `a.bit_and(b.bit_not())` | words, D-SIGIL-ALGEBRA1=B |

### Candidate meanings of `~`, with examples

| Meaning | Where | Example | Fit for Jet |
|---|---|---|---|
| Bit complement | C family, Python, Odin, Lua, Zig | `~mask` | **A, B.** Most readers expect it. |
| Exclusive-or (infix) | Odin, Lua 5.3 | `h ~ b` | **A.** Avoids the old power meaning of `^`. |
| Logical not | MATLAB | `~done` | **B.** Duplicates `!`. |
| Not-equal (`~=`) | Lua, MATLAB | `a ~= b` | **B.** Duplicates `!=`. |
| Concatenate / append (`~=`) | D, Raku | `s ~ t`, `s ~= t` | No: Jet concatenates with `+` and interpolation. |
| Pattern match | awk `~`, Perl `=~`, PostgreSQL `~` | `line ~ /GET/` | No: Jet has `Regex` and `is_match`. |
| Formula / "similar to" | R, mathematics | `y ~ x` | No use in Jet. |
| Copy | Jet until October | `~input` | Retired by D-COPY-WORD1. |
| Destructor, lazy pattern, label | C++ `~T`, Haskell `~pat`, OCaml `~x` | | No. |

### Same program, three options

```jet
// A (recommended): bit sigils for integers
perms := READ | WRITE | EXEC
perms &~= WRITE                  // drop write
flip :: perms ~ EXEC             // toggle exec
inverse :: ~perms
loop b in data -> hash = (hash << 5) ~ b
if perms ~= READ {}              // error: `~=` updates; to compare write `!=`

// B (your literal reading): ~ is the one not, ~= is not-equal
perms := READ | WRITE | EXEC
perms &~= WRITE
flip :: perms.bit_xor(EXEC)
if perms ~= READ -> print("more than read")
if ~done -> retry()

// C (today): words, ~ free
perms := READ.bit_or(WRITE).bit_or(EXEC)
perms = perms.bit_and(WRITE.bit_not())
flip :: perms.bit_xor(EXEC)
loop b in data -> hash = (hash << 5).bit_xor(b)
```

### Trade-offs and edge cases

- **Precedence (A).** Follows C: shifts first, then `&`/`&~`, then `~`, then `|`. All of them bind tighter than comparisons, which avoids C's `a & b == c` trap, as D-BITOREXPR1 already pinned.
- **Infix `&` next to the prefix `&` mark.** Bit operands are plain integers and never take an ownership mark, so `a & b` and `&list.push(x)` cannot share a position. Infix `|` exists only in value position, which keeps it apart from type unions and or-patterns.
- **Infix `^` stays an error (A).** It now names both `~` and `.pow()`, so neither a C habit nor a power habit runs silently.
- **Lua and MATLAB `~=` habits (A).** `~=` is a statement, so a `~=` inside a condition fails to compile with a fix that names `!=`.
- **Bool (A).** `~flag` on a Bool is an error naming `!`. Exact `Int` complement stays `-x - 1`, and fixed widths keep their width.
- **Cost of B.** It also renames about 19,588 `!=` and about 2,271 prefix `!` uses, and it leaves exclusive-or as a word.

### Ballot

Ballot D-BIT-SIGILS1. PLAIN OK, READY, `losses=[]`. It states in its detail that A or B reopens D-SIGIL-ALGEBRA1 and D-SIGIL-FREED2, and that D-MATH-BITS1 is not affected.

**Card draft if A wins:**

```json
{"title": "Bit sigils for integers: & | ~ &~ and updates (D-BIT-SIGILS1=A)",
 "body": "Add value-position infix & | ~ &~, prefix ~ complement, and compounds &= |= ~= &~= for every integer type, with C precedence above comparisons. Retire bit_and/bit_or/bit_xor/bit_not; flip E0398 to teach the sigils; jet fix rewrites the 2,595 method calls in 465 files preserving grouping. Infix ^ stays an error naming ~ and .pow(); ~ on Bool names !; ~= in a condition names !=. Amend D-SIGIL-ALGEBRA1=B and D-SIGIL-FREED2=A rows in syntax-decisions.md and the lexical ledger.",
 "crit": ["Parser, sema and all tiers accept the sigil set on Int and fixed widths with C precedence; fixtures cover precedence vs comparisons", "jet fix migrates every bit_* call in the tree with grouping preserved; bit methods removed", "Teaching errors: infix ^, ~ on Bool, ~= in a condition, each with a structured fix", "Spec, core-library reference and syntax-decisions updated; Examples/features/math bit examples rewritten"]}
```

## Zig's `@` builtins, and what Jet should take from them (#6385)

**Plain answer.** Zig's `@` functions are built into the compiler. They are
things ordinary Zig code cannot write itself: importing a file, reading or
building a type, stopping the build with a message, casts, CPU instructions.
The `@` keeps them in their own namespace. Zig never lets a name hide another,
so without the `@` every new builtin would break someone's variable. Jet
already covers almost all of them, but it splits them three ways: `$` facts,
`#` markers, and ordinary Core methods. That split is better than Zig's single
bag, so Jet should keep it. What Jet should take is the *list*: four
ratified pieces are not built yet, and one common operation is missing.

### What they are

Zig's reference says: "Builtin functions are provided by the compiler and are
prefixed with `@`" [Z-REF]. Zig 0.15.2 has 117 of them. Zig 0.16 has 124
(counted from the reference's headings; the list is in the lane at
`research/zig/builtins-master.txt`). They fall into nine families:

| Family | Examples | Why it must be built in |
|---|---|---|
| Modules | `@import`, `@embedFile`, `@cImport` | The compiler resolves files and packages |
| Types as values | `@TypeOf`, `@typeInfo`, `@Type` (0.16: `@Int`, `@Struct`, …), `@This`, `@typeName` | Types are compile-time values only the compiler can inspect or make |
| Fields and declarations by name | `@field`, `@hasField`, `@hasDecl`, `@FieldType`, `@fieldParentPtr` | A string names a member, resolved while compiling |
| Compile-time control | `@compileError`, `@compileLog`, `@inComptime`, `@setEvalBranchQuota` | They talk to the compiler itself |
| Source facts | `@src`, `@returnAddress`, `@frameAddress`, `@errorReturnTrace` | Only the compiler or the runtime knows them |
| Casts | `@as`, `@intCast`, `@truncate`, `@bitCast`, `@ptrCast`, `@floatFromInt`, … (about 20) | The target type comes from context ("result type") |
| CPU operations | `@addWithOverflow`, `@clz`, `@popCount`, `@byteSwap`, `@sqrt`, `@mulAdd`, atomics, `@Vector`, `@shuffle`, `@prefetch` | Each maps to one machine instruction |
| Code generation | `@setRuntimeSafety`, `@branchHint`, `@call(.always_tail, …)`, `@setFloatMode`, `@export`, `@extern` | They change how code is emitted |
| Targets | `@wasmMemoryGrow`, `@workItemId`, C varargs (`@cVaArg`, …) | They exist only on one target |

**Why the `@`.** Zig forbids shadowing: "Identifiers are never allowed to
'hide' other identifiers by using the same name" [Z-REF §Shadowing]. If
builtins were plain names, each new one would break every program that already
used that name. The `@` prefix removes the clash for good (an outside account
reaches the same conclusion: [PD]). It also tells the reader that the call
may not behave like a function. It may take a type, infer its target type from
context, or never return.

**The set keeps changing.** Zig's own release notes show the cost of a large
built-in surface:

- **0.11** renamed every `@xToY` cast to `@yFromX` (nine names). It also
  dropped the target-type argument from casts, so `@intCast(u8, x)` became
  `@intCast(x)`. `zig fmt` rewrote old names automatically [Z-0.11].
- **0.12** replaced `@errSetCast` with `@errorCast` and `@fabs` with `@abs`
  [Z-0.12].
- **0.14** replaced `@setCold` with `@branchHint` and removed `@fence`
  [Z-0.14].
- **0.16** removed `@Type`: "in practice, it was clunky to use for common
  tasks", so it became eight builtins such as `@Int` and `@Struct`. It also
  deprecated `@cImport` in favour of the build system [Z-0.16].

The automatic rewrite is what made the renames survivable.

### How each family maps to Jet today

All probes were run with the published dev compiler `59213de00f`. The files
are under the lane's `probes/zig/`.

| Zig | Jet today | Probe result | Verdict |
|---|---|---|---|
| `@import` | `use math :: core.math`, `use "file.jet"` | works (z08) | Equal |
| `@embedFile` | `embed_file("path")`, `embed_bytes`, `find("glob")`, each hashed into `.jet/lock` | `TEXT :: comptime { embed_file(…) }` works (z20) | Jet is stronger: inputs are recorded |
| `@cImport` | the C import model in the FFI spec section; Zig is moving it to the build system too | not probed | Same direction as Zig 0.16 |
| `@sizeOf`, `@alignOf`, `@typeName` | `T.$layout.size`, `T.$layout.alignment`, `T.$name` inside `comptime { }` | prints `8`, `8`, `P` (z03b) | Equal |
| `@typeInfo` | `T.$fields`, and `T.reflect()` in a derive body | `$fields` works (z03c); `P.$fields.len()` inside `comptime` fails with a false message (**defect 1**) | Equal, with a bug |
| `@TypeOf` | none; inference and generics cover the uses | `x.$type` → E0302 (z14) | No gap claimed |
| `@field` by string, `@hasField` | template splices `self.$field` (D-NAME-SPLICE1=B); no string lookup by design | n/a | Jet is deliberately typed |
| `@Type` / `@Struct` | `compiler.generate(name, value)` adds checked items | not built for user code (#3520) | Covered by #3520 |
| `@compileError` | a `panic` or failed check inside `comptime` stops the build with E3010 and the message | `Error [E3010]: table too big` (z04) | Equal for messages |
| `@compileError` with a code and fix | `reject(SomeDiagnostic{…})` with what, why and fix (D-MARKER-DIAG1=A) | `reject` → E0102 "Nothing named `reject`" (z05) | **Gap 3: ratified, not built** |
| `@compileLog` | none | n/a | Not needed; `jet inspect expand` shows computed values |
| `@inComptime` | `$phase` (D-PREP-SURFACE2, D-META-ROOT3) | E0107 "Nothing named `$phase`", fix "Declare it first: `$phase :: ...`" (z06, z06b) | **Defect 2: ratified, not built, wrong fix** |
| `@src` | `$here` and `$caller` (D-JAI-SITE1=A) | E0003 "`$here` is not a compiler fact" (z01, z02) | Ratified, not built; carded as #5010 |
| `@errorReturnTrace` | failure reports list the lines an error passed through (#6014 C1, met) | not exposed as a value | Carried to the #6377 answer |
| casts: `@as`, `@intCast` | checked `U8.from_int(x)` returns `U8?`; numbers take their type from use (D-INFER-RULE1, open) | `U8.from_int(300) ?? U8{0}` prints 0 (z08) | Equal, and safer |
| `@truncate` | **none**. Core instead holds 12 private copies of `fn to_u8(n: Int) -> U8 { U8{n.bit_and(255)} }` | `U8.truncate(300)` and `U8.wrap(300)` both report "Nothing named `U8` exists here" (z09) | **Gap 4** |
| `@addWithOverflow` and friends | `checked_add`, `wrapping_add`, `saturating_add` methods | all work (z08) | Jet is clearer |
| `@clz`, `@ctz`, `@popCount` | `leading_zeros`, `trailing_zeros`, `count_ones` | work (z08) | Equal |
| `@byteSwap`, `@bitReverse` | none | E0311 (z10) | Small gap, not carded (no caller needs it yet) |
| `@bitCast` (float bits) | `math.to_bits`, `math.from_bits` | functions exist | Equal |
| `@sqrt`, `@mulAdd`, `@min`, `@max` | `math.sqrt`, `math.fma`, variadic `min`/`max` | work (z08) | Equal |
| atomics | `Atomic<Int>` | used by `Examples/features/concurrency/bounded_spsc_atomic.jet` | Equal |
| `@setRuntimeSafety(false)` | no per-block opt-out: `#Unchecked { }` → E0927 (z12); the open ballot D-UB-BOUNDARY1 proposes exactly this block | n/a | Owner choice already on a ballot |
| `@branchHint`, `@setFloatMode` | none | n/a | Not carded; see the speed-of-light card #6343 |
| `@call(.always_inline)` | `#Inline fn` | works (z13) | Equal |
| `@call(.always_tail)` | every tail call is a jump (D-TAIL-CALL1=A) | not built; carded as #6358 | See the #6382 answer |
| `@export`, `@extern` | `#Export(c)` and the foreign-function model | `#Export(c) fn add` runs (z19) | Equal |
| `@Vector`, `@workItemId` | `#Kernel`, compute launch (D-COMPUTE-KERNEL-SURFACE1=B) | not probed | Covered by the GPU cards |
| `@panic`, `@trap` | `panic("msg")`; no `unreachable()` (z17) | n/a | Equal for user code |

### What Jet should take, and what to avoid

1. **Keep Jet's three-way split.** Zig puts operations that are really ordinary
   functions (`@sqrt`, `@popCount`, `@min`) in the compiler bag. That is part
   of why its list keeps changing. Jet makes them methods and Core functions,
   so they appear in completion and documentation like everything else. Only
   compiler facts get `$`, and only rules about code get `#`. This already
   matches pillar 9 (#6367): a small set of primitives, with ordinary
   functions on top.
2. **Build the four ratified pieces that are missing.**
   - `$here` and `$caller` (#5010).
   - `$phase` (defect 2).
   - `reject(Diagnostic)`, so a library's compile-time check can give its own
     error code (gap 3).
   - The false comptime-effect message (defect 1).
3. **Add one truncating conversion (gap 4).** Twelve copies of the same helper
   in Core show that a real need has no public route. Spell it like the
   existing wrapping arithmetic.
4. **Ship renames with automatic fixes.** Zig's renames were survivable
   because `zig fmt` rewrote old names. Jet's rule that retired spellings
   teach their replacement, with a `jet fix` edit, is the same tool. Keep it
   mandatory.
5. **Avoid** string-named member access (`@field(x, "name")`) and a single
   type-constructing builtin (`@Type`). Zig itself retired `@Type` as clumsy.
   Jet's typed splices and `compiler.generate` are the better path.

### Defects and gaps found (card drafts in `answers/6385-cards.json`)

1. Calling `P.$fields.len()` inside `comptime` reports `E3410 'a signal
   handler' is a Tier-2 comptime effect`. `[1, 2, 3].len()` in the same
   position works (probe `z03d_len.jet`).
2. `$phase` is ratified and listed in the metaprogramming reference, but
   reading it gives `E0107 Nothing named '$phase'`. The fix line tells the
   user to declare a `$` name, which the language forbids. The same reference
   (line 188) still says fact reads use "the `@` member form", although `@`
   is retired and the examples use `$`.
3. `reject(…)` from D-MARKER-DIAG1=A is not built (`E0102`).
4. There is no public truncating integer conversion; Core keeps 12 private
   copies of `to_u8`. An unknown static method on a number type, such as
   `U8.truncate`, reports "Nothing named `U8` exists here".

No owner ballot is needed: every item above is either ratified or a small
addition inside an existing method family.

### Sources

- [Z-REF] Zig language reference, 0.15.2 and master, "Builtin Functions" and
  "Shadowing": <https://ziglang.org/documentation/0.15.2/#Builtin-Functions>,
  <https://ziglang.org/documentation/master/>
- [Z-0.11] Zig 0.11.0 release notes, "Rename Casting Builtins", "Cast
  Inference": <https://ziglang.org/download/0.11.0/release-notes.html>
- [Z-0.12] Zig 0.12.0 release notes, "@errorCast", "@abs":
  <https://ziglang.org/download/0.12.0/release-notes.html>
- [Z-0.14] Zig 0.14.0 release notes, "New @branchHint Builtin, Replacing
  @setCold", "Removal of @fence":
  <https://ziglang.org/download/0.14.0/release-notes.html>
- [Z-0.16] Zig 0.16.0 release notes, "@Type Replaced with Individual
  Type-Creating Builtin Functions" (proposal #10710), "@cImport Moving to
  Build System": <https://ziglang.org/download/0.16.0/release-notes.html>
- [PD] P. Dubroy, "Zig shadowing and builtins" (2025-06-08):
  <https://github.com/pdubroy/til/blob/main/zig/2025-06-08-Zig-shadowing-and-builtins.md>
- Jet: `Docs/spec/spec.md` "Compile-time names and staged syntax";
  `Docs/spec/reference/metaprogramming.md`; `Docs/spec/syntax-decisions.md`
  (D-COMPILER-NS1, D-LAYOUT-FACTS1, D-NAME-SPLICE1); decisions D-JAI-SITE1,
  D-MARKER-DIAG1, D-UB-BOUNDARY1 (open), D-TAIL-CALL1.
