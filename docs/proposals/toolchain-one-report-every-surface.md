# The toolchain speaks once: one report, every surface

Status: proposal for owner decision. Ballots D-REPORT-* on the audit card. Date: 2026-08-07.

## Executive summary

The failure rethink asked what a failure is. The owner ratified the answer (D-FAIL-MODEL1): every runtime failure is one product — a report — delivered on one of three routes. That founded half of what Jet says to humans. This audit swept the other half: compile diagnostics, warnings, test failures, build and config errors, formatter and CLI complaints, jetpack, the JSON output, the editor, and the browser. Nobody founded that half, and it shows.

The sweep found one excellent product and a crowd of imitations. The compile diagnostic — `Error [E0102]:` with an arrow, a caret, a Why, and a Fix — is registered, snapshot-tested, and explainable. It is the best thing Jet says. Around it:

- six competing error styles across the tools, and 184 uncoded `error:` prints in the CLI;
- a test runner that prints `left: 1, right: 2` with no file, line, or caret — while the same check in production code gets a full frame;
- a golden-test helper that returns a bare `bool`, so a missing file and a mismatch are silently the same;
- three incompatible JSON shapes, plus a fourth documented in the spec that exists only in a dead file;
- an editor that receives the What but never the Why or the Fix, and a quick-fix engine that scrapes prose with a pattern match;
- a registry that is a 252 KB markdown file re-parsed from scratch on every `jet explain` call, its text in two unsynchronized copies — 328 codes have no what/why/fix at all, and nothing checks that the spec's words match the compiler's words.

The one idea: **everything Jet says to a human is one report — one registry row, one text home, one frame — rendered by one renderer per surface.** The failure rethink proved this for the runtime moment. This proposal completes it: the compile moment, the test moment, and the tool moment are the same product; the terminal, the browser, the editor, and the machine are four renderers of the same row. A test failure is not a new kind of output — it is the report that reached the test boundary. A web error is not a bare JavaScript throw — it is the same report the terminal would print. An editor squiggle is not a shortened message — it is the full report, shown the editor's way.

This deletes machinery instead of adding it. One registry with typed rows replaces the markdown-plus-inline-strings split brain. One frame replaces six house styles. One JSON schema replaces three live shapes and one dead file. One structured fix field replaces prose scraping. The metaprogramming rethink already ratified the landing zone: rules declared in the compile-time program, and `reject` minting user reports with the same code/what/why/fix shape. The registry becomes one more table in that one program — the table the owner's slate already said should exist, minus the one table nobody balloted: the diagnostics themselves.

Six ballots (D-REPORT-*) ask direction-level questions. Each stands alone. What does not change: the ratified compile frame word-for-word, the ratified `Stop` report and its routes (all 11 D-FAIL-*), the lint override law (D-LINTPOLICY1), `jet explain`, the snapshot discipline (I4), and the walls — I2 (rustc hidden) and I9 (one meaning on every tier).

## Glossary

- **Report** — the one product: a registered code, a What in plain words, a Why, a Fix, a source location when one exists, and an optional cause chain. Ratified for runtime failures by D-FAIL-MODEL1; this proposal makes it the shape of everything Jet says.
- **Row** — one registry entry: the code, the severity, the moment, the message templates (What/Why/Fix with holes for values), and the optional structured fix.
- **Moment** — when the report is caught: compile, run, test, or tool (the toolchain acting on your project: build setup, fmt, jetpack, doctor).
- **Surface** — where the human is: terminal, browser, editor, or machine (JSON for CI and tools).
- **Renderer** — the one program per surface that turns a row plus values into what the human sees. Renderers own layout, never meaning.
- **Frame** — the rendered shape: severity word, code in brackets, the sentence, the arrow, the caret line, Why, Fix.
- **Template with holes** — message text written once in the row with named holes (`the list has {len} items…`); the emit site supplies values. Plain message templating — not the derive-body law (D-META-CODE1), which governs generated code, not prose.
- **Ledger line** — a test runner's one-line pass/fail row (`name: FAIL`). A ledger line is a table of contents, not a report; the report prints under it.

