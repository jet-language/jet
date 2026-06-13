# Language server (LSP) — unified vision & decision file

> **STATUS (2026-06-12): DRAFT for owner ratification.** This file is
> the full editor-experience vision for Jet. It expands
> docs/plans/m13-lsp.md (the milestone plan) the same way
> package-manager-decisions.md expanded m12: survey every tool worth
> stealing from, lay out each open choice with worked examples, and
> recommend. Once the D-LSP decisions below are ratified, this file is
> the single source of truth for editor behavior and
> docs/plans/m13-lsp.md is rewritten to match its phasing.

**How to read this file:** plain language first, spec second. §1
defines every term once. §3 is the survey — the best ideas from every
language whose editor experience people rave about, mainstream or not,
plus the failures we refuse to repeat. Each decision in §5 shows what
you would actually *see in the editor* under each option, then
strengths and weaknesses, then a recommendation. You don't need to know
how a language server works internally to decide — that's what the
examples are for.

---

## 1. The words, in plain language (read once)

- **Language server** — a long-running background program the editor
  starts and talks to. The editor sends "the user typed a character" or
  "what's under the cursor at line 12?"; the server answers. All the
  smartness lives in the server; the editor just draws.
- **LSP (Language Server Protocol)** — the standard wire format for
  that conversation, invented by Microsoft for VS Code and now spoken
  by essentially every editor (VS Code, Neovim, Helix, Zed, Emacs,
  Sublime, JetBrains). Speak it well and you support *all* editors at
  once.
- **Client** — the editor's side of the conversation.
- **Diagnostic / squiggle** — an error or warning shown inline as a red
  or yellow underline, with the message on hover. The editor face of
  `Error [E0307]: …`.
- **Completion** — the popup list of suggestions as you type.
- **Hover** — the floating card shown when the mouse (or a keypress)
  rests on a name: its type, its documentation.
- **Go to definition / find references** — jump to where a name was
  created; list every place it's used.
- **Rename** — change a name once and have every use in the project
  updated, safely (not text search-and-replace — it knows *which*
  `count` you mean).
