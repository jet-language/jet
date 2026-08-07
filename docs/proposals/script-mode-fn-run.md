# Script mode and fn run

Proposal, 2026-08-07. Owner-approved direction. Two ballots decide adoption shape (D-ENTRY-SCRIPT1) and the script visibility law (D-ENTRY-ORDER1).

## Executive summary

A `.jet` file of bare statements is a script: `jet run file.jet` runs them. Script mode is sugar — the bare statements are the body of an implicit `fn run()`. One mechanism (I8), two spellings, one meaning on every tier (I9). A notebook cell and a REPL line are bare code under this same law, so this is the substrate for both.

Three owner rulings are baked in as law: an imported script never runs; bare code plus an explicit `fn run` in one file is an error with an auto-wrap fix; diagnostics nudge promotion to `fn run` and never punish the script rung.

One choice is open, and it is the heart of the second ballot: what a name can see inside a script file. Strict top-to-bottom, or declarations visible file-wide with statements in order.

## The on-ramp, today vs proposed

Today, rung zero costs a ceremony line and its closing brace:

```jet
// hello.jet — today
fn run() { print("Hello World") }
```

```text
$ jet run hello.jet        # without the wrapper, today:
Error [E0003]: expected `fn`, `#Test`, `struct`, or `comptime` here, found the name `print`
```

Proposed — create file, type the statement, run:

```jet
// hello.jet — proposed
print("Hello World")
```

```text
$ jet run hello.jet
Hello World
```

## The design

**The law.** Bare statements in a file are the body of an implicit `fn run()`. Declarations stay declarations. `jet run` treats the implicit entry exactly like an explicit one: same sema, same diagnostics, same meaning on AOT, JIT, interpreter, and web (I9). This spelling desugars, so nothing about `fn run` changes:

```jet
// script spelling                 // means exactly
name :: ask("Name?")               fn run() {
print("Hello {name}")                  name :: ask("Name?")
                                       print("Hello {name}")
                                   }
```

Precedent: Jetpack already synthesizes a `fn run { task(…) }` wrapper for a selected `#Job fn` (D-JPK-TASKRUN1 shipped work). U7 stays law: a lone script never needs a manifest.

**The verb rhyme.** `jet run` ↔ `fn run` (S12), `jet build` ↔ `fn build` (D-BUILDSCOPE1), `jet dev` ↔ `fn dev` (U19). Script mode adds a second spelling of `fn run` only. The other entry fns are declarations and stay legal inside a script file. D-ENTRY-SCRIPT1 option B extends the rhyme: `jet test file.jet` runs the script's `#Test` blocks, `jet dev file.jet` hot-reloads it.

**Rung 1 — promotion.** The promotion step is moving the loose code inside `fn run()`. You take it when the program grows past a toy: typed CLI args (D-CLIFLAG1), failure with `fn run() => () ?` (D-S80-RUN1), tests, or a second file. Docs and diagnostics point at this step; nothing forces it.

**Mixing rule (owner ruling 2).** Both bare code and `fn run` in one file is an error with an auto-wrap fix:

```text
Error [E0621]: this file has both loose code and `fn run`
 Why: a file has one entry; loose code is already an implicit `fn run`, so this file names two
 Fix: move the loose code into `fn run` — `jet fix hello.jet` does it for you
```

`jet fix file.jet` performs the wrap: loose statements move into `fn run` in written order — statements above the declaration go before its current body, statements below go after. This reuses the one ratified rewrite authority (D-REL5: only `jet fix` rewrites user source); the LSP quick-fix applies the same rewrite. When no `fn run` exists yet, the same action wraps the loose code into a new `fn run()` — the one-keystroke promotion.

**Rung 2 — multi-file and the import rule (owner ruling 1).** "NEVER run an imported script." A file with bare code can be run, never imported:

```jet
// tools.jet                        // app.jet
print("cleaning…")                  use "./tools"     // error below
clean_all()
fn clean_all() { … }
```

```text
Error [E0620]: `tools.jet` is a script, so importing it is not allowed
 Why: importing must never run code by surprise; a script's loose code runs only when you run the file directly
 Fix: move the loose code into functions — then `use "./tools"` imports those functions normally
```

