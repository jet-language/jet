# Jonathan Blow on error reporting, locations and stack traces: ideas for Jet (2026-10-30)

Card #6377. Source: "Why Compilers Bury the Real Error", Jonathan Blow Clips,
uploaded 2026-10-15, 12:37 long (<https://www.youtube.com/watch?v=ZF1m1DsW3Rg>).
It is a clip of Blow's stream <https://youtube.com/watch?v=MnctEW1oL-E&t=8847s>.
The source registry check found no earlier mining (`check_sources.py`: status `new`).

## How this was mined

- **Capture.** yt-dlp took the info JSON and the YouTube auto captions.
  The `en-orig` track is the original English speech.
  The `en` track is a machine re-translation and was used only as a cross-check.
  Comments were not captured.
  Capture root: the mining run for #6377 (local capture, not kept), with `transcript-orig.txt`.
- **Coverage.** Four segments cover 00:00 to 12:37 without gaps: 37 ledger items and 29 claims (23 micro, 6 macro).
  `check_coverage.mjs` passes (exit 0). A fresh-context reviewer ran the recall audit and it passed (see the end).
- **Jet state.** The `jet-state.mjs` digest is `digest.json` (master 1eef4640ad, dev 03c64ad0c4, Tower revision 86436).
  Probes are in the card's scratch probe set (trace/, not kept in the repository).
  They ran with the published dev compiler `jet` 03c64ad0c4, under `jet check` or `jet run`.
  t06 also ran as a built binary with `--backtrace=full`.

## The ideas, in talk order

Each row gives the timestamp, the idea, what Jet does today and what Jet should do.
"Probe" means a program was run on the current compiler. "Source" means the code or record was read but not run.

### 1. Parsing past the first error (00:00–03:40)

| At | Idea in the talk | Jet today | What Jet should do |
| --- | --- | --- | --- |
| 00:16 | Error reporting is a big part of a compiler's user interface. | Agrees. Every diagnostic has a code, What, Why, Fix and a `More: jet-lang.dev/e/…` line (D-DIAG-URL1=B). Probe t01. | Nothing new. |
| 00:30 | One early mistake in C gives a cascade of about 37 unrelated errors. | Mostly immune. In t01 one unknown name, used by three later lines, gives exactly one E0107. A known cascade remains: E0119 then E0112 (#6360). | Finish #6360. |
| 00:44 | Experienced C users read only the first error; IDE lists that hide the real cause fail everyone else. | **Defect.** In t03 (a missing `}`), the first error printed is a follow-on E0003 ("found the keyword `fn`"). The real cause, E0083, prints second. | Card #6397: print the root cause first and drop its follow-on. |
| 01:25 | After an undeclared name, stop checking the function; it cannot make sense. | Jet keeps checking but stays quiet about poisoned values (t01). | Nothing new. |
| 01:53 | "Poison values": anything built from an unknown name is poison and reports nothing. | Built, in effect (t01). | Nothing new. Keep it as a rule for new checks. |
| 02:18 | Poisoning costs effort across the whole type checker, so Jai skips it. | Jet already paid that cost. | Do not copy Jai's shortcut. |
| 02:33 | Jai reports only the first error per file, but all files. | Better. t02 reports two independent unknown names from two functions of one file. | Keep. The owner wants all errors by default (#4914). |
| 02:53 | Compilation is parallel, so "reset at the next function" is unsound. | Jet checks bodies on compiler threads, and each body has its own diagnostic list (source: `CheckProgram.jet:2699-2702`). That answers the objection. | Nothing new. |
| 03:16 | First error only is "mostly fine", but listing all errors will matter. | The owner already decided: report all errors (#4914). Four collect-all gaps are open there. | Finish #4914. |

### 2. Source locations, unclosed braces (03:40–07:36)

| At | Idea in the talk | Jet today | What Jet should do |
| --- | --- | --- | --- |
| 04:03 | Every intermediate form, such as the syntax tree, should link each piece back to its source. | Ratified: D-BIG-CAUSE1 (done) and D-BIG-DIAGORIGIN1 (#3202, ready). | Keep. Note the comptime gap below. |
| 04:23 | Compilers say "end of file" for an unclosed `#if` without saying where it opened. | Jet's E0083 names the opener line and marks the end of the file (source: `State.jet:615-646`; probe t03). | Nothing new. |
| 04:36 | Locations get lost where a later stage drops the link. | **Defect.** In t04 a compile-time `panic` in `check_size` (line 4) is called from line 10. The E3010 report points only at `comptime {` (line 9). It shows neither the panic line nor the call. | Card #6398: a compile-time stop points at the stop and lists the compile-time calls. Related: #6393 (found by #6382). |
| 05:01 | Locations also matter for debugging the compiler itself. | Internal compiler errors already name file, line, column and function (source: `Docs/audits/jit-aot-parity-2026-08-31/gate.log:996`). | Nothing new. |
| 05:22 | Keeping locations costs trivial memory today. | Diagnostics carry a byte span. Not measured here. | No action. |
| 06:01 | Treat the report as an interface and go beyond locations for some errors. | Same as 00:16. | — |
| 06:25 | For an unclosed brace, pointing at the open brace helps little: any missing `}` could be the one. The symptom may show up at top level. | **Defect.** In t03 the `if` on line 9 lacks its `}`. E0083 blames `fn run() {` on line 7, and its suggested edit adds `}` at the end of the file. | Card #6397. |
| 07:12 | Editors' indentation shows where the brace is missing; the compiler could use that. | Not done. In t03 the `}` at column 1 closes an `if` opened at column 5. That mismatch points straight at line 9. Jet's formatter fixes indentation, so this signal is reliable in Jet code. | Card #6397: use the indentation mismatch to name the likely opener and place the fix. |
| 07:30 | Jai passes all location information to the user's metaprogram. | **Gap.** `$here` and `$caller` are E0003 "not a compiler fact" (t05, t09; D-JAI-SITE1 ratified, #5010). No ratified fact gives a declaration's location. `$origin` is a different fact: the `#Track` value-origin fact (D-COMPILER-NS1). So t10b's `null` for `Point.$origin` is correct today. | #5010 as planned. #3520 C24: compile-time code reads any declaration's location as a SourceLocation record; #3520 picks the spelling. |

### 3. Metaprogram checks and stack traces (07:36–11:56)

| At | Idea in the talk | Jet today | What Jet should do |
| --- | --- | --- | --- |
| 07:40 | The metaprogram gets syntax tree nodes so it can check the program for errors. | Ratified but unbuilt. D-HOOK-STAGES1=A is the codebase-rule channel: `#On(compiler.declared)` and `#On(compiler.checked)` hooks return `compiler.finding(d, msg)`. D-JAI-EVENTS1=A adds `settled` and `finished` stages. D-META-COMPILER1=A's `Checked` event gives the function body; its `Error` event observes diagnostics. | Keep. |
| 07:57 | Example codebase rule: "don't hold an entity pointer across a frame boundary". Its report says why and links to a web page. | Two ratified channels, both unbuilt. Hook findings are message-only L1401 warnings on a declaration, with no link; only a team deny policy makes them E1293 errors. `reject(SomeDiagnostic)` (D-MARKER-DIAG1=A, #6039, built by #6388) has what, why and fix, but no link; t11 gives E0102. Every compiler diagnostic prints `More:`. | Ballot D-DIAG-LIBLINK1: may a library error carry its own link? Ballot D-REJECT-AT1: may a hook finding be a full Diagnostic error? |
| 08:14 | Project-specific checks are something a compiler author will never ship for you. | Agrees with Jet's direction (library `Diagnostic`, `#Allow(name)`). | Deliver #6039. |
| 08:24 | Such a report must point at the right node's place in the code. | **Gap in ratified law.** D-MARKER-DIAG1 puts a `reject` report's primary span at the marker's use site. A hook finding sits on its whole declaration. Neither can point at one call inside a body. | Ballot D-REJECT-AT1: hook findings and `reject` share one Diagnostic report with an optional `at:` span. |
| 08:36 | A metaprogram can export data to a visualizer; clicking a node opens the editor at that spot. | Partly possible. `fn build` can write files under declared authority. But no fact gives a declaration's location (t10b), so the editor jump fails. | Same #3520 C24 as 07:30. |
| 09:02 | Jurassic Park banter. | — | Out of scope. |
| 09:35 | Some debugger abilities belong in the language. | Jet splits them. Failure trails live in the runtime (t07c); write watches live in the debugger (`Dap03.jet:315-321`). | Keep the split (see 12:00). |
| 09:47 | A compile-time option gives a stack trace from anywhere, mostly in debug builds. | Ratified, unbuilt, and without a contract: D-JAI-CONTEXT1=A adds `context.trace` ("stack trace") to the context. t08: `context` is E0107. | #5025 C6 and C7 (below). |
| 09:56 | It costs almost nothing; the cost is spread over calls. | No number is given. Spreading the cost still means a cost on every call. | #5025 C7: zero measured cost when unread, against the pre-change commit. |
| 10:05 | The trace does not rely on the OS or linker and is identical on every OS. | **Gap.** Jet writes and reads its own debug info (source: `Fault.jet:12-36`), but uses it only for native faults. A `panic` three calls deep prints only the innermost frame, even when built with `--backtrace=full` (t06). Failures, by contrast, print every hop (t07c). | Card #6399: a panic report lists the Jet call chain. |
| 10:23 | Mechanism: the hidden context points to a linked list of trace nodes (call line, depth). | Jet does not need a list on every call. Its frame-pointer walk over its own debug info costs nothing until it is read (`Fault.jet:222-237`). | #5025 C6: one shared frame source for panics, faults and `context.trace`. |
| 10:47 | Packing the trace means copying the nodes to an array and re-pointing the links, because the stack goes away. | In Jet this is just an owned list. | #5025 C6: reading the trace gives an owned list of the ratified `SourceLocation` record. |
| 11:23 | The packed trace is data: write it to a file, with stable names, file and line per frame, and types. | Failure trails already show function, file:line and the source line (t07c); Tower records #6014 C1 as met (08f32b2d24). The header says "3 hops via ?" although the program wrote no `?`. | #6014 C4: fix the "via ?" wording. |

### 4. Data breakpoints (11:56–12:37)

| At | Idea in the talk | Jet today | What Jet should do |
| --- | --- | --- | --- |
| 12:00 | A language-level data breakpoint: run a function when a chosen value is written, without a separate tool. | The debugger already supports write data breakpoints on Jet local paths: up to 32, write access only (source: `Dap03.jet:315-321`, not exercised). | No new language construct. A write hook in the language would add a hidden effect to every write. Keep it in the debugger. |
| 12:28 | Stack traces were one step toward moving debugger features into the language. | Same as 09:35. | — |

## Other defects the probes found

- **E1321 recommends a spelling the compiler refuses.**
  t07: `fn run() -> Int LoadError!` gets E1321: "returns `()` or `() ?`".
  t07d: writing `fn run() -> () ? {` gets the same E1321.
  t07b: writing `fn run() -> () LoadError!` gets E0003, "A unit-fallible signature has no `->`".
  The accepted form is `fn run() LoadError!` (t07c).
  The text comes from `Compiler/JetSema/Source/Sema/Outputs.jet:307`. Card #6400.
- **E0047 doubles the backticks around a type name.**
  t10 prints "but this is ``OriginInfo``" (from `Places.jet:867,890`). This is the same class as #4028's L0303 item under #3720.
- **E3001's Why text uses internal words** ("The shared report boundary owns the program-side stop", "(I2)").
  #3720's CI guard against invariant tags in runtime stop rows already covers it; t06 is a witness.

## Probe list

| Probe | What it shows | Result on jet 03c64ad0c4 |
| --- | --- | --- |
| t01_cascade | Unknown name used by three later lines | 1 problem (E0107) |
| t02_two_fns | Two unknown names in two functions | 2 problems (E0107 ×2) |
| t03_unclosed | `if` body never closed | E0003 at 15:1 first, then E0083 blaming line 7 |
| t04_comptime_panic | Compile-time panic in a helper | E3010 with the message, pointing at `comptime {` 9:5 |
| t05_site / t09_caller | `$here`, `$caller` | E0003 "not a compiler fact" |
| t06_panic_trace | Panic three calls deep | E3001 with one frame (`inner`, line 4), under both run and `--backtrace=full` |
| t07c_fail_trace | Failure passed up three functions | Root cause plus "Trail [E3002] (3 hops via ?, origin first)" with three lines |
| t07 / t07b / t07d | Entry return spellings | E1321 / E0003 / E1321 |
| t08_context_trace | `context.trace` | E0107 "Nothing named `context`" |
| t10b_decl_origin | `Point.$origin`, `helper.$origin` | `null`, `null`, `false`, `null` |
| t11_reject | `reject(Diagnostic)` | E0102 "Nothing named `reject`" |

## Owner questions answered

| Owner asked about | What Jet has today | What this run adds |
| --- | --- | --- |
| Debug info | Jet walks its own debug info for native faults (`Fault.jet:12-36,222-237`). The debugger has write watches on Jet paths (`Dap03.jet:315-321`). | Nothing new; a language-level data breakpoint is not adopted (12:00). |
| Stack traces | Failures print every hop (t07c). Panics print one frame (t06); `backtrace` (#5829) covers native faults only. | Card #6399 for full panic traces; #5025 C6/C7 for a typed, free `context.trace`. |
| Metaprogram access to trace and error info | D-META-COMPILER1's `Error` event hands handlers each diagnostic. `context.trace` is ratified only as "stack trace". No declaration location fact exists. | #5025 C6 types the trace; #3520 C24 asks for a declaration location fact. |
| Metaprograms adding errors | `reject` is ratified but unbuilt (E0102 today; #6388). Hook findings (D-HOOK-STAGES1, D-JAI-EVENTS1) are ratified warnings with no link. | Ballots D-REJECT-AT1 (one channel, any span, real errors) and D-DIAG-LIBLINK1 (own link). |

## Recall audit

A fresh-context reviewer ran the audit. Seed `6377-r1-kestrel` sampled segments 01 (00:00–03:40) and 02 (03:40–07:36).
The reviewer re-mined each segment from `transcript-orig.txt` before reading the ledger: 22 concepts, 0 misses, 0 disputed mappings.
`recall-audit.json` is in the capture root, and `check_coverage.mjs --audit` exited 0.