- **Code action / quick fix** — the 💡 lightbulb: a one-click,
  machine-applied edit attached to a diagnostic ("Add missing arm for
  `Green`").
- **Code lens** — small clickable text the server injects *above* a
  line, e.g. `▶ run test` above a test function.
- **Inlay hint** — faint ghost text the server adds inline that isn't
  in the file, e.g. showing the inferred type: `val x⟨: Int⟩ = …`.
- **Semantic tokens** — the server telling the editor what each word
  *is* (type, function, mutable parameter…) so coloring reflects
  meaning, not just regex guesses.
- **Snippet** — a completion that inserts a fill-in-the-blanks template
  and tabs the cursor through the blanks.
- **Error recovery** — a parser's ability to take half-typed, broken
  code and still produce its best-guess structure for the rest of the
  file, instead of giving up at the first problem. Inside an editor,
  code is broken ~90% of the time (you're mid-keystroke); this is the
  difference between a server that works while you type and one that
  only works between edits.
- **Overlay** — the server's view of a file you've edited but not
  saved. The editor streams you the live buffer; you must analyze
  *that*, never the stale copy on disk.
- **Debounce** — waiting a beat (say 200ms) after the last keystroke
  before doing expensive work, so typing "hello" triggers one check,
  not five.
- **Cancellation** — abandoning an in-flight answer because the user
  typed again and the question is now stale.
- **Index** — a prebuilt lookup table (every definition in the project
  and its location) so "find references" is a table lookup, not a
  fresh search of every file.
- **Latency budget** — a hard number ("diagnostics in <100ms") that a
  test enforces, so speed is a feature that can't silently rot.

## 2. Vision (one paragraph)

**The compiler is the server.** Jet's front end already owns every
fact an editor could ask for — every type, every ownership rule, every
error message with its what/why/fix. The language server is not a
second program that re-learns Jet; it is the same lexer/parser/sema,
running long-lived, answering questions instead of printing and
exiting. That one architectural commitment — the thing Roslyn needed a
ground-up rewrite to reach, rust-analyzer spent years converging on,
and Zig's zls still doesn't have — is free for us because we're early.
On top of it we take the best habit from each tool people rave about:
Gleam/Dart's "it ships inside the compiler binary, zero install,"
rust-analyzer's "broken code is the normal case," Merlin's twenty-year
head start on editor ergonomics, gopls's "zero configuration," Metals'
"the server can diagnose its own setup," and Dart's "the editor's
fixes and the CLI's fixes are the same fixes." And Jet's superpower
carries straight over: the squiggle's message is *byte-identical* to
the terminal's — same renderer, same codes, same snapshot tests — so
the editor experience inherits every hour invested in docs/04.

## 3. The survey — what we take from each tool, what we refuse

The famous ones and the cult favorites. "Take" items become decisions
or requirements below; "refuse" items are the anti-lessons.

| Tool (language) | What it does best (we take it) | What we refuse |
|---|---|---|
| **rust-analyzer** (Rust) | The gold standard. Syntax trees that survive arbitrarily broken code, so every feature works mid-keystroke. Quick fixes as structured edits ("assists"). Every request cancellable — typing never waits. Tiny fixture tests with a `$0` cursor marker pinning each feature's behavior | Born *outside* the compiler — years of duplicated language knowledge before convergence (we start unified). Its 100+ configuration settings |
| **TypeScript** (tsserver) | Invented the category — LSP itself grew out of tsserver + OmniSharp. Auto-import: completing a name from another module inserts the `import` line for you. The "instant feedback or it's broken" culture | Its own non-LSP protocol, needing a translation shim in every non-VS-Code editor |
| **Roslyn** (C#) | "Compiler as a service": the compiler rebuilt as a library that editors query — our architecture rule, proven at scale. Lossless trees that keep every space and comment (what makes safe rewrites possible) | Needing a multi-year ground-up rewrite to get there (we start there) |
| **gopls** (Go) | One official server, zero configuration: open a file, it works. Format-on-save with zero options as *culture*, ending all style debates | The five competing half-tools that preceded it (godef, gocode, guru…) — fragmentation wasted years. Telemetry-by-default debates |
| **Dart analysis server** | Co-designed with the language from day one — tooling is a language feature, not an afterthought. `dart fix` applies the *same* fixes the editor lightbulb offers, in bulk, from the CLI | — |
| **clangd** (C++) | Latency as an obsession: answers from a cached "preamble" while the real check runs behind; an on-disk index so go-to-definition works seconds after a cold start, even on huge projects | Won't work at all until you produce a `compile_commands.json` — setup-before-value |
| **Merlin** (OCaml) | The pre-LSP pioneer, beloved for two decades. Error-recovering parsing ten years before it was standard. Type *widening*: tap hover again to see the type of the enclosing expression, then its parent. `destruct`: one keystroke turns a value into a match with every case stubbed | — |
| **Haskell** (HLS) | Eval-in-comments: a code lens runs the example in a doc comment and writes the answer into the file. Case-split code actions | The plugin architecture's fragility; "works only if compiler+server+project versions align" setup pain |
| **Lean 4** | The editor *is* the language's main UI — a live infoview updates the proof state at every keystroke; the server ships inside the compiler. Proof that "live feedback" can be the product | — |
| **Idris / Agda** | Hole-driven development, adored by its users: write `?todo`, the editor tells you what type belongs there, splits cases, even *searches* for an expression that fits | Holes are a language-design question for Jet, not an LSP one — spirit taken (case-split), feature deferred |
| **Metals** (Scala) | `metals doctor`: a command that inspects the setup and says exactly what's wrong and how to fix it, instead of failing silently | A separate out-of-process build server (BSP) — complexity our single-binary world doesn't need |
| **Gleam** | The whole experience ships in the one compiler binary — `gleam lsp` exists the moment the language is installed. Friendliest setup story in the industry | — |
| **Elm** | Error copy so good people screenshot it — carried into the editor verbatim. Our docs/04 is the same bet | The core team shipped no server; the community wrapper can only parse the compiler's *output* — expose libraries, not stdout |
| **Pyright** (Python) | Fast by doing less: pure analysis, never executes code, checks as you type | Python's fragmentation: five competing servers, none complete, users must choose |
| **zls** (Zig) — anti-lesson | — | A community server forced to *reimplement* the language by hand: it can't evaluate comptime, so it shows wrong types and misses errors. The single strongest argument for LSP-I1 below |
| **Kotlin** — anti-lesson | — | First-class only inside IntelliJ for a decade; every other editor second-class; an official LSP only began in 2024. Editor-neutral from day one or pay later |
| **SourceKit-LSP** (Swift) — anti-lesson | — | Cross-module answers come from an index produced *by building* — information is stale until your next successful compile. Live analysis or nothing |
| **Elixir** — lesson | — | Years of ElixirLS vs Lexical vs Next LS, until the core team finally built an official server on compiler internals. Ship official early so community effort pools instead of splitting |

The pattern across every success story: **the language team ships it,
inside or beside the compiler, sharing the compiler's brain, working
on broken code, fast enough to run between keystrokes.** Every failure
is missing one of those.

## 4. The shape: one front end, three customers

```
        ┌──────────────────────────────────────────────────┐
        │   the front end, as a library  (the one truth)   │
        │   lexer · parser · sema · fmt · diagnostics      │
        │   SourceProvider: reads disk OR open-buffer text │
        └──────────────────────────────────────────────────┘
            │                  │                   │
   jet build/run/test       jet lsp            jet dev (future)
   batch: check once,    long-running:      watch & re-run; same
   print, exit           answer editor      foundation (owner
                         questions          direction 2026-06-12)
```

Five rules sit above every decision below (proposed invariants, §7):
the server **never reimplements** language knowledge, **never
crashes** the session, **never blocks** typing, **never lies** (no
stale answers, and diagnostic text is byte-identical to the CLI), and
treats **broken code as the normal case**.

## 5. The decisions (D-LSP1…D-LSP13), each with worked examples

At-a-glance:

| ID | Question | Recommendation |
|----|----------|----------------|
| D-LSP1 | Where does the server live? | A — `jet lsp`, a subcommand of the one binary |
| D-LSP2 | What happens on half-typed code? | A — full error recovery; every feature works mid-keystroke |
| D-LSP3 | When do squiggles update? | A — live, debounced ~200ms, stale work cancelled |
| D-LSP4 | How does it stay fast as projects grow? | A — re-parse changed files only; measure before getting clever |
| D-LSP5 | What shows up when you type? | A — type-aware ranking + switch-arm snippet + auto-import |
| D-LSP6 | What does hover say? | A — full card: type + ownership in Jet words + doc comment |
| D-LSP7 | How do quick fixes work? | A — structured edits from sema, shared with a CLI `jet fix` |
| D-LSP8 | How much ghost text (inlay hints)? | A — off by default, except the hidden-clone hint |
| D-LSP9 | How many settings? | A — near-zero configuration |
| D-LSP10 | Standard protocol or custom extras? | A — strict standard LSP for v1 |
| D-LSP11 | What happens when *it* breaks? | A — crash-proof handlers + `jet lsp doctor` |
| D-LSP12 | How is it tested? | A — fixture tests + transcript tests + latency bench in CI |
| D-LSP13 | Live feedback (code lens / eval)? | A — defer to `jet dev` (post-v1); design the foundation now |

---

### D-LSP1 — Where does the server live?

*The question in plain words: when an editor needs a Jet language
server, what program does it run — and who is responsible for it never
disagreeing with the compiler?*

**Option A — a subcommand of the one `jet` binary (Rec).** The Gleam /
Dart model. The editor runs `jet lsp`; the server is the compiler,
in a different mode.

What you'd see — setup is "install Jet," full stop:

```
$ jet lsp --version
jet 0.9.2 (language server mode)

# VS Code: install the Jet extension; it finds `jet` on PATH. Done.
# Neovim:  vim.lsp.start({ cmd = {"jet", "lsp"} })       — one line
# Helix:   language-server jet = { command = "jet", args = ["lsp"] }
```

And version skew — the bug class where editor and compiler disagree —
cannot exist:

```
$ jet build           # says your code is fine
# …then the editor shows a red squiggle on the same line?
# Impossible: the squiggle came from the same binary, same sema,
# same diagnostic renderer.
```

- **Strengths:** zero install beyond the language itself; the server
  literally cannot drift from the compiler (zls's fatal flaw, solved
  structurally); one artifact to build, version, and release; the
  shared-foundation direction for `jet dev` falls out naturally.
- **Weaknesses:** the binary carries the server code even for users who
  never open an editor (negligible — it's the same front end plus a
  thin protocol loop, no new dependencies under I6); a server crash
  bug is a compiler-repo bug (we consider that a strength: one
  bug tracker, one owner).

**Option B — a separate `jet-ls` binary, same repo, shared libraries.**
The rust-analyzer end-state (after it moved in-tree). Two binaries
ship side by side.

```
$ which jet jet-ls
/usr/local/bin/jet
/usr/local/bin/jet-ls        ← must be present, found, and the SAME
                               version — three new ways to fail
$ jet --version && jet-ls --version
jet 0.9.2
jet-ls 0.9.1                 ← upgraded one, not the other: now the
                               editor and compiler disagree about
                               what compiles
```

- **Strengths:** the compiler binary stays a few hundred KB smaller; a
  catastrophic server bug can be patched and shipped alone.
- **Weaknesses:** every editor's config needs the second binary's
  name; installs can half-succeed; version-skew support tickets are a
  permanent tax. All cost, no user-visible benefit at our scale.

**Option C — a separate community project (the zls / early
rust-analyzer model).** The compiler repo ships nothing; someone else
builds a server by re-implementing Jet's rules.

What you'd see — the inevitable drift:

```
# zls today, transplanted: the editor's idea of Jet lags the real one
val x = parse(input);      ← editor: "type unknown" (server can't run
                              sema it didn't reimplement)
$ jet build                ← compiler: fine, x is Int
```

- **Strengths:** zero effort from us.
- **Weaknesses:** violates LSP-I1 by definition; the survey's clearest
  failure mode (zls, Elm, pre-2024 Kotlin, Elixir's fragmentation).
  Not actually an option — listed because it's the default outcome of
  *deciding nothing*.

---

### D-LSP2 — What happens on half-typed code?

*The question in plain words: you're mid-edit — a paren isn't closed,
a name is half-typed. Do completions, hover, and navigation still
work? This is the single most load-bearing choice in the file: inside
an editor, code is broken most of the time.*

The running example — you're partway through writing a function, and
ask for completion below it:

```jet
fn greet(name: String          ← no `)` yet; file doesn't parse
fn main() {
    val msg = gre▌             ← cursor here; you hit ctrl-space
}
```

**Option A — full error recovery: every feature works on broken code
(Rec).** The parser never gives up; it builds its best-guess structure
for the whole file, marks the gap, and moves on. Sema runs on the
recovered tree.

What you'd see:

```
    val msg = gre▌
              ┌──────────────────────────────────┐
              │ greet(name: String)   fn         │  ← it knows greet
              └──────────────────────────────────┘     exists, knows
                                                       its parameter,
                                                       even though the
                                                       `)` isn't typed
```

And exactly **one** squiggle, at the actual problem:

```
fn greet(name: String
                     ~ E0xxx: this parameter list isn't closed — add `)`
fn main() {              ← NO cascade of nonsense errors down here
```

- **Strengths:** the editor feels alive *while you think*, not just
  between edits — this is most of what people mean when they call
  rust-analyzer or Merlin magic. Bonus: the same recovery makes
  *terminal* errors better (one real error instead of a cascade), so
  the work pays docs/04 dividends too.
- **Weaknesses:** recovering parsers are genuinely harder to write
  than bail-on-first-error ones; each statement/expression form needs
  a "what if it's cut off here?" answer, and bad recovery can invent
  confusing phantom errors (mitigated by snapshot tests on broken-code
  fixtures — same I4 discipline, applied to incomplete programs).

**Option B — last-good-tree fallback.** When the file doesn't parse,
answer questions from the most recent version that *did*.

What you'd see — answers from the past:

```
    val msg = gre▌
              ┌──────────────────────────────────┐
              │ (no suggestions)                 │  ← greet didn't exist
              └──────────────────────────────────┘     last time the file
                                                       parsed, so the
                                                       editor's never
                                                       heard of it
```

- **Strengths:** much easier to build; features never see a broken
  tree.
- **Weaknesses:** the staleness is worst *exactly when you need help
  most* — writing new code. Hover and go-to-definition silently point
  at old line numbers after edits above. This is the "it works in
  demos, annoys daily" trap.

**Option C — bail: features off while the file has errors.** What LSP
v0 effectively does today.

```
    val msg = gre▌
              (no completions — file has syntax errors)
fn greet(name: String
~~~~~~~~~~~~~~~~~~~~~ parse error
fn main() {
~~~~~~~~~~~ parse error          ← cascade: one typo, three squiggles
```

- **Strengths:** zero extra work.
- **Weaknesses:** the editor goes dumb the moment you start typing —
  the opposite of the assignment. Fine for v0, disqualifying for v1.

---

### D-LSP3 — When do red squiggles update?

*The question in plain words: you fix a typo. How long until the
squiggle disappears — and does checking ever make typing feel sticky?*

The running example — `examples/11_enums.jet` is open; you delete the
`Green` arm from `next()`, watch, then undo.

**Option A — live, debounced ~200ms, stale work cancelled (Rec).**
Every keystroke schedules a re-check; the check starts after a ~200ms
quiet gap; a new keystroke abandons the in-flight check.

What you'd see (timeline):

```
0ms     you delete the Green arm
0–200ms typing more? nothing runs — keys are never waited on
200ms   quiet — check starts (changed file re-parsed, sema re-run)
~250ms  squiggle appears on `switch light {`:
        E0307: `switch` doesn't cover every case — missing: Green
        💡 1 fix available
you hit undo
~250ms later: squiggle gone
```

The message is the terminal's, verbatim — same renderer, same code,
same what/why/fix — plus the lightbulb (D-LSP7).

- **Strengths:** errors surface while the mistake is still in your
  head, which is the entire point of an editor integration; debounce +
  cancellation means typing latency stays at zero no matter how slow
  checking gets; this is what every server in the survey's "take"
  column converged on.
- **Weaknesses:** needs the D-LSP4 machinery to stay under budget on
  big projects; transient "you're mid-thought" errors flash briefly
  (the debounce gap absorbs most; we never interrupt typing to show
  them).

**Option B — on save only.** The server re-checks when you hit save.

```
you delete the Green arm … nothing
you keep working on a wrong assumption for four minutes
you hit ⌘S → squiggle appears, four minutes late
```

- **Strengths:** trivially cheap; no incrementality needed at all.
- **Weaknesses:** "stale until saved" is the SourceKit-LSP complaint
  in miniature; modern editors auto-save erratically so behavior feels
  random; beginners — our audience — are precisely the people who
  won't know to save to refresh.

**Option C — every keystroke, no debounce.** Maximum freshness.

```
typing "Yellow" = 6 keystrokes = 6 full checks racing each other;
on a large project the fan spins and the squiggles strobe
through half-typed states: missing: Y… missing: Ye… missing: Yel…
```

- **Strengths:** ~150ms fresher than A in the best case.
- **Weaknesses:** all cost, imperceptible benefit; the strobing
  half-word errors are actively worse than a beat of patience.

---

### D-LSP4 — How does the server stay fast as projects grow?

*The question in plain words: at 50 files, does every keystroke
re-check all 50? There are three escalation levels of cleverness; each
buys speed and costs complexity and bug surface. Which do we build
now?*

The shared yardstick — a hard latency budget, enforced in CI:

```
$ jet lsp --bench tests/lsp/bench/5k-line-project.session
  replaying 312 recorded edits…
  diagnostics  p95  41ms   (budget 100ms)  ✓
  completion   p95  18ms   (budget  50ms)  ✓
  hover        p95   6ms   (budget  50ms)  ✓
```

**Option A — file-granular: re-parse only changed files; re-run sema
whole-program; measure before getting clever (Rec — matches m13).**
Parsing dominates and is perfectly cacheable per file; Jet's sema is
fast and whole-program keeps it simple and *correct by construction*.

What you'd see — editing one file of fifty:

```
keystroke in src/scoring.jet
  re-parse: scoring.jet only          (49 cached trees reused)
  sema:     whole program             (fast: no I/O, trees in memory)
  total:    ~40ms on the 5k-line bench — well inside budget
```

- **Strengths:** small, boring, hard to get wrong — no cache
  invalidation bugs, which are the #1 source of "restart your language
  server" folklore. The bench harness tells us *if and when* this
  stops being enough, with numbers instead of vibes.
- **Weaknesses:** sema cost grows with project size; at some size
  (likely far beyond v1 projects) the budget fails and we escalate to
  B. The architecture must not paint over that door (sema behind
  clean entry points, no global mutable state).

**Option B — query memoization now (the rust-analyzer/salsa model).**
Every computation ("type of function X") is a cached query that
remembers its inputs; edits invalidate only the queries they touch.

What you'd see — same editor behavior, different ceiling:

```
keystroke in src/scoring.jet
  invalidated: 3 queries (letter()'s body, its signature uses, …)
  recomputed:  3 queries; 4,910 untouched
  total: ~5ms — and still ~5ms at 500k lines
```

- **Strengths:** the only known design that scales to enormous
  projects; per-keystroke cost proportional to the edit, not the
  project.
- **Weaknesses:** the single largest engineering investment in this
  file — rust-analyzer's took years, and salsa-the-crate is barred by
  I6 so it's all hand-built; cache-invalidation bugs masquerade as
  "phantom errors, restart fixes it," the exact folklore we refuse to
  ship. Building it before the bench proves the need is gold-plating.

**Option C — recompute everything every time.** What v0 does.

```
keystroke → re-read, re-parse, re-check all 50 files: ~300ms and
growing linearly; the 100ms budget fails at medium size
```

- **Strengths:** zero machinery; provably never stale.
- **Weaknesses:** fine for v0's single files; fails the budget the
  moment projects are real. Listed as the floor, not a contender.

---

### D-LSP5 — Completion: what shows up when you type?

*The question in plain words: ctrl-space is the most-used feature of
any server, hundreds of times an hour. Is the list a dumb alphabet, or
does it know what you're trying to do?*

The running example — `Light` from examples/11_enums.jet is in scope:

```jet
enum Light { Red; Yellow; Green; }
fn label(light: Light) -> String { … }
fn next(light: Light) -> Light { … }
```

**Option A — type-aware ranking + member completion + switch-arm
snippet + auto-import (Rec — matches m13, sharpened).**

What you'd see — three moments. First, ranking by what *fits*: the
context wants a `String`, so producers of `String` float up:

```jet
fn main() {
    val text: String = la▌
                       ┌────────────────────────────────────────┐
                       │ label(light: Light) → String        fn │ ← fits; first
                       │ last_index(…) → Int                 fn │ ← matches "la",
                       └────────────────────────────────────────┘   doesn't fit; lower
```

Second, the killer demo — complete a `switch` on an enum and every arm
is pre-filled (Merlin's `destruct`, Jet-shaped); tab moves through the
blanks:

```jet
    switch light ▌
    ── accept "switch — fill in all arms" ──▶
    switch light {
        (light == Red) -> { ▌ };
        (light == Yellow) -> {  };
        (light == Green) -> {  };
    }
```

Exhaustiveness (E0307) becomes something you *never even hit*, because
the editor writes the exhaustive skeleton for you.

Third, auto-import (tsserver's gift): completing a `pub fn` from
another module inserts the `import` too:

```jet
    val grade = let▌
                ┌──────────────────────────────────────────┐
                │ letter(score: Int) → String   fn  scoring │
                └──────────────────────────────────────────┘
    ── accept ──▶
import "scoring";                       ← added at the top for you
…
    val grade = scoring.letter(▌);
```

- **Strengths:** every piece answers a beginner's actual question
  ("what can I put here?", "what are the cases?", "where does that
  function live?") — docs/00's audience, served at the keystroke
  level; the switch snippet is the 10-second demo that sells the
  language.
- **Weaknesses:** ranking needs the expected type at the cursor *in
  broken code* — D-LSP2's recovery is a hard prerequisite; ranking
  quality needs fixture tests per context (D-LSP12) or it regresses
  silently.

**Option B — names in scope, alphabetical.** The floor; what most
young languages ship.

```jet
    val text: String = la▌
                       ┌──────────────────────────────┐
                       │ label        fn              │
                       │ last_index   fn              │   ← same prefix,
                       │ Light        enum            │     no idea what
                       └──────────────────────────────┘     you need
```

- **Strengths:** a fraction of the work; no type machinery at the
  cursor.
- **Weaknesses:** on a real project the list is hundreds long and the
  answer is item 40; beginners read it top-to-bottom and learn to
  ignore it. "Technically has completion" is how languages get called
  half-finished.

**Option C — everything in A, plus postfix completion.** The
rust-analyzer/ReSharper power-user trick: type a value, then a dot,
then a *transformation*:

```jet
    light.switch▌
    ── accept ──▶            ← the dot-word rewrites the line
    switch light {
        (light == Red) -> { ▌ };
        …
    }

    ready.if▌   ──▶   if ready { ▌ }
```

- **Strengths:** beloved by experts; you type in the order you think
  ("this value… now branch on it").
- **Weaknesses:** dot-then-keyword is *almost syntax* — beginners will
  absolutely type `light.switch` in a file and be confused it doesn't
  compile. In a beginner-first language that ambiguity costs more than
  it pays. Defer; revisit post-v1 with owner sign-off (it's also an
  I7-adjacent surface question).

---

### D-LSP6 — What does hover say?

*The question in plain words: resting on a name is how people ask
"what is this thing?" — hover is the documentation system most users
will ever see. How much does the card carry?*

The running example, from examples/08_ownership.jet:

```jet
fn archive(take name: String) -> String { … }
fn main() {
    val saved: String = archive(take "vault");
    print(sa▌ved);                ← hover here
}
```

**Option A — the full card: type + ownership in Jet words + doc
comment (Rec — matches m13).**

What you'd see:

```
┌──────────────────────────────────────────────────────┐
│ saved : String                                       │
│ val — set once, never changes                        │
│ ──────────────────────────────────────────────────── │
│ declared at line 23                                  │
└──────────────────────────────────────────────────────┘
```

And on a documented function (S49 `///` comments), hovering
`archive`:

```
┌──────────────────────────────────────────────────────┐
│ fn archive(take name: String) -> String              │
│ takes ownership of `name` — the caller can't use it  │
│ after this call                                      │
│ ──────────────────────────────────────────────────── │
│ Stores the name in the archive and returns the       │
│ stored copy.                                         │
└──────────────────────────────────────────────────────┘
```

The ownership line is the differentiator: no other language explains
*its hardest concept* in the hover, in product-copy English. This is
docs/04's voice, in the editor, before any error happens.

- **Strengths:** hover becomes where Jet's ownership model gets
  *taught*, one rest-of-the-mouse at a time; doc comments become
  worth writing because they surface everywhere.
- **Weaknesses:** the copy for each ownership mode (`val`, `var`,
  `mut`, `take`, `view`) is product text — needs the same snapshot
  discipline as diagnostics or it drifts (pin with fixture tests,
  D-LSP12).

**Option B — bare type.**

```
┌──────────────────┐
│ saved : String   │
└──────────────────┘
```

- **Strengths:** least work; never wrong.
- **Weaknesses:** answers the question a Rust expert has, not the one
  a Jet beginner has ("why can't I change it?" / "what does `take` do
  here?"); wastes the front end's hardest-won knowledge.

**Option C — A, plus Merlin's widening.** Repeat the hover keystroke
to expand the inspected expression outward:

```
print(saved.first_word());
        ▌
press 1: saved                : String
press 2: saved.first_word()   : String
press 3: print(…)             : (nothing)
```

- **Strengths:** answers "what type is *this whole expression*?"
  without selecting anything — OCaml folks who try it never go back;
  cheap once hover exists (it's hover on enclosing spans).
- **Weaknesses:** standard LSP has no "press again" notion — needs
  either the selection-range trick or a custom request, which collides
  with D-LSP10's strict-standard rule. Verdict: take A now; add
  widening when it fits *within* the standard protocol (selection
  range + hover compose today in VS Code and Helix).

---

### D-LSP7 — How do quick fixes work, and does the terminal get them too?

*The question in plain words: docs/04 mandates a Fix line for every
error. Today that line is prose a human retypes. Should fixes become
machine-applicable edits — and if so, are the editor's lightbulb and a
CLI command the same fixes or two implementations?*

The running example — you added `Blue` to `Light` but `next()` doesn't
handle it. The terminal already says:

```
Error [E0307]: `switch` doesn't cover every case — missing: Blue
 Fix: add an arm for: Blue
```

**Option A — sema attaches structured edits to diagnostics; the
lightbulb and a new `jet fix` are the same data (Rec — matches m13,
plus Dart's bulk-apply).** `Diagnostic` grows an optional machine
edit: *replace this span with this text*. One source; two faces.

What you'd see in the editor:

```jet
    switch light {
    ~~~~~~ E0307: `switch` doesn't cover every case — missing: Blue
    💡 Add arm for `Blue`
    ── click ──▶
        (light == Blue) -> {
            return ▌;          ← cursor placed in the one blank
        };
```

And the same fix from the terminal, Dart-style, across a whole
project:

```
$ jet fix
  src/lights.jet:8   E0307  added switch arm for `Blue`
  src/lights.jet:21  E0307  added switch arm for `Blue`
  2 fixes applied · 1 not auto-fixable (E0304: unknown variant — needs a human)
$ jet build
  error: 1 remaining → src/lights.jet:30
```

Only *safe* fixes auto-apply; anything judgment-shaped stays a
suggestion.

- **Strengths:** one implementation, snapshot-tested once, surfacing
  everywhere (editor, CLI, future CI bots); refactor-scale chores
  ("I renamed a variant, fix all 40 switches") become one command;
  this is the m13 foundation refactor — it lands first anyway.
- **Weaknesses:** every fix's inserted text must *compile* — that's a
  new test obligation per diagnostic (each fix needs a
  fixture-applies-cleanly test, extending I4); span-edit plumbing
  touches `Diagnostic`, the renderer, and the LSP at once.

**Option B — fixes implemented in the LSP layer only.** The lightbulb
edits are built from the diagnostic's text, in lsp.rs; the CLI keeps
prose.

- **Strengths:** no changes to sema or `Diagnostic`; ships faster.
- **Weaknesses:** parses our own error strings to reconstruct
  intent — fragile in exactly the way I3 ("checking lives in sema")
  warns about; CLI users get nothing; the moment a fix needs type
  information (most do), lsp.rs starts reimplementing sema — LSP-I1
  violated from the inside.

**Option C — prose only (status quo).** The Fix line stays human-only.

```
 Fix: add an arm for: Blue        ← you read it, you type it
```

- **Strengths:** zero work; already shipped.
- **Weaknesses:** we *computed* the exact missing arms and then made
  the user transcribe them. The gap between "great error" and "great
  error you can accept with one keypress" is the gap users actually
  rave about.

---

### D-LSP8 — Inlay hints: how much ghost text?

*The question in plain words: the editor can draw faint text that
isn't in the file — inferred types, ownership events. Helpful overlay
or visual noise? (⟨angle text⟩ below is ghost: visible, not in the
file, not saved.)*

**Option A — off by default, except the hidden-clone hint (Rec —
matches m13).** The one thing Jet does *implicitly* (auto-clone,
L0201) is the one thing always made visible:

```jet
fn main() {
    val greeting: String = "hello";
    show(⟨clone⟩ greeting);     ← ghost: a copy happens here silently
    print(greeting);
    val n = count_words(text);  ← no ⟨: Int⟩ clutter — types stay off
}                                  unless you turn them on
```

One setting (`jet.hints.types: on`) adds the type hints for those who
want them:

```jet
    val n⟨: Int⟩ = count_words(text);
```

- **Strengths:** ghost text is reserved for the single place Jet's
  surface hides a cost — the hint *is* ownership teaching, not
  decoration; default screenshots of Jet code stay clean (beginners
  can't yet tell ghost from real, and confusion there is expensive).
- **Weaknesses:** users coming from rust-analyzer's everything-on
  default may miss type hints until they find the setting (the doctor
  card and README mention it).

**Option B — everything on by default** (rust-analyzer's posture):

```jet
fn main() {
    val greeting⟨: String⟩ = "hello";
    show(⟨name:⟩ ⟨clone⟩ greeting);
    val n⟨: Int⟩ = count_words(⟨text:⟩ text);
}
```

- **Strengths:** maximum information; experts who like it, love it.
- **Weaknesses:** a beginner's first Jet file is now 30% text that
  isn't real — "why doesn't yours have the `: String`?" is a support
  question we'd be choosing to create. Rust-analyzer's own most-cited
  config tweak is people turning these *off*.

**Option C — no inlay hints at all.**

- **Strengths:** nothing to build.
- **Weaknesses:** throws away the clone hint, the one ghost with a
  safety story to tell. L0201's lint text exists precisely because
  hidden copies matter.

---

### D-LSP9 — How many settings?

*The question in plain words: gopls works the moment it starts and has
a handful of options. rust-analyzer has well over a hundred. Which
philosophy?*

**Option A — near-zero configuration (Rec).** Everything has the right
default; the settings that exist fit on one screen.

What you'd see — the entire settings surface, v1:

```
jet.path          where the jet binary is        (default: PATH)
jet.hints.types   show inferred-type ghosts      (default: off)
jet.trace         log protocol traffic for bugs  (default: off)
```

Formatting has no options at all — `jet fmt` is the format (gofmt's
lesson: the absence of knobs is what ends the style wars).

- **Strengths:** "open file, it works" is the entire setup story —
  the Gleam/gopls experience our audience needs; every setting we
  don't add is a support matrix we don't carry and a docs page nobody
  writes.
- **Weaknesses:** power users will ask for toggles (completion
  styles, hint sets, lint severities) and the answer is usually "no,
  and here's why" — that takes spine; a genuinely-needed setting
  arrives a release later than it would under B.

**Option B — configure everything** (rust-analyzer's posture).

```
jet.completion.snippets.enable, jet.completion.autoimport.enable,
jet.completion.postfix.enable, jet.hover.ownership.show,
jet.hover.docs.show, jet.diagnostics.debounceMs,
jet.hints.types, jet.hints.clone, jet.hints.parameterNames,
jet.semanticTokens.enable, …                       (× 10 more screens)
```

- **Strengths:** nobody is ever blocked by a default; behavior
  disputes get settled by "it's a setting."
- **Weaknesses:** every option doubles the test matrix and ages into
  folklore ("paste these 15 settings from that blog post"); defaults
  stop being designed because escape hatches exist. For a
  beginner-first language this is the simplicity ratchet (I8) running
  in reverse.

---

### D-LSP10 — Standard protocol only, or custom extensions?

*The question in plain words: LSP covers ~95% of what we want.
rust-analyzer added custom requests beyond it (view syntax tree,
expand macro, run flycheck) that only its own VS Code extension
understands. Do we extend?*

**Option A — strict standard LSP for v1 (Rec).** Every feature in this
file expresses itself through standard requests.

What you'd see — a Helix user, no plugin, no extension, nothing:

```
$ hx src/lights.jet
  # diagnostics, completion (with the switch snippet), hover with
  # ownership, rename, code actions — 100% of Jet's editor features,
  # because Helix speaks standard LSP and that's all we use.
```

VS Code, Neovim, Zed, Emacs, Sublime: identical feature list. The VS
Code extension is *only* packaging — grammar files and "find the
binary," not features.

- **Strengths:** one implementation serves every editor *equally* —
  the anti-Kotlin guarantee, structural rather than promised; nothing
  to keep in sync per-editor; the protocol's limits double as a scope
  fence (I8 again).
- **Weaknesses:** some niceties don't fit and wait (Merlin-style
  widening as a dedicated gesture; a syntax-tree visualizer for
  compiler debugging); if a future flagship feature truly needs an
  extension, this decision gets reopened with a concrete case in
  hand.

**Option B — extend where useful (rust-analyzer's posture).**

```
jet/viewSyntaxTree, jet/expandSwitch, jet/ownershipFlow …
  → VS Code with our extension: extra commands appear
  → every other editor: those features simply don't exist
```

- **Strengths:** no ceiling on ambition; great internal debugging
  tools ride the same channel.
- **Weaknesses:** a two-tier editor world on day one, which the survey
  says communities resent for years; each extension is protocol
  surface we document, version, and test alone. Internal debug needs
  have a simpler home: `jet ast file.jet` in the terminal.

---

### D-LSP11 — What happens when the server itself breaks?

*The question in plain words: every language server eventually hits an
internal bug. The folklore fix everywhere is "restart your editor."
What's ours instead?*

**Option A — crash-proof request handlers + `jet lsp doctor` (Rec —
m13's crash policy, plus Metals' best idea).** A panic in any request
is caught, logged, answered with an error; the session lives on. The
user is told once, with a path forward. And a doctor command inspects
the whole setup.

What you'd see — a handler bug strikes mid-session:

```
┌─ VS Code notification (once, not per keystroke) ──────────────┐
│ Jet hit an internal bug answering a hover request. The server │
│ is still running. Details: ~/.jet/lsp/crash-2026-06-12.log —  │
│ please attach it to a GitHub issue.                           │
└───────────────────────────────────────────────────────────────┘
  # completion, diagnostics, everything else: still working
```

And when an editor setup misbehaves, instead of forum spelunking:

```
$ jet lsp doctor
  jet binary         0.9.2  (~/.local/bin/jet)
  editor handshake   ok — VS Code 1.99 connected, initialize 9ms
  project root       ~/code/weather  (jet.toml found)
  open buffers       3 overlays active
  last full check    41ms  (budget 100ms)  ✓
  crashes this session   0
  log                ~/.jet/lsp/session.log
  all clear — if the editor still misbehaves, run: jet lsp --trace
```

- **Strengths:** "never crashes, tells you what's wrong" is a
  reputation compounder — Metals' doctor is the most-praised feature
  of a server in a far more complicated ecosystem; crash logs with
  context turn user pain directly into fixable bug reports (the ICE
  banner philosophy, I2, extended to the server).
- **Weaknesses:** catch-and-continue can mask state corruption — so
  the rule is *answer with an error, then re-check the world from
  source text* (our D-LSP4 simplicity makes resetting cheap: there's
  little cached state to corrupt); doctor is one more surface to keep
  truthful.

**Option B — let it die; the editor auto-restarts it.** What many
servers do in practice.

```
hover → server process exits
VS Code: "The Jet language server crashed 5 times in 3 minutes;
          not restarting it."        ← squiggles gone, silence
```

- **Strengths:** restart-from-zero is the ultimate state reset; no
  catch logic.
- **Weaknesses:** loses all open-buffer state mid-flight; five
  crashes and editors give up entirely; the user learns nothing
  except that Jet feels broken. Acceptable as a *backstop* under A
  (if the process somehow does die, restart is clean) — not as the
  policy.

---

### D-LSP12 — How do we know it works (and stays fast)?

*The question in plain words: the compiler has golden examples and ui
snapshots (I4/I5). What's the equivalent contract for a long-running,
interactive server?*

**Option A — three layers: caret-marker fixture tests, JSON transcript
tests, latency bench in CI (Rec — m13's exit criteria, made
concrete).**

Layer 1 — fixture tests (rust-analyzer's house style): tiny inline
programs with `▌` marking the cursor, pinning one behavior each. These
are to the LSP what tests/ui is to diagnostics:

```rust
#[test]
fn hover_val_shows_ownership() {
    check_hover(
        r#"
fn main() {
    val greeting = "hi";
    print(gre▌eting);
}
"#,
        expect![[r#"
            greeting : String
            val — set once, never changes
        "#]],
    );
}

#[test]
fn completion_ranks_by_expected_type() { … }

#[test]
fn switch_fix_inserts_compilable_arm() { … }   // applies the edit,
                                               // then runs sema on the
                                               // result: zero errors
```

Layer 2 — transcript tests: a recorded editor session (JSON in/out)
replayed against the real server binary — initialize, didOpen, edits,
requests — asserting responses. Catches protocol bugs fixtures can't.

Layer 3 — the bench (from D-LSP4) in CI, failing the build when p95
exceeds budget:

```
$ cargo test && jet lsp --bench tests/lsp/bench/*.session
  lsp fixtures ......... 184 passed
  lsp transcripts ......  12 passed
  bench: diagnostics p95 41ms / 100ms  ✓   completion p95 18ms / 50ms  ✓
```

- **Strengths:** every feature in this file gets a pinned,
  reviewable example — hover copy and completion ranking become
  product copy under snapshot discipline, exactly like docs/04;
  speed regressions fail CI instead of arriving as user reports;
  fixtures double as living documentation of intended behavior.
- **Weaknesses:** the fixture harness (markers, expect-blocks,
  apply-fix-then-compile) is real infrastructure built before it pays
  off; transcripts are brittle to protocol-shape changes (kept few
  and high-value).

**Option B — transcript tests only.** End-to-end or nothing.

- **Strengths:** tests exactly what editors experience.
- **Weaknesses:** one behavior change breaks dozens of long JSON
  files; nobody can read a transcript diff and see *what* regressed;
  ranking/copy nuances are practically untestable at this layer.

**Option C — manual dogfooding.** Use it daily; fix what annoys.

- **Strengths:** honest signal; zero harness cost.
- **Weaknesses:** doesn't scale past one user, catches nothing in CI,
  and violates the spirit of I4/I5 — if it isn't pinned by a test, the
  behavior doesn't exist.

---

### D-LSP13 — Live feedback: code lenses and inline evaluation?

*The question in plain words: the most-loved features in Haskell and
Lean go beyond answering questions — the editor runs things. Do we, and
when? (Owner direction 2026-06-12 already says the server foundation
is shared with a future `jet dev` watch mode, and no dev-mode features
land in M13.)*

**Option A — defer to `jet dev`, post-v1; design the foundation now
(Rec).** v1 ships nothing here, but D-LSP1/4's long-running,
overlay-fed, file-granular server is *built as* the engine `jet dev`
will host. This decision exists so deferral is a recorded choice, with
the prize on the table:

What B and C would look like, so we know what we're deferring —

The run/test lens (B):

```jet
▶ run
fn main() {
    print(label(Light.Red));
}

▶ run test          ✓ passed 41ms ago
test "next cycles" {
    assert(next(Light.Green) == Light.Red);
}
```

HLS-style eval-in-comments (C) — a lens runs the example in a doc
comment and writes the answer *into the file*:

```jet
/// example: next(Light.Red)
/// gives:   ▶ eval
    ── click ──▶
/// example: next(Light.Red)
/// gives:   Light.Yellow          ← written by the editor; and now
                                     it's a testable claim docs can't
                                     let drift
```

- **Strengths (of deferring):** v1 scope stays the m13 list; running
  user code from the server raises real questions (sandboxing,
  long-running programs, output limits) that deserve their own
  decision file alongside `jet dev`; nothing is foreclosed — the
  foundation is explicitly designed for it.
- **Weaknesses:** the eval lens is a genuine wow-feature for a
  beginner language ("the docs answer back") and competitors are
  starting to copy HLS; post-v1 may feel far away.

**Option B / C — ship lenses (run/test) or eval in M13.**

- **Strengths:** demo material; the test lens in particular is cheap
  once `jet test` exists per-function.
- **Weaknesses:** contradicts the standing owner direction and m13's
  out-of-scope list; every lens is a process-spawning, output-capping,
  "what if it loops forever" problem the diagnostics path never has.
  If pulled forward, pull B (lenses) only — C (eval) belongs with
  `jet dev`.

---

## 6. Phasing (how this maps onto the roadmap)

- **Shipped (M6, LSP v0):** diagnostics on open/change, formatting,
  S14 autocorrect code actions, the VS Code extension skeleton.
- **M13 = LSP v1, in this order** (each step is independently
  shippable):
  1. `SourceProvider` overlay refactor (prerequisite; `jet run` stays
     byte-identical) and the structured-fix refactor of `Diagnostic`
     (D-LSP7's foundation — CLI `jet fix` falls out here).
  2. Error-recovering parser (D-LSP2) — the load-bearing investment;
     terminal cascades improve as a side effect.
  3. Debounced live diagnostics with cancellation (D-LSP3) +
     file-granular incrementality + `jet lsp --bench` (D-LSP4) + the
     fixture/transcript harness (D-LSP12).
  4. Completion (D-LSP5 A), hover (D-LSP6 A), go-to-definition /
     references / rename (span table in sema).
  5. Semantic tokens; inlay clone hint (D-LSP8); `jet lsp doctor`
     (D-LSP11); tree-sitter + TextMate grammars generated from
     src/syntax.rs.
- **Post-v1:** `jet dev` and lenses/eval (D-LSP13), postfix completion
  (D-LSP5 C, owner sign-off), Merlin widening within standard LSP
  (D-LSP6 C), query memoization *only if the bench fails* (D-LSP4 B).
- **Open engineering note (not a D-decision):** m13's JSON-RPC layer
  question stands — if the hand-rolled JSON becomes the bottleneck or
  bug source under load, request owner approval for serde_json in the
  tooling path (I6 protocol) rather than gold-plating.

## 7. Invariants (extend I1–I8, alongside PM-I1…PM-I8)

- **LSP-I1** Single source of truth: the server reuses
  lexer/parser/sema/fmt as libraries and never reimplements language
  knowledge. Any LSP feature that needs a fact sema doesn't expose
  gets it *added to sema*, not recomputed locally. (zls is the
  cautionary tale.)
- **LSP-I2** The server never crashes the session: panics in handlers
  are caught, logged with context, answered with an error response.
  Process death is a P0 bug, like an ICE (I2's sibling).
- **LSP-I3** The server never blocks typing: every request is
  cancellable, and no answer is computed for a question that's already
  stale.
- **LSP-I4** The server never lies: results reflect the current buffer
  (overlay, not disk), and a diagnostic's text in the editor is
  byte-identical to the terminal's — same renderer, same codes; the
  tests/ui snapshots bind both (compiler I4 extends to the LSP
  unchanged).
- **LSP-I5** Broken code is the normal case: every capability must
  have fixture tests on syntactically incomplete programs, not just
  valid ones.
- **LSP-I6** Speed is enforced, not hoped for: the latency budgets
  live in CI; a regression past budget fails the build.

## 8. Conflicts & gaps this file resolves (the reconciliation ledger)

| Topic | docs/plans/m13-lsp.md says | This file adds/decides |
|---|---|---|
| Server location | implied in-binary | explicit: `jet lsp` subcommand, with the version-skew rationale (D-LSP1) |
| Error recovery | unstated (capabilities assume it) | named as the load-bearing prerequisite, with its own step and broken-code test obligation (D-LSP2, LSP-I5) |
| Diagnostics cadence | "per-keystroke with debouncing" | concrete: ~200ms debounce + cancellation, with the timeline (D-LSP3) |
| Incrementality | file-granular, "measure before getting clever" | same, plus the named escalation path and CI-enforced budgets (D-LSP4, LSP-I6) |
| Completion | scope list + switch snippet | adds type-aware *ranking* and auto-import; explicitly defers postfix (D-LSP5) |
| Quick fixes | structured fixes shared with CLI `--fix` | CLI face named `jet fix`, Dart-style bulk output; fix-must-compile test rule (D-LSP7) |
| Configuration | unaddressed | near-zero settings philosophy (D-LSP9) |
| Protocol extensions | unaddressed | strict standard LSP for v1 (D-LSP10) |
| Doctor command | unaddressed (crash policy only) | `jet lsp doctor` (D-LSP11) |
| Testing | transcripts + bench | adds the fixture layer with caret markers (D-LSP12) |
| Code lens / eval | out of scope, no rationale | recorded as a decision with the deferred prize visible (D-LSP13) |

## 9. On ratification (agent checklist)

1. Owner answers D-LSP1…D-LSP13 → record each in §5 with date.
2. Rewrite docs/plans/m13-lsp.md to match §6's step order; fold the
   ratified decisions in by ID.
3. Add LSP-I1…LSP-I6 to the invariants list CLAUDE.md points at.
4. The hover ownership copy and completion labels in this file's
   examples are *drafts* — final text goes through the docs/04 voice
   review and gets pinned by D-LSP12 fixtures before shipping.
5. Postfix completion (D-LSP5 C) and any future protocol extension
   (D-LSP10 B) re-enter through the syntax/owner decision protocol —
   they are surface area, not internals.