## The one idea

**Everything Jet says to a human is one report from one registry, rendered by one renderer per surface.**

The beginner story: you learn one sentence shape, once. The message that stops your build, the message that stops your program, the message under a failed test, and the message in your browser console are the same shape with the same voice. Every one carries a code, and `jet explain CODE` answers for all of them. You never learn a second error language because there is no second one.

The expert story: you get one machine surface for everything. One JSON schema for compile, test, fmt, and package output. The editor gets the full report — Why, Fix, a link to the explanation, and a structured edit instead of a scraped one. Your own code mints reports on the same rails: `reject` in a compile-time rule, `Err` with a code at run time, both carrying the same fields the compiler's own reports carry. The registry is a table your program can read.

## Evidence: the shadow systems

One product, many imitations. Each row is a mechanism doing the report's job with its own home and its own defect.

| # | Mechanism | Home | Defect |
|---|---|---|---|
| 1 | Registry = markdown | `crates/jet-cli/src/Explain.rs:13` (`include_str!` of `docs/spec/diagnostics.md`, 252 KB) | re-parsed on every `explain` call; "retired" detected by substring |
| 2 | Text in two copies | ~1771 `Diagnostic::error` call sites vs 420 markdown what/why/fix rows | nothing checks they match; 328 codes have no what/why/fix; 63 prose rows have no registry row |
| 3 | Spec→code hole | `tests/diagnostics_coverage.rs:645` | checks emitted ⊆ registered only; E0903, E3626, E3628 are registered ghosts; ~32 codes exist only as fixture text |
| 4 | Six error styles | registry frame; `error:`+`fix:` lowercase (184 sites in `Source/`); jetpack `  error[E1230]:` (`crates/jetpack/src/Output.rs:428`); `what:/why:/fix:` (`crates/jet-rt/src/__gc.rs:432`); `panic:` frame; bare lines (`crates/jetpack-bin/src/main.rs:65-203`) | six voices for one product |
| 5 | Test assertion | `crates/jet-codegen/src/Codegen/TIR/emit/helpers.rs:433-435` | prints `left: 1, right: 2` — no file, line, caret; the non-test path has all three and the test-mode branch drops them. D-FAIL-BREACH1 already binds `require`/`require_eq` into the one stop family, so this branch is ratified-law debt, not an open design |
| 6 | Golden helper | `crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:1052` | returns bare `bool`; missing file and mismatch are the same silent false |
| 7 | Test machine output | `Codegen/mod.rs:2517` | `--json` does not cover test results at all. (The `JETTEST2` binary record is `jet prove` coverage evidence, D-JPROOF1 — it stays; the gap is that no *report* surface exists for tests) |
| 8 | Three live JSON shapes | `Diagnostics.rs:539` (envelope), `:444` (`jet.diagnostic/v1`, one variant, `relatedSpans` hardcoded `[]`), Canvas (`graph_helpers.rs:596`) | plus 10 hand-rolled envelope sites; `CmdImport.rs:783` says `"what"` where the schema says `"message"` |
| 9 | Dead JSON module | `Source/DiagnosticsJSON.rs` (144 lines, never `mod`-declared) | the spec's "Machine-readable diagnostics" section documents a schema only this dead file implements, while naming the live serializer as its source |
| 10 | Editor gets What only | `Source/LSP/Server.rs:786` | Why and Fix never reach the editor; no related info, no explain link |
| 11 | Fix by prose scraping | `Diagnostics.rs:154` (`attach_teaching_edit`) | the quick-fix edit is parsed out of the Fix sentence with a pattern match; prose is load-bearing protocol |
| 12 | Explain pointer missing | 2 of ~85 coded eprintln sites print "run `jet explain …`" | a code with no path to its essay |
| 13 | Degraded explain | `tests/cli/explain_E0102.txt` | 328 codes explain as a one-line meaning and a stage — no What/Why/Fix |
| 14 | Severity split brain | `Diagnostics.rs:57` (`Severity::{Error,Lint}`), `Source/Compiler.rs:855` (a second copy), three code prefixes (E/L/frozen W0410) | no ratified severity law; the enum, the prefixes, and the LSP mapping each freelance — ballot D-REPORT-SEV1 ratifies the law |
| 15 | Web runtime silence | `Codegen/Web.rs:4569` (JS index → silent `undefined`), `:4913-4968` (bare `throw new Error`), `:4857` (raw wasm panic) | D-FAIL-EDGE1 ratified the fix; the dev overlay still shows build errors only (`WebHost.rs:1312-1450`) |
| 16 | Exit-code freelancing | jetpack: 0 uses of `ExitCodes`, raw `return 2` ×224 across the jetpack crates for everything; `jet fmt` exits 2 on a parse error where `jet check` exits 1 | one table, two tools that ignore it |
| 17 | Error pages | `docs/reference/errors/` | 18 pages generated of ~749 registered codes |