The exact rule: a file with any bare statement is a script. A script executes only as the direct target of `jet run` / `jet dev` / `jet test`. Every other consumption path — `use`, package module discovery — reports E0620. Remove the last bare statement and the file is an ordinary module; its `pub` functions import normally. The import spelling itself belongs to the open D-NAME-FILES1 slate (card #1625); this rule attaches to whichever spelling wins there.

**What a script may contain.** Bare statements plus any top-level declaration: `fn`, `struct`, `#Test`, `use`, `#Known` constants, `#Job` fns, `fn build`, `fn dev`. Only an explicit `fn run` conflicts with bare code (E0621), because both claim the entry.

**One shared rule under both ordering options.** A bare binding is a local of the implicit `fn run` body. A top-level `fn` never sees it — otherwise scripts would grow hidden mutable globals:

```jet
tax :: 0.2
fn total(price: Float) => Float { price * (1.0 + tax) }   // error below
print(total(10.0))
```

```text
Error [E0622]: `tax` is script code, so `total` cannot use it
 Why: loose bindings live in the script body; a function sees only its parameters and file-wide declarations
 Fix: pass `tax` as a parameter, or lift it to a `#Known` constant
```

```jet
#Known tax :: 0.2                                          // fixed: file-wide constant (S57)
fn total(price: Float) => Float { price * (1.0 + tax) }
print(total(10.0))
```

## The open choice: what a name sees inside a script

Jet's current file law is order-independent: a top-level declaration is visible anywhere in its file, before or after its line (verified against the shipped compiler; the same law is stated for generic-module lookup, E0855 text, and D-MODCOMPUTE1 field evaluation). Statements inside a function body run in written order. A script file holds both kinds, so it must pick a visibility law. Three real options, worked on programs a beginner actually writes.

**P1 — helper below its use:**

```jet
greet("Ada")
fn greet(name: String) { print("Hello {name}") }
```

**P2 — two helpers calling each other:**

```jet
print(even(10))
fn even(n: Int) => Bool { if n == 0 { true } else { odd(n - 1) } }
fn odd(n: Int) => Bool { if n == 0 { false } else { even(n - 1) } }
```

**P3 — a binding defined mid-file:**

```jet
print("start")
limit :: 10
print(limit * 2)
```

### Option A — strict top-to-bottom

A name exists only after its line, declarations included. The story is one sentence: the file runs like you read it.

- P1 fails: `Error [E0623]: `greet` does not exist yet on line 1 — Fix: move `fn greet` above its first use`.
- P2 is impossible: whichever helper comes first names the other too early. No fix exists except promotion to a module file.
- P3 works.

Costs. Reordering code changes meaning, so refactors break scripts. Mutual recursion cannot be written. Promotion becomes a semantic change: the same two declarations that were order-sensitive in the script become order-free the moment they move to a module file — the law flips underneath the user mid-ladder. Notebook cells inherit Python's hidden-state fragility: a cell works or fails depending on which cells ran first, for functions too.

### Option B — declarations file-wide, statements in order

Declarations follow the existing file law (visible anywhere); bare statements run top to bottom as the implicit body. This adds zero new law: declarations already work this way in every Jet file, and statements already run in order in every function body. The desugar picture is exact — declarations stay at file level, statements become the body.

- P1 works, prints `Hello Ada`.
- P2 works, prints `true`.
- P3 works.

Costs. The reader must know the two categories: `fn greet` is reachable from line 1, but `limit :: 10` is not (E0622 explains when it bites). Two visibility descriptions live in one file — though each is the one Jet already has for that kind of code.

### Option C — statements only

A script admits no declarations at all; the first `fn` or `struct` forces full promotion. One category, no visibility question.

- P1, P2 are errors: `a script holds only statements — move to fn run to define functions`.
- P3 works.

Costs. The script rung can never hold a helper, so it dies at about five lines; the nudge to promote becomes a wall. `#Test` blocks beside a script are also gone.

### Recommendation: B

B is the only option that adds no new law — it is exactly the desugaring, and the promotion step never changes what a name can see. A punishes reordering and forbids mutual recursion, breaking real beginner programs (P1, P2). C kills the script rung's usefulness. For notebooks and the REPL, B makes function cells order-free and only state cells ordered, which matches how people actually re-run cells; A reproduces the out-of-order fragility notebooks are hated for. Expert cost of B is one teaching sentence: "loose bindings are body locals; declarations are file-wide."

## Decisions

| ID | Question | Rec |
|---|---|---|
| D-ENTRY-SCRIPT1 | Adopt script mode as sugar for `fn run` — for `jet run` only, for the whole verb table (run/dev/test), or keep explicit `fn run` mandatory? | B — whole verb table |
| D-ENTRY-ORDER1 | Script visibility: strict top-to-bottom, declarations file-wide with statements ordered, or statements only? | B — declarations file-wide |

Amended if ratified: S12 (gains the implicit spelling; explicit `fn run` stays canonical past a toy), E0003 copy (statement position becomes legal at the top of an entry file). Interacts, unamended: D-VERDICT-678-1 (`run.jet` default target may be a script), U7/D-ECO14 (reaffirmed), D-BUILDSCOPE1 (`fn build` stays a declaration beside code), D-ECO-OUTPUT-DEFAULT1 (an implicit `fn run` counts as the zero-config entry in its five-step rule), D-JPK-TASKRUN1 (wrapper precedent), open D-NAME-FILES1 slate on card #1625 (owns the import spelling; not touched here).

On ratification: record outcomes in `docs/spec/syntax-decisions.md`; mint implementation cards covering parser (statement position at file top), sema (implicit entry synthesis; E0620/E0621/E0622 and, under A, E0623 — final codes at registration), all-tier parity per I9, examples with golden output, formatter round-trip, diagnostic UI snapshots, and the `jet fix` wrap rewrite.