Supporting counts: 749 unique codes in the spec tables; 690 emitted from Rust; 31 stale `.stderr` fixtures (card #1601); 857 UI snapshots holding the one good product to its word.

## The model

Three axes, one law — the failure model's axes, extended to all speech.

**Axis 1 — moment: compile / run / test / tool.** When the report is caught. The failure rethink ratified that compile and run are the same product at two moments (D-FAIL-BREACH1). Test and tool are moments three and four, not new products.

**Axis 2 — severity: blocking or advisory.** What the report demands. An error, a stop, and a test failure block; a warning advises. Severity is metadata on the row; policy can move it (D-LINTPOLICY1), never reword it.

**Axis 3 — surface: terminal / browser / editor / machine.** Where the human is. One renderer per surface. Renderers marshal; the registry owns meaning — I9's "dumb engines" rule, applied to talkers.

**The law: if Jet says it to a human, it is a report: one registered row, one text home, rendered by the surface's renderer — never a bare string.** Status lines (progress, success, ledger lines) are not reports and stay plain.

Ratified law, re-read as theorems of this model:

- I4 (no snapshot, no diagnostic) — the law at the compile moment.
- D-FAIL-BREACH1 (one renderer for every stop, every tier) — the law at the run moment.
- I2 (rustc hidden) — only registry text may reach a human; rustc's voice is not a report, so it never renders. The ICE banner is Jet's one report about itself.
- I9 (tier parity) — one meaning per report on every engine; the dev overlay already ships this law for one surface: "the exact same `message` string the terminal's parity line renders — same poll, same words, cannot drift" (`WebHost.rs:1312`).
- D-LINTPOLICY1 — severity is row metadata a team can move; the text and the code stay.
- D-FACT-WORD1 (tighten and loosen) — one vocabulary across diagnostic copy is checkable only if the text has one home. With rows, it is one lint over one table instead of a hunt across 1771 strings.
- The metaprogramming law ("a marker exists only as a registry row", D-VERDICT-1455-1) — a diagnostic exists only as a registry row, and `reject` mints user reports with the same code/what/why/fix signature (D-META-USER1).

The "ohhh" connections:

1. A failed test already prints a report — it just prints the wrong half. `require` in production code renders a full frame with file, line, and caret; the same `require` under `jet test` throws the frame away and prints `left: 1, right: 2`. The test runner is an engine re-encoding meaning, which is exactly what I9 forbids engines to do.
2. The editor, the docs site, `jet explain`, and the quick-fix engine each rebuilt a partial copy of the registry because there is no single typed home. Four consumers, four partial copies, zero checks between them.
3. A quick fix is not a sentence to scrape. The Fix prose and the text edit are two renderings of one structured fact that belongs in the row.
4. The spec's JSON section documents a dead file. The shipped schema, the documented schema, and the crypto schema are three answers to a question the owner was never asked.
5. The metaprogramming slate moved every rule table into the one compile-time program — markers, planes, rights, build facts — and its own evidence table flagged the diagnostic registry as the same split brain ("two sources, kept in sync by a manual checklist"). It was diagnosed and never balloted. This is the missing table.
6. `Err` values carry a `code` field (D-FAIL-ERROR1), `reject` carries code/what/why/fix (D-META-USER1), and the compiler's diagnostics carry code/what/why/fix. User space and compiler space already agreed on the shape; nobody said it out loud.

## The surface

Before/after pairs from real output. Each item is marked ratified, amended, or new.

### 1. One frame, every moment — extends ratified (I4, D-FAIL-BREACH1)

The compile frame is untouched — it earned its place:

```text
Error [E0102]: nothing named `pirnt` exists here
  --> app.jet:2:5
    |
  2 |     pirnt("hi")
    |     ^^^^^
 Why: only functions that have been defined (or built in, like `print`) can be called
 Fix: did you mean `print`?
```

The runtime frame is ratified and owed (D-FAIL-BREACH1, cards #1530-#1536):

```text
Stop [E3010]: the list has 3 items, so position 10 doesn't exist
  --> app.jet:12 in run
 Why: reading past the end of a list has no answer to give.
 Fix: check the position first, or use `list.get(i)` for a maybe-value.
```

New in this proposal: the test moment and the tool moment join the frame.

**Test — before (today, `helpers.rs:435`):**

```text
totals: FAIL
  left: 101, right: 100
1 passed, 1 failed
```

No file. No line. No caret. Which of the six `require_eq` calls in the test failed? Run it in your head.

**Test — after (frame owed by ratified law; presentation and word are ballots D-REPORT-TEST1 and D-REPORT-SEV1):**

```text
totals: FAIL
  Stop [E3001]: expected 100, got 101
    --> tests/orders.jet:14 in totals
      |
   14 |     require_eq(total(cart), 100)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
1 passed, 1 failed
```

Most of this is already law. D-FAIL-BREACH1 binds a failed `require` into the one stop family (E3001), and the ratified failure model names a test body as a boundary — the runner contains the stop, records the ledger line, and continues. The test-mode branch that throws the frame away is that law's unpaid delivery, not an open question. What the ballots ask: how the contained report is presented (TEST1) and which word heads it (SEV1). The golden helper also stops returning a bare `bool` and produces a report, with "no golden file exists yet" and "the output changed" as two different messages instead of one silent `false`.

**Tool — before (today, six styles at once):**

```text
error: can't find the file `app.jet`
 fix: check the spelling, or run jet from the folder that contains it
  error[E1274]: no build log for `web`
error[E2110]: automatic memory management failed
 what: ...
build-plugin manifest changed after verification
```

**Tool — after (proposed): one frame.** The 184 uncoded `error:` prints and jetpack's private style adopt the registry frame; each real failure gets a row; a spelling like `can't find the file` keeps its exact words as the What. Trivial usage echoes (`usage: jet explain <CODE>`) stay plain — they are instructions, not reports.

```text
Error [Exxxx]: can't find the file `app.jet`
 Fix: check the spelling, or run jet from the folder that contains it.
```

(`Exxxx` stands for a newly allocated code — the existing numbers stay where the registry put them.)

### 2. One text home: the registry is a table, not an essay — new (D-REPORT-HOME1)

Today the text lives twice: inline Rust strings render, a markdown table explains, and a manual six-step checklist keeps them honest. It has not: 328 codes explain with no prose, three codes are registered ghosts, and `jet explain` re-parses 252 KB of markdown per call.

Proposed: one registry of typed rows — code, severity, moment, What/Why/Fix templates with named holes, optional structured fix. Emit sites reference the row and supply hole values. The spec chapter, the `docs/reference/errors` pages, and `jet explain` are generated from the rows, so every code gets a generated page and the words cannot drift because there is nothing to drift from. The metaprogramming slate decides the row's final home (declarations in the one compile-time program, D-META-REG1's table); this ballot decides that the registry is rows, not prose.

```jet
// proposed — an illustration of a row, not a spelling. The declaration
// word and final form land with D-META-REG1's one table and need their
// own Syntax.rs row and decision ID (I7) before any code ships.
report E3010 {
    severity: .Stop
    what: "the list has {len} items, so position {index} doesn't exist"
    why:  "reading past the end of a list has no answer to give."
    fix:  "check the position first, or use `list.get(i)` for a maybe-value."
}
```

The snapshot discipline does not weaken: every row still has UI snapshots (I4), and the coverage test finally checks both directions — every emitted code has a row, and every row is emitted or explicitly marked retired or reserved. The E0903/E3626/E3628 ghosts surface on day one.

Two honest costs. First, generation gives every code a *page*, but the 328 codes with no Why/Fix prose still need that prose written — a visible authoring backlog with its own card, not a free gift. Second, the compiler needs the rows before it can compile the program that declares them: at stage 0 the build generates a typed Rust table from the declarations (the same embed-and-rebuild pattern the Prelude uses today), and that generation step dies at self-hosting (#813).

### 3. One machine voice — amended (D-DX1; deletes two shapes and a dead file)

Before — a CI script parsing Jet output today needs four parsers:

```text
{"schema_version":1,"diagnostics":[...]}          # jet check --json
{"schema":"jet.diagnostic/v1","code":"E2702",...} # crypto only, preempts the above
{"code":...,"rendered":...}                       # Canvas problems panel
JETTEST2<binary>                                  # jet test proof report
{"command":"fmt","status":"dirty","files":[...]}  # jet fmt --json (one of three fmt shapes)
```

After — proposed: one report schema on every command that talks JSON:

```text
$ jet test orders.jet --json
{"schema":"jet.report/v1","moment":"test","severity":"stop","code":"E3001","what":"expected 100, got 101","why":"...","fix":"...","detail":null,"file":"tests/orders.jet","line":14,"col":5,"span":{"start":210,"end":238},"fix_edits":[],"cause":[]}
```

One shape for compile, run (dev loop), test, fmt, and jetpack. Ledger and status payloads (`"status":"ok"`, pass counts) keep their small command envelopes; every *report* inside any envelope is this one object. The two extra live shapes are deleted; `Source/DiagnosticsJSON.rs` and its orphan goldens are deleted; the spec's JSON chapter is regenerated from the real schema. Gate audits (D-FACT-GATE1) and future inspect surfaces emit their findings in the same shape.

Field honesty against today's shipped shape (D-DX1): `detail` stays; `message` is renamed to `what` to match the registry's field names; the spec's promised `suggestions` array (many fixes per report) is kept as `fix_edits`, not narrowed to one; the `schema` tag replaces the bare `schema_version` number. Every delta is enumerated in the ballot. One records gap: D-DX1 is ratified in spec prose but has no board record, so the MACHINE1 ratification also mints the record it amends.

### 4. The editor gets the whole report — new (D-REPORT-EDITOR1)

Before — the editor shows the What and nothing else (`Server.rs:786`): no Why, no Fix, no link, and the quick-fix edit is scraped out of the Fix sentence by pattern match.

After — proposed: the LSP payload carries the full report: What as the message, Why and Fix as the detail the editor shows on hover, a code link that opens the explanation, and the structured fix from the row as the quick-fix edit — the same edit `jet fix` applies, no scraping. Related locations ride the standard related-information field instead of a hardcoded empty list.

### 5. The browser is a surface, not an exception — ratified delivery (D-FAIL-EDGE1), one new piece

Before — today in the browser console:

```text
Uncaught Error: divided by zero        # no code, no location, no Why, no Fix
undefined                              # a JS out-of-range list read — no error at all
```

D-FAIL-EDGE1 already ratified the fix for the program edge: the same report, delivered in the target's native shape. The one piece this proposal adds: `jet dev`'s overlay — which already renders compile reports verbatim — renders runtime reports too. The page that showed you the build failure shows you the stop, same words, same code, dismiss with Escape.

### 6. The explain pointer is part of the frame — new (folds into D-REPORT-LAW1)

Today 2 of ~85 coded tool errors tell you `jet explain` exists. Proposed: the terminal renderer owns the pointer line — toolchain commands print it once per run after the last report. Sites stop deciding; the renderer decides. Scope and exits: only toolchain commands print it — a shipped release binary's stop report omits it, because its user may not have `jet` at all — and a `--no-hints` flag turns it off for scripts that want bare frames.

### 7. Deletions

- The markdown-as-registry: `docs/spec/diagnostics.md`'s tables become generated output of the row registry; the manual sync checklist dies.
- `Source/DiagnosticsJSON.rs` (dead), the `jet.diagnostic/v1` one-variant shape, the Canvas private shape, the 10 hand-rolled JSON envelope sites, and the orphan goldens that encode text no longer in the source.
- The six house styles: jetpack's private frame, the `what:/why:/fix:` outlier in `jet-rt`, the 184 uncoded `error:` prints, and the second `DiagnosticSeverity` enum in `Source/Compiler.rs:854`.
- `attach_teaching_edit` prose scraping — retired row-by-row as structured fixes are authored; it runs on every diagnostic today, so it is deleted only when the last scraped pattern has a structured replacement. The authoring count (codes whose Fix matches the scraped patterns) is measured in Phase A and carded.
- Raw exit ints in jetpack — `ExitCodes` becomes the one producer, per the ratified exit law (D-FAIL-EXIT1). (The `JETTEST2` record stays — it is `jet prove` evidence, not a report surface; JSON test reports are additive.)
- The Canvas private shape's consumer migrates with it: `crates/jet-canvas/src/js/diagnostics-query.js` (including the `window.__jetCanvasProblems` export) reads `jet.report/v1` in the same Phase C change that deletes the shape.

## Beginner magic, expert control

The ladder. Each rung is opt-in; no rung changes what the rung below does.

**Rung 0 — type nothing.** Every message from every Jet tool has one sentence shape: a severity word, a code in brackets, the What, an arrow to your code, Why, Fix. Colors follow your terminal; `NO_COLOR` and `--color=never` turn them off; the plain output is byte-identical minus color. Nothing to configure, nothing to learn twice.

**Rung 1 — ask.** `jet explain E0102` — any code from any moment, compile or runtime or test or tool. Same essay the docs site shows, because both are generated from the same row.

**Rung 2 — machine output.** `--json` on any command: one schema, `jet.report/v1`, every report from every moment. Pipe it to `jq`, a CI annotator, or an editor plugin without per-command parsers.

**Rung 3 — team policy.** `pkg.jet` `policy.lints.deny` (ratified D-LINTPOLICY1): a team promotes a warning to blocking. Severity moves; the code and the words stay; the bypass is spelled and recorded.

**Rung 4 — mint your own.** Your error values carry codes on the value route (`Err("config rejected", code: "CFG404")`, ratified D-FAIL-ERROR1). Your compile-time rules reject with the full shape (`reject(code:, what:, why:, fix:)`, ratified D-META-USER1). Your reports render in the same frame, explain the same way, and ride the same JSON.

**Rung 5 — read the table.** The registry is rows in the compile-time program. An expert reads it the way they read any table, lints it (the D-FACT-WORD1 vocabulary check becomes one query), and tooling builds on it without a markdown parser.

Ceremony creep check: no default gained a marker. The common case — write code, read one error shape — is unchanged except that more surfaces honor it. Magic without an exit check: every default here has all three exits — colors follow `NO_COLOR`/`--color`, the pointer line follows `--no-hints` (proposed), JSON is opt-in, and severity policy is project-level and spelled.

## What it looks like

One bug, four surfaces, today and after. The program:

```jet
fn total(cart: [Int]) => Int {
    return cart.sum() + fee(cart[10])      // cart has 3 items
}
```

| Surface | Today | After |
|---|---|---|
| Terminal (`jet run`) | `panic: index out of bounds: the index is outside the list` — no location | the `Stop [E3010]` frame above, identical on AOT, JIT, and interpreter (ratified, owed) |
| Test | `totals: FAIL` / `  left: 101, right: 100` | the ledger line plus the full report with file, line, caret (proposed) |
| Browser | `Uncaught Error`, or silent `undefined` | the same report as a typed error object in the console; the dev overlay frames it like a build error (ratified edge + proposed overlay) |
| Editor | a squiggle reading "the list has 3 items, so position 10 doesn't exist" and nothing else | the same squiggle; hover shows Why and Fix; the code links to the explanation; the quick fix offers `list.get(i)` as a real edit (proposed) |
| CI | four parsers | one `jq` filter (proposed) |

## What this unlocks

- **Beginners.** One error language across the whole toolchain. The first failed test teaches the same shape the first compile error taught.
- **Teaching and docs.** All 749 codes get generated pages and one text home — today 18 pages exist. The 328 codes with no Why/Fix prose become a visible, carded authoring backlog instead of a hidden one.
- **Editors and tools.** Full-fidelity reports over LSP and one JSON schema make third-party integrations one afternoon instead of one reverse engineering project per command.
- **CI.** Structured test failures with codes and spans — annotate the exact line in a pull request without regex archaeology.
- **The vocabulary laws.** D-FACT-WORD1 (tighten/loosen) and every future copy rule become single queries over one table instead of sweeps over 1771 strings.
- **Self-hosting.** The self-hosting card (#813) needs diagnostics ported; porting one typed table is a task, porting a markdown parser plus six styles is a rewrite.
- **The web tier.** Runtime reports in the overlay close the last visible I9 gap a browser user can see.

## What stays

- The compile frame, word-for-word. Phase A (the registry migration) preserves every existing message string byte-for-byte — snapshots prove it. The balloted Phase C style collapse then rewords the six tool styles into the frame; those rewordings are the point, and they are snapshot changes the owner sees.
- All 11 D-FAIL-* rulings and their cards (#1527-#1536) — this proposal is the same law at the remaining moments, not a change to those.
- I2: rustc stays hidden; the ICE banner (with rustc's verbatim stderr, by design) stays Jet's one report about itself.
- I4's snapshot discipline, extended to the new moments: a test-failure report and a tool report need snapshots like any diagnostic.
- D-LINTPOLICY1's override law; `jet explain`; the exit-code table and the ratified exit law (D-FAIL-EXIT1).
- `NO_COLOR` / `FORCE_COLOR` / `--color` precedence and byte-identical plain output (`Terminal.rs:26`).
- Silent deopt (E2211 retired): tier changes are not reports; experts keep `--trace-tiers`.

## Decisions for the owner

| Ballot | Question | Recommends |
|---|---|---|
| D-REPORT-LAW1 | Adopt the law: everything Jet says to a human is one registered report, one renderer per surface? | adopt |
| D-REPORT-HOME1 | The registry becomes typed rows with one text home; spec, docs, and explain are generated from it? | rows in the compile-time program |
| D-REPORT-SEV1 | The severity law: which words and levels label reports, and what heads a stop the test runner contains? | two severities; reuse `Stop`/`Error` |
| D-REPORT-TEST1 | How a failed test presents the report the boundary caught: full frame, compact line, or a flag? | full frame, same renderer |
| D-REPORT-MACHINE1 | One JSON schema (`jet.report/v1`) for every command; the other shapes are deleted? | one schema, JSON lines |
| D-REPORT-EDITOR1 | The editor receives the full report: Why, Fix, explain link, structured fix edit? | full report |

Each ballot stands alone; any subset can be adopted. Ratified decisions a ballot amends are named inside its text (D-DX1 for MACHINE1; the spec's I4 process prose for HOME1; none are contradicted silently).

## Implementation shape

**Phase A — internal re-founding, no surface change.** Build the row registry and generate the spec tables, explain data, and error pages from it; every existing message string preserved exactly; both-direction coverage checks turn on; the ghosts and stale fixtures surface as their own cards. Delete the dead file and orphan goldens.

**Phase B — land the ratified-but-unbuilt on the substrate.** The D-FAIL delivery cards (#1530-#1536) build their one runtime renderer against the row registry, so stop reports are rows from day one. The 31 stale fixtures (#1601) re-bless against generated text.

**Phase C — balloted surface unifications, each a coherent greenfield migration.** Test reports, the one JSON schema, the editor payload, the tool-style collapse, the overlay's runtime pane. Each deletes its replaced form in the same change.
