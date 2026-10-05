# Learnability, reasoning, reading, and writing: research and Jet audit

Date: 2026-09-28. Method: `research` (standalone), with a report-only audit of
current Jet. This report records dated evidence and proposals. It does not own
plans or status; nothing here creates a Tower card, ballot, or implementation.

## Question and scope

1. Why do people find learning to program hard, according to research,
   practitioner essays, talks, and forum discussion?
2. Which of those difficulties can a *language* (its semantics, syntax,
   diagnostics, defaults, and bundled tools) prevent or shrink, without
   literally teaching?
3. What does the research say about making code easy to **reason about**,
   **read**, and **write**, in that priority order (philosophy pillar 2)?
4. Where does current Jet stand against those findings, and what should change?

## Method and evidence limits

- External sources are cited inline with links. Peer-reviewed papers and the
  authors' own essays were preferred; forum threads are cited as testimony, not
  as measurement.
- Jet evidence comes from code, registries, examples, UI snapshots, the
  syntax-decision record, Tower cards (read-only), three read-only scout passes,
  and 26 probe programs plus scaffold and glyph checks run by this investigation.
- **Probe binary provenance.** Probes used `target/debug/jet` (built
  2026-09-28 13:07) and, for one check, `target/release/jet` (built 11:15).
  HEAD was `68e1f8fb`, and the working tree carried another stream's
  uncommitted edits in `crates/jet-sema`, `crates/jet-parser`, `crates/jet-jit`,
  and `crates/jet-codegen`. This investigation did not rebuild, to avoid
  compiling that stream's in-flight work. Probe results therefore describe the
  working-tree compiler, not a clean release. Findings marked **(recheck)** may
  be transient.
- Probes ran in `~/.cache/jet-dev/learn-probes/`, a directory with its own
  `package.jet` granting `[IO, Mem.Alloc]`. Probe sources are reproduced in
  Appendix A.
- This is not a user study. Any statement about how learners would react is a
  prediction from the cited research, not an observation of Jet users.

---

## Part 1. Why learning to program is hard

### 1.1 The machine is invisible: notional machines and hidden state

Du Boulay coined the *notional machine*: "an idealized, conceptual computer
whose properties are implied by the constructs in the programming language"
([du Boulay 1986](https://journals.sagepub.com/doi/abs/10.2190/3LFX-9RRF-67T8-UVK9);
definition as quoted in [ACM TOCE 2024](https://dl.acm.org/doi/10.1145/3688390)).
Sorva's review names it a major challenge of introductory programming and
notes that misconceptions from wrong mental models are "covert and resistant
to change," unlike syntax slips
([Sorva 2013](https://dl.acm.org/doi/10.1145/2483710.2483713);
[Qian & Lehman 2017](https://dl.acm.org/doi/10.1145/3077618)). Pea's
"superbug" is the belief that the computer understands intent
([cited in Qian & Lehman](https://dl.acm.org/doi/pdf/10.1145/3077618)).

Bret Victor puts the same problem bluntly: "We expect programmers to write code
that manipulates variables, without ever seeing the values of those
variables." His requirements are *read the vocabulary*, *follow the flow*, and
*see the state*, plus "eliminate hidden state"
([Learnable Programming, 2012](https://worrydream.com/LearnableProgramming/)).

**Language lever:** the fewer invisible rules and hidden state the semantics
has, the smaller the notional machine a learner must build.

### 1.2 Working memory is tiny

Hermans' *The Programmer's Brain* frames comprehension around long-term,
short-term, and working memory, with short-term capacity of a handful of
chunks, and separates intrinsic, extraneous, and germane load
([reading notes](https://masalmon.eu/2026/08/21/the-programmer-s-brain-reading-notes/);
[publisher](https://www.manning.com/books/the-programmers-brain)). Guzdial:
"CS1 has too large a cognitive load"
([2010](https://computinged.wordpress.com/2010/04/14/is-learning-to-program-inherently-hard/)).
An fMRI study found that "vocabulary size burdens programmers' working memory"
and that the data-flow metric DepDegree correlated most strongly with brain
activation ([Peitek et al., ICSE 2021](https://web.eecs.umich.edu/~weimerw/2024-481F/readings/peitek2021-metrics.pdf)).

**Language lever:** every extra concept required before the first useful
program is extraneous load. Every construct whose meaning depends on distant
context consumes working memory while reading.

### 1.3 Invalid assumptions and "invisible rules"

Ko, Myers, and Aung observed 40 non-programmers learning Visual Basic and
classified 130 barriers into six kinds: design, selection, coordination, use,
understanding, and information
([Ko et al. 2004](https://faculty.washington.edu/ajko/papers/Ko2004LearningBarriers.pdf)).
Key observations:

- Learners cross a barrier by making a *simplifying assumption*. When it is
  wrong, they hit a later barrier of a different kind. "A bigger concern is how
  prone … programming interfaces are to invalid assumptions *prior* to their
  use."
- Coordination barriers are "the invisible rules" of how parts combine; 20 of
  25 were crossed with wrong assumptions.
- 34 of 38 understanding barriers were insurmountable. Example: a message
  "expected: =" that did not say where or why.
- Defaults matter: learners assumed a `Timer` would start; it was disabled by
  default, and they rewrote correct code instead.

**Language lever:** make rules visible at the point of use, make defaults match
the common expectation, and make errors say what was right and wrong.

### 1.4 Syntax is a measurable barrier

Stefik and Siebert found that C-style Java and Perl gave novices no better
accuracy than *Randomo*, a language with randomly chosen keywords, while
Python, Ruby, and Quorum did better. Non-programmers rated `for`, `while`, and
`foreach` as the three least intuitive loop words
([Stefik & Siebert 2013](https://dl.acm.org/doi/10.1145/2534973);
[summary](https://neverworkintheory.org/2014/01/29/stefik-siebert-syntax.html)).
Hermans cites research finding that half of the programs 18–20-year-old
students write still contain a syntax error, and designed Hedy to introduce
syntax gradually
([Leiden 2020](https://www.universiteitleiden.nl/en/news/2020/12/programming-is-easy-to-learn);
[Hedy, ICER 2020](https://hedy.org/research/Hedy_A_Gradual_Language_for_Programming_Education_2020.pdf)).

### 1.5 Silent mistakes cost far more than loud ones

The Blackbox study measured 18 novice Java mistakes over two years, about 100
million compilations and more than 900,000 users
([Brown & Altadmri, TOCE 2017](https://kar.kent.ac.uk/57219/1/educators.pdf)).
Mismatched brackets were the most frequent mistake but had a median
time-to-fix of 17 seconds. The mistakes the compiler did **not** report hit the
1000-second measurement cap: string comparison with `==`, ignoring a non-void
return value, and `&` versus `&&`. Educators' beliefs about which mistakes
mattered did not match the data.

**Language lever:** the highest-value move is to turn silent semantic mistakes
into compile-time errors or make them impossible.

### 1.6 Small patterns cause large misreadings: atoms of confusion

Gopstein et al. isolated tiny code patterns and confirmed 15 of 19 as
significantly more misread than equivalent clear code, including pre- and
post-increment in expressions, assignment as a value, logic as control flow,
implicit type conversion, the conditional operator, operator precedence,
repurposed variables, omitted braces, and implicit predicates
([Gopstein et al., FSE 2017](https://ssl.engineering.nyu.edu/papers/gopstein_atoms_fse_2017.pdf)).
The Cloudbleed leak involved two of these patterns.

### 1.7 Composition is harder than constructs

Spohrer and Soloway found that novice bugs are dominated by *plan
composition*—combining correct pieces—rather than misunderstanding individual
constructs ([CACM 1986](https://dl.acm.org/doi/10.1145/6138.6145)). Pane,
Ratanamahatana, and Myers found non-programmers describe solutions with
aggregate operations ("all the items where…") rather than index loops
([IJHCS 2001](https://dl.acm.org/doi/10.1006/ijhc.2000.0410)).

### 1.8 Error messages

A 219-reference working-group review concluded that compiler messages remain
a major obstacle and distilled guidelines: increase readability, reduce
cognitive load, provide context, use a positive tone, show examples, show
solutions or hints, allow interaction, scaffold, use logical argument, and
report at the right time
([Becker et al. 2019](https://dl.acm.org/doi/10.1145/3344429.3372508);
[summary](https://neverworkintheory.org/2021/09/02/compiler-error-messages-considered-unhelpful.html)).
Barik et al. found developers prefer messages with proper argument structure,
and accept a weaker argument when it offers a resolution
([FSE 2018](https://doi.org/10.1145/3236024.3236040)). Enhanced messages give
mixed results partly because students do not read them or because load is
already high ([Prather et al. 2017](https://dl.acm.org/doi/abs/10.1145/3105726.3106169)).
Elm's "compiler errors for humans" became the reference bar
([Elm 2015](https://elm-lang.org/news/compiler-errors-for-humans)).

### 1.9 Cliffs, setup, and expectations

Forum and essay testimony is consistent:

- Setup and environment failures make newcomers extrapolate that everything
  will be that hard ([HN, "Learning to program is getting harder"](https://news.ycombinator.com/item?id=16394857)).
- A "sudden leap in difficulty" where basics start supporting follow-on
  concepts ([HN 2012](https://news.ycombinator.com/item?id=4932720)).
- Trautman's stages: the hand-holding honeymoon, then the "cliff of
  confusion" when support ends ([summary](https://mamchenkov.net/wordpress/2015/04/28/why-learning-to-code-is-so-damn-hard/)).
- Telling people it is easy prepares them for self-exclusion when it is not
  ([Hanselman 2016](https://www.hanselman.com/blog/stop-saying-learning-to-code-is-easy)).

A language-level cliff example: Swift 6 made data races compile errors, and
ordinary single-threaded UI code produced "a wall of Sendable violations." The
Swift project responded with main-actor-by-default "approachable concurrency"
whose stated goal is to "maintain progressive disclosure for non-concurrent
code" ([Swift vision](https://github.com/swiftlang/swift-evolution/blob/main/visions/approachable-concurrency.md);
[practitioner account](https://blakecrosley.com/blog/swift-6-2-concurrency-in-practice)).
Safety that fires before the learner uses the feature it protects is a cliff.

### 1.10 Implicitness is not the enemy; the reasoning footprint is

Turon's Rust ergonomics framework asks how much information is needed to
understand a line and how hard it is to find. An implicit feature is judged on
**applicability** (where it can happen, and whether there is a heads-up),
**power** (how much it can change behaviour), and **context-dependence** (how
far away the deciding information lives). A feature large on one axis should be
small on the other two. Rust's `?` is marked (low applicability surprise) even
though powerful; unrestricted implicit conversion is bad on all three
([Rust blog 2017](https://blog.rust-lang.org/2017/03/02/lang-ergonomics.html)).
Lattner adds the reading-side rule for rare features: "make the syntax clear so
that when you run up into it … you know that you don't know what it does"
([ATP 205 transcript](https://atp.fm/205-chris-lattner-interview-transcript)).
Hickey's "Simple Made Easy" separates *simple* (not interleaved) from *easy*
(familiar) ([InfoQ 2011](https://www.infoq.com/presentations/Simple-Made-Easy/)).

---

## Part 2. What a language can do about it

These principles restate the research as language obligations. Part 6 audits
Jet against them.

| # | Principle | Research basis |
|---|---|---|
| L1 | Small notional machine: few hidden rules, no hidden state, no action at a distance. | 1.1, 1.3, 1.10 |
| L2 | Turn silent mistakes into loud ones; prefer impossibility over detection. | 1.5 |
| L3 | Ban or fence confirmed atoms of confusion. | 1.6 |
| L4 | Defaults match the beginner's expectation; experts opt out. | 1.3 (Timer), 1.9 (Swift), philosophy pillar 1 |
| L5 | Every implicit feature keeps a small reasoning footprint. | 1.10 |
| L6 | Errors state what, why, and a concrete fix, in the learner's terms, at the right place, without internal vocabulary. | 1.3, 1.8 |
| L7 | No cliff: safety and power appear when the program uses them. | 1.9 |
| L8 | Rare or powerful features look unfamiliar and searchable. | 1.10 (Lattner) |
| L9 | Aggregate operations and composition patterns are first-class. | 1.7 |
| L10 | The first useful program needs almost nothing. | 1.2, 1.9 |

---

## Part 3. Reasoning about code

### 3.1 What the research says

- **State is the main source of complexity.** Each bit of state doubles the
  cases to simulate mentally; state "contaminates" stateless code that calls
  it ([Moseley & Marks, *Out of the Tar Pit*, 2006](https://curtclifton.net/papers/MoseleyMarks06a.pdf)).
- **Local reasoning.** Mutable value semantics upholds "the independence of
  values to support local reasoning": references are second-class, so
  variables never share mutable state
  ([Racordon et al., JOT 2022](https://www.jot.fm/contents/issue_2022_02/article2.html);
  [Hylo](https://hylo-lang.org/introduction/)).
- **Control flow must be visible.** Duffy on Midori: invisible `throw` control
  flow "is just as bad as `goto`"; requiring `try` at each call site made
  failure points "as easy to pick out as `return` statements"
  ([The Error Model, 2016](https://joeduffyblog.com/2016/02/07/the-error-model/)).
  Midori also made an ignored return value a compile error with an explicit,
  auditable `ignore`.
- **Bugs are not recoverable errors.** Duffy separates abandonment for
  programming bugs from typed, checked recoverable errors (same source).
- **Absence must be typed.** Hoare's "billion-dollar mistake"
  ([summary](https://en.wikipedia.org/wiki/Void_safety)).
- **Make invalid states unrepresentable; parse, don't validate**
  ([King 2019](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate)).
- **Function colouring is a reasoning cost** when an effect must be threaded
  through every caller ([Nystrom 2015](https://journal.stuffwithstuff.com/2015/02/01/what-color-is-your-function/)).
- **Some patterns are measurably misread** (1.6); loops are harder than
  conditionals and some negations add difficulty
  ([Ajami et al., EMSE 2019](https://link.springer.com/article/10.1007/s10664-018-9628-3)).

### 3.2 Jet today: what already supports reasoning (keep)

Each row was checked in code or by a probe.

| Keep | Evidence |
|---|---|
| Immutable `::` by default; mutation of a `::` name is E0111 with a plain fix. | probe p06; `Syntax.rs:21-22` |
| Write access is visible at the signature and call site (`&T`, `bump(&c)`); editing a read parameter is E0205. | probe p14; `Examples/features/effects/all_sigils.jet` |
| Aliasing conflicts are rejected with a plain explanation (E0212). | probe p15 |
| No truthiness: `if n` on an `Int` is E0110. | probe p07 |
| `=` in a condition is E0322 with "use `==`". | probe p01 |
| Same-scope shadowing is E0118 (removes the *repurposed variable* atom). | probe p11 |
| `Int` is exact and arbitrary-precision; `9223372036854775807 + 1` prints `9223372036854775808`. | probe p10; D-INTBIG1 |
| `/` on `Int` never truncates silently: `7 / 2` prints `3.5` (a `Fraction`); removes the *type conversion* atom `(double)(3/2)`. | probes p13, p23 |
| Decimal literals are exact `Decimal` by default: `0.1 + 0.2` prints `0.3`. Crossing to binary `Float` is an explicit type error with a clear message (E0112). | probes p20, p22, p23 |
| No null: `?T` with `None`/`Val`; E0108 on a bare value. | probe p21; `Examples/features/types/option.jet` |
| Exhaustive dispatch over enums (E0307). | probe p19 |
| Bugs stop; world failures are values (D-FAIL-MODEL1), matching Duffy's split. Out-of-bounds stops with "the list has 3 items, so position 5 doesn't exist". | probe p09; `syntax-decisions.md:1482-1488` |
| Fallible results must be used; `.drop("reason")` is the auditable discard (Midori's `ignore`). | `Examples/features/errors/discard_fallible.jet` |
| Effects are inferred, `-[]>` proves purity, and effect rows can bound callers. | `Examples/features/effects/effects.jet` |
| Unsafe code is fenced by `#Unsafe("reason")`. | `Examples/features/lowlevel/rawptr.jet` |
| Change-level reasoning tools exist: `jet review`, `jet diff` by meaning, `jet audit` for implicit copies. | `jet help` output |

### 3.3 Jet today: findings

**F1 — Failure propagation is invisible at call sites, and signatures can hide
fallibility.** S7 and D-FAILURE-FOUNDATION1=A make "plain fallible calls
propagate automatically", and "a callable that omits its error contract is
implicitly fallible with the default `Err`" (`syntax-decisions.md:1454,
7481-7490`). Probe p25 shows the consequence: `fn double(raw: String) -> Int`
has no failure contract, yet it propagates a failure from `parse`, and so does
`total()`, whose two calls look identical to infallible calls. The program
prints `start` and then exits with the failure; nothing in `total` or `double`
tells a reader that either line can leave the function.
Against Turon's footprint: applicability is universal (any call), power is
high (it exits the function), and context-dependence is transitive (you must
inspect the callee's body, recursively). Against Duffy, this is the exact
property Midori's call-site `try` removed. The ratification's tradeoff clause
requires hover, diagnostics, and inspection to make the route visible (card
#2388 body); that helps editor readers but not code review, diffs, or printed
code. The dogfood scorecard rated reasoning 3.1/5 (card #2393).
Priority: highest, because reasoning is Jet's most critical goal. Owner gate.

**F2 — A propagated default-error failure prints no location.** Probe p25,
with and without `--verbose`, prints only `Error: empty input`. D-FAIL-MODEL1=A
says "every failure is one product: a report with a code, message, why, fix,
and source location", and D-ERRCTX1 says every fallible call joins the failure
journey (`syntax-decisions.md:1476-1488`). The learner receives neither the
origin line nor the path. This is an *information barrier* (Ko 1.3) and an
apparent spec-compliance gap. **(recheck)**

**F3 — Pre- and post-increment are expressions.** D-INCR1=A:
`Examples/features/basics/increment.jet` teaches `loop --lives > 0` and
`print(n++)`. Probe p17 accepts `j :: i++ + ++i` and prints `4`. Pre- and
post-increment are both confirmed atoms of confusion (1.6). Jet already avoids
most other atoms; this one is deliberately admitted.

**F4 — The binding sigil silently changes ownership.** Probe p15:
`ys :: xs` makes `ys` a live read view of `xs`, so `xs.push(4)` is E0212.
Probe p24: `ys := xs` *moves* `xs` with no `^`, so later use is E0121. At
call sites a consuming parameter requires `^name`
(`Examples/features/memory/ownership.jet`). One character (`:` versus `=`)
therefore selects borrow versus move as well as immutable versus mutable, and
the move is unmarked only in this position. That is a hidden dependency in the
cognitive-dimensions sense and violates L1 and L5.

**F5 — E0121 for a binding move leaks placeholder text and byte offsets.**
Probe p24 prints "`xs` was consumed by `the earlier consuming operation`" and
"move site: source bytes 235..237", with no caret on the move. Card #2387
(done) made E0121 name the consuming callee for call moves; the binding-move
case still falls back to placeholder text.

**F6 — `else` after an exhaustive enum dispatch is accepted silently.**
Probe p24 (second version) checks clean with `.Red`, `.Green`, `.Blue`, and
`else`. Adding a variant later silently routes it to `else`, defeating E0307.
The flagship example `Examples/features/basics/pattern_matching.jet:12-17`
teaches exactly this shape.

**F7 — Discarding a non-unit result is silent.** Probe p03 calls
`double(3)` as a statement and checks clean. Must-use covers fallible results
and `#MustUse` items only. Blackbox's *ignored non-void result* reached the
1000-second median fix time (1.5). Jet can do better than Java here: its effect
inference already knows when a call is pure, and discarding the result of a
pure call is almost always a bug.

**F8 — Mixed `&&` and `||` need no parentheses.** Probe p12 accepts
`a || b && c`. *Operator precedence* is a confirmed atom. D-ARMHEAD-PAREN1=A
already requires grouping in dispatch arm heads (`syntax-decisions.md:7492`),
so the policy exists in one place but not in ordinary expressions.

**F9 — Two remainder operators with opposite sign rules.** `%` is floored
(`-7 % 2` is `1`) and `%%` is truncated (`-7 %% 2` is `-1`); `/%` is floor
division (`math_layout.rs:428-436`, probe p13). Each is defensible, but the
pair differs by one character and inverts the C/Java/JavaScript meaning of
`%`, a known transfer trap (Hermans on unlearning, 1.2).

**F10 — Numeric literal type depends on context.** `0.1 + 0.2` is `Decimal`
and equals `0.3`; the same literals passed to a `Float` parameter produce
`0.30000000000000004` (probes p20, p22). The type error at the boundary is
clear, so this is a moderate context-dependence cost, not a defect. It is
worth showing the literal type in hover and inlay hints.

---

## Part 4. Reading code

### 4.1 What the research says

- **Words beat symbols for novices.** C-style syntax performed like random
  syntax; the most common loop words rated least intuitive (1.4).
- **Punctuation and identifiers per line hurt.** In Buse and Weimer's model,
  average identifiers per line, line length, and brackets, parentheses, and
  punctuation had the greatest negative effect on readability; blank lines
  mattered more than comments ([TSE 2010](https://web.eecs.umich.edu/~weimerw/p/weimer-tse2010-readability-preprint.pdf)).
- **Descriptive names are faster.** Developers found defects about 14% faster
  with descriptive identifiers (Hofmeister, Siegmund & Holt, SANER 2017;
  [journal version](https://dl.acm.org/doi/10.1007/s10664-018-9621-x)).
- **Labels carry meaning.** Victor contrasts HyperTalk's
  `drag from "0,0" to "100,100" with optionKey` with
  `dragMouse(0, 0, 100, 100, OPTION_KEY)`: "A learner must be able to look at a
  line of code and know what it means"
  ([Learnable Programming](https://worrydream.com/LearnableProgramming/)).
- **Rare features must announce themselves** (Lattner, 1.10).
- **Static types act as documentation** that the compiler keeps current
  ([Endrikat et al., ICSE 2014](https://dl.acm.org/doi/10.1145/2568225.2568299)).
- **Reading aloud helps comprehension**, so syntax should be pronounceable
  ([Hermans, Strange Loop 2019](https://www.thestrangeloop.com/2019/how-to-teach-programming-and-other-things.html)).
- **One glyph, one meaning.** The cognitive-dimensions framework names
  consistency, role-expressiveness, and hidden dependencies as notation
  properties ([Green & Blackwell tutorial](https://www.cl.cam.ac.uk/~afb21/CognitiveDimensions/CDtutorial.pdf)).

### 4.2 Jet today: what supports reading (keep)

| Keep | Evidence |
|---|---|
| One way to build text (interpolation); `+` on text is E0109. | probe p02 |
| Labelled arguments, label-only zones, and editor ghost text for positional calls. | `Examples/features/basics/named_args.jet` |
| Keywords are few and conventional (`fn`, `if`, `loop`, `return`, `struct`, `enum`); no `for`/`while`/`foreach` split. | `Examples/features/basics/` |
| `{expr=}` debug interpolation shows name and value together. | `Examples/features/basics/interp_debug_label.jet` |
| Every keyword and sigil has one registry home with a decision ID (I7). | `crates/jet-foundation/src/Syntax.rs:1-59` |
| `jet explain <glyph>` exists (card #2045). | probe below |
| A canonical formatter (`jet fmt`). | `jet help` |

### 4.3 Jet today: findings

**F11 — Several glyphs carry two or more meanings, separated only by
position.**

| Glyph | Meanings | Evidence |
|---|---|---|
| `^` | prefix move (`^acc`); infix power (`2 ^ 8`); task capture | `math_layout.rs:442-444`; `Syntax.rs:445-447` |
| `&` | write-access parameter and argument; bitwise AND | `core_surface.rs:288`; `math_layout.rs:437` |
| `!` | error contract in signatures; logical and bitwise NOT; deny root | `Syntax.rs:47`; `math_layout.rs:448-450` |
| `~` | prefix copy; part of `~|` exclusive-or | `math_layout.rs:445-447` |
| `/`, `*` | division and multiplication; positional-only and label-only parameter separators | `named_args.jet:34` |
| `T{…}` | struct construction; typed literal (`Float{3.4}`); field or parameter default (`Int{30}`, `String{"world"}`); checked text head (`Name{"…"}`); typed list literal (`[Int]{1, 2}`) | `Syntax.rs:50-51`; E0112 text in probe p23; `named_args.jet:34`; `first_hour.jet` |
| `==` | equality; pattern-dispatch table head (`if c == { .Red -> … }`) | `pattern_matching.jet:12` |
| `#` | applied marker (`#CLI`, `#[Doc(…), Short("v")]`); fixed-size array separator; package version separator | `Syntax/package_files.rs:88-99` (ReadingSurface scout) |
| `@` | compile-time block, name, or fact; package-source reference | `Syntax.rs:29`; `Syntax/markers.rs:1-11` |
| `\|` | or-pattern alternative; bitwise OR | `math_layout.rs:438-441` |

A signature such as
`fn connect(host: String, /, *, timeout seconds: Int{30}, tls: Bool{true})`
(`named_args.jet:34`) packs five punctuation roles into one line, which is the
feature set Buse and Weimer found most harmful. Each choice has a ratified
reason; the finding is the aggregate reading cost (L8).

Vocabulary size compounds this. The live keyword list has 55 spellings
(`Syntax/package_files.rs:444-551`), and the marker registry has 85 current
`#Name` markers (`crates/jet-codegen/src/Prelude/Markers.jet:24-253`), counts
from the ReadingSurface scout pass. Peitek et al. found vocabulary size loads
working memory (1.2). Examples also add avoidable punctuation:
`Examples/features/basics/branches.jet` calls `describe(Float{-5.0})`, while
probe p26 shows `describe(-5.0)` runs with the same output.

**F12 — `jet explain` answers glyphs in internal vocabulary.** Probe:
`jet explain ^` prints "SIGIL_MOVE: registered sigil `^`" and "Why Jet
enforces it: owner decision D-CAP2". `jet explain ??` prints "OP_FALLBACK".
It does not mention that infix `^` is power, give an example, or say what
happens to the old name. A learner who meets `^` in someone else's code (the
Lattner case) gets an internal constant name and a decision ID.

**F13 — A binding cannot state its type.** Probe p04: `x: Float :: 3 / 2` is
E0003, "types ride the value". The ways to state a type are constructors and
typed literals (`Float{…}`), which are the overloaded `T{…}` forms of F11.
Annotating an intermediate result is a documented reading aid (Endrikat 2014;
Rust keeps it optional). The current rule forces the reader to infer or
hover.

**F14 — Pattern dispatch is spelled with `==`.** Reading `if c == { … }` aloud
("if c equals…") gives the wrong meaning: it is a multi-way match, and arms can
bind names (`.Val(n)`). `==` is also a boolean operator on the same line types.
This is a role-expressiveness cost; a learner is likely to predict equality.

---

## Part 5. Writing code

### 5.1 What the research says

- **Ergonomics is interruptions, not keystrokes.** "What matters much more is
  how easy it is to remember the right one to type"; pedantic errors about
  things that do not matter yet break flow (Turon, 1.10).
- **Progressive disclosure.** "The complexity inherent in the language needs to
  be progressively disclosed" ([Lattner, ATP 205](https://atp.fm/205-chris-lattner-interview-transcript));
  Swift 6's concurrency cliff shows the cost of violating it (1.9).
- **Gradual syntax.** Hedy's levels let children control difficulty and
  reduced syntax overload (1.4); Jet's one-mechanism rule excludes language
  levels, but the lesson about *order of exposure* still applies.
- **Fast feedback and seeing values.** Pane's heuristics include "support
  incremental testing and feedback" and "help detect, diagnose, and recover
  from errors" (quoted in Ko 2004); Victor's "create by reacting".
- **Examples are the main discovery path.** Learners copy working examples
  and then hit use and coordination barriers adapting them (Ko 2004;
  [Wang et al. 2021](https://arxiv.org/pdf/2104.11806)).
- **Pit of success.** "It must be easy for developers to do the 'right' thing
  … almost as if by accident" (Duffy 2016).

### 5.2 Jet today: what supports writing (keep)

| Keep | Evidence |
|---|---|
| `fn run() { print("hello, world") }` is the whole hello world; no `main` signature, imports, or class. | `Examples/features/basics/hello.jet` |
| Typo suggestions: `totel` → "Did you mean `total`?" (E0107). | probe p05 |
| Plain-language type names in errors: "Int (a whole number)", "Decimal (an exact base-10 number)". | probes p07, p23 |
| Every diagnostic has what, why, fix, and a `More:` link; `jet explain CODE`. | all probes |
| `jet learn`: predict, check, explain, controlled edit, transfer. | `Examples/learn/README.md` |
| REPL, `jet dev` watch loop, `jet fix`, LSP with inlay hints and code actions. | WritingAndFeedback scout, `Source/LSP/Server.rs:369-407` |
| Foreign habits get teaching errors (`async`/`await`, `let`/`var`, `+` on text). | `tests/ui/async_await_teaching.stderr`; probe p02 |
| Aggregate operations on collections (`map`, `filter`, eager by default). | `Examples/features/collections/` |

### 5.3 Jet today: findings

**F15 — The `jet new` scaffold teaches too much and did not run on these
builds.** The generated `run.jet` (source: `Source/CmdCompile.rs:5261`)
contains a `#CLI` struct with a `#Doc` marker and a `String{"world"}`
default, a closure passed to `ui.reactive_render`, `ui.null_backend()`, and
`ui.constraint(0.0, 0.0, 320.0, 80.0)`, next to four `@*.jet` override files.
The first-hour guide says `jet run` prints `hello, world`
(`Docs/spec/guides/first-hour.md:92-103`) and does not mention the UI code.
On both working-tree binaries, `jet run` in a fresh scaffold failed with seven
errors, including errors located in `Core/ui/ui.jet` and
"`ui.mount` needs a `UiNode` tree, but the second argument is
`<corelib>/Core/ui::Core/ui/ui.jet::UiNode`", with each of the first two
errors printed twice. **(recheck)** on a clean build; the concept load is
independent of the build.

**F16 — Success output from `jet check` is noisy.** A clean `jet check
file.jet` prints a key=value status line and four `proof: output=not-applicable
… (diagnostic=E2389)` lines before "ok" (probe p03). A learner reads five
lines of internal terms to learn "no problems".

**F17 — Missing-return error explains grammar, not the gap.** Probe p08
(`sign` returns for `n > 0` and `n < 0` only) gives E0114: "its final
statement produces no value … statements and semicolon-terminated expressions
yield unit". It does not say "when `n` is `0`, `sign` reaches the end without
a value". Jet has no semicolons in ordinary code (D-SEMI1), yet the message
mentions them.

**F18 — An unclosed parenthesis reports a `;` that is not in the source.**
Probe p16 (`print("hello"` then `}`) gives "Expected `,` between arguments,
found `;`". The implicit statement terminator leaks into the message, and the
error does not point at the unclosed `(`. Brackets were Blackbox's most
frequent novice mistake.

**F19 — Authority errors on a print-only program read as a data dump.** A
`package.jet` without an authority block (probe package before the fix) makes
`print` fail with E1803, whose Why line is
`required_effects=IO; granted_effects=; denied_effects=; …`. Card #2259 (done)
fixed the scaffold and fix text; a hand-written or older manifest still hits
this wall on the first `print`. This is an L7 cliff: authority safety fires
before the program does anything a learner would consider risky.

**F20 — A loose file inherits a distant package.** Checking a file in
`~/.cache/jet-dev/scratch/learn-research/probes/` reported E1334 about a
symlink in an unrelated sibling project, because an ancestor directory holds a
`package.jet`. The message names the symlink but not the package root that
pulled it in. (Environment finding; recorded because it blocked the first
probe run.)

**F21 — `jet help` leads with expert commands.** The global screen lists more
than 50 commands, starting with `registry`, `db`, `inspect`, and `bind`
before `run` and `check`. Ko's selection barrier: a learner cannot find the
five verbs the first-hour guide uses.

**F22 — Evidence that writing and reading lag.** The dogfood campaign's
scorecard was reading 3.6, writing 2.9, reasoning 3.1, creating 2.8,
modifying 2.8, diagnostics 2.6, tooling/docs 2.7, and 9 of 9 measured blind
preferences chose Rust (card #2393;
`Docs/proposals/dogfood-jet-experience-5-of-5.md:271-302`). The r2 rerun has
not run. Those scores predate several fixes and were affected by compiler
defects, so they are the latest measurement, not the current state.

---

## Part 6. Proposals

Ordered by the priority reasoning > reading > writing, then by expected
effect. "Owner gate" marks choices AGENTS.md reserves to the owner (syntax,
semantics, public API, product behaviour). Nothing here is ratified.

### 6.1 Reasoning

**P1 — Make every exit point visible in source (F1; owner gate).** Options,
all preserving the implicit beginner route for writing:

- A. Keep propagation implicit; require that a function without a written
  contract that can fail shows `!Err` in its **generated signature view**
  (hover, `jet doc`, `jet review`), and add a formatter-maintained marker.
  Lowest cost; still invisible in plain text and diffs.
- B. A one-token call-site marker on propagating calls (Swift `try`, Rust
  `?`), inserted by `jet fix` and the formatter so beginners never type it.
  Reading gets Duffy's property; writing cost is near zero because tools add
  it. Conflicts with S7; needs a ballot.
- C. B, but only in functions whose failure contract is written, leaving
  beginner code implicit. Mixed; two reading rules.

Recommendation: B, because the reader, not the writer, is the priority, and
the tool can do the writing. Edge cases for the ballot: closures, `??`
chains, `?(text)` context, and method chains.

**P2 — Signatures do not lie (F1; owner gate).** Whatever P1 chooses, a
signature written as `-> Int` should not be silently fallible. Candidate rule:
functions with no written contract may be fallible only when private to the
file; public functions state `!Err` explicitly, and `jet fix` writes it.

**P3 — Report location for every propagated failure (F2).** The default
report prints the origin line and the propagation path, as D-FAIL-MODEL1
already requires. Implementation, not a new mechanism.

**P4 — Retire expression-position `++`/`--` (F3; owner gate).** Keep
`x++` and `x--` as statements if the owner values the spelling; reject them
as operands. This removes a confirmed atom at almost no expressive cost
(`x += 1` remains).

**P5 — One move marker everywhere (F4; owner gate).** A whole-value move from
a named place is always spelled `^name`, including `ys := ^xs`; `ys := xs`
of a non-copy value is a teaching error with a `jet fix` edit. Separately,
document that `::` of a place is a view.

**P6 — Warn on `else` after closed exhaustive arms (F6).** A lint with a fix
that deletes the arm; update `pattern_matching.jet` to teach exhaustive
dispatch without `else`.

**P7 — Discarded pure results are errors (F7).** When effect inference proves
a call pure (`-[]>`) and its non-unit value is dropped, report a registered
diagnostic; `.drop("reason")` stays the escape. This uses a Jet-specific fact
to beat Blackbox's worst class of silent mistake with few false positives.

**P8 — Parenthesize mixed `&&`/`||` (F8).** Extend D-ARMHEAD-PAREN1's rule
and fixit to ordinary expressions.

**P9 — Show inferred facts where the reader looks (F1, F10).** Hover, `jet
doc`, and `jet review` show the inferred effect row, failure contract, and
literal types; `jet review` flags a change that makes a function newly
fallible or impure. This is "a clear place to look" for every implicit fact.

### 6.2 Reading

**P10 — A glyph budget (F11; owner gate for each change).** Add a registry
check that lists each glyph's meanings and fails when a new meaning is added
without a ballot that names the reading cost. Candidates to reconsider:
infix `^` for power (a `pow`-style word or `**` avoids colliding with move),
and the `T{…}` default form (`= default` or a `default` word would separate
defaults from construction).

**P11 — Plain-language glyph explanations (F12).** `jet explain ^` prints each
meaning by position with a two-line example, what happens to the old name, and
the related diagnostic codes. Internal constant names and decision IDs move to
`--verbose`.

**P12 — Optional binding type ascription (F13; owner gate).** Allow
`x: Float :: …` as a checked assertion with no conversion power, so it adds
reading information without a second conversion mechanism.

**P13 — Consider a dispatch word (F14; owner gate).** A keyword such as
`match c { … }` (or `if c is { … }`) would read aloud correctly and keep `==`
boolean-only. Evaluate with a predict-the-output comparison (P20).

### 6.3 Writing

**P14 — A print-only scaffold (F15).** `jet new` produces
`fn run() { print("hello, world") }` plus one `#Test`; UI, CLI structs, and
override files move behind `jet new --template …` or appear only when
requested. Re-run the scaffold on a clean build to confirm or clear the error
half of F15.

**P15 — Quiet success (F16).** Human `jet check` success prints one line;
proof rows move to `--verbose` and `--json`.

**P16 — Diagnostics that name the gap (F5, F17, F18, F19).**
- E0114: name the path that falls off the end ("when `n` is `0`").
- Parser: report the unclosed `(` with its line; never mention an implicit `;`.
- E0121: name the binding move site by line and column with a caret.
- E1803: a plain Why sentence ("this program prints, and `package.jet` has not
  allowed printing yet"), with the data behind `--verbose`.

**P17 — Beginner authority without a manifest cliff (F19; owner gate).**
Standard output and argv run without a grant in a package with no authority
block, and experts opt into deny-by-default, matching pillar 1's "experts opt
out". This is product behaviour and needs a ballot.

**P18 — Tiered help (F21).** `jet help` shows the first-hour verbs first and
moves the rest under `jet help all`.

**P19 — Name the package root in file-scope errors (F20).** When an ancestor
`package.jet` determines scope, say which one.

### 6.4 Measuring it

**P20 — A comprehension cell for new surfaces (process; owner gate).**
AGENTS.md already requires a paired two-program performance cell for any
surface added for performance. Mirror it for reading: a new syntax or
semantics proposal ships a small *predict-the-output* pair (new form versus
the plain form), in the style of Gopstein's atom tests, run through `jet
learn`'s existing predict-check loop with agents or volunteers. Ratification
waits until the new form is predicted at least as accurately. This turns
Stefik's "do basic usability testing" into a gate, and gives the owner
evidence for P4, P10, P12, and P13.

**P21 — Run the dogfood r2 rerun (F22).** Card #2393 is ready and holds the
only end-to-end measurement of reading, writing, and reasoning.

---

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 | no-action | report-only run: proposals P1, P2, P9 recorded; ratified S7 and D-FAILURE-FOUNDATION1 need an owner ballot, not authorized here |
| F2 | no-action | report-only run: recommendation P3 recorded; implementation is not authorized here |
| F3 | no-action | report-only run: P4 recorded; reopening D-INCR1 is an owner choice |
| F4 | no-action | report-only run: P5 recorded; owner choice |
| F5 | no-action | report-only run: residual gap after done card #2387 recorded as P16 |
| F6 | no-action | report-only run: P6 recorded; implementation is not authorized here |
| F7 | no-action | report-only run: P7 recorded; implementation is not authorized here |
| F8 | no-action | report-only run: P8 recorded; implementation is not authorized here |
| F9 | no-action | report-only run: evidence only; covered by the P10 glyph review |
| F10 | no-action | report-only run: P9 recorded; current behaviour is intentional |
| F11 | no-action | report-only run: P10 recorded; each glyph change is an owner choice |
| F12 | no-action | report-only run: P11 recorded; implementation is not authorized here |
| F13 | no-action | report-only run: P12 recorded; owner choice |
| F14 | no-action | report-only run: P13 recorded; owner choice |
| F15 | no-action | report-only run: P14 recorded; scaffold failure needs a clean-build recheck |
| F16 | no-action | report-only run: P15 recorded; implementation is not authorized here |
| F17 | no-action | report-only run: P16 recorded; implementation is not authorized here |
| F18 | no-action | report-only run: P16 recorded; implementation is not authorized here |
| F19 | no-action | report-only run: residual after done card #2259 recorded as P16 and P17 |
| F20 | no-action | report-only run: P19 recorded; implementation is not authorized here |
| F21 | no-action | report-only run: P18 recorded; implementation is not authorized here |
| F22 | card | #2393 |
<!-- /audit-dispositions -->

---

## Appendix A. Probe programs

All probes ran from `~/.cache/jet-dev/learn-probes/` with the binary noted in
"Method". Output excerpts appear in the findings.

| Probe | Source | Result |
|---|---|---|
| p01 | `x := 3` / `if x = 4 -> print("four")` | E0322 |
| p02 | `b :: "sta" + "rt"` | E0109 (interpolation is the one way) |
| p03 | `fn double(n: Int) -> Int { n * 2 }` / `double(3)` as a statement | clean; runs |
| p04 | `x: Float :: 3 / 2` | E0003 "types ride the value" |
| p05 | `print(totel)` with `total` in scope | E0107 "Did you mean `total`?" |
| p06 | `count :: 0` / `count = count + 1` | E0111 |
| p07 | `n := 0` / `if n -> …` | E0110 |
| p08 | `sign` returns only for `n > 0` and `n < 0` | E0114 (see F17) |
| p09 | `xs :: [1, 2, 3]` / `print(xs[5])` | stop E3010, exit 70 |
| p10 | `big := 9223372036854775807` / `big += 1` | prints `9223372036854775808` |
| p11 | `x :: 1` / `x :: 2` | E0118 |
| p12 | `print(a \|\| b && c)` | clean; `true` |
| p13 | `print(7 / 2)`, `print(-7 / 2)`, `print(-7 % 2)` | `3.5`, `-3.5`, `1` |
| p14 | `fn bump(p: P) { p.x += 1 }` | E0205 |
| p15 | `ys :: xs` / `xs.push(4)` | E0212 (view) |
| p16 | `print("hello"` then `}` | E0003 "found `;`" |
| p17 | `i := 1` / `j :: i++ + ++i` | clean; `4` |
| p18 | explicit `!Err` propagation through `load()` | `-1` via `??` |
| p19 | `if c == { .Red … .Green … }` over three variants | E0307 "missing: Blue" |
| p20 | `print(0.1 + 0.2 == 0.3)` | `true` (Decimal) |
| p21 | `?Int` return of a bare `Int`; `none` | E0108 "Wrap it with `Val(...)`"; E0107 |
| p22 | `fn add(a: Float, b: Float)` / `add(0.1, 0.2)` | `0.30000000000000004`; L0502 on `x == 0.3` |
| p23 | `a :: 0.1` passed to a `Float` parameter; `1 / 3` returned as `Float` | E0112 (Decimal vs Float); E0113 (`Fraction`) |
| p24 | `ys := xs` / `xs.push(4)`; exhaustive arms plus `else` | E0121 with placeholder text; `else` accepted |
| p25 | `fn double(raw: String) -> Int { n :: parse(raw) … }`, `total()` calls it twice | prints `start`, then `Error: empty input`, exit 1 |
| p26 | `fn describe(celsius: Float)` / `describe(-5.0)` | runs; prints `-5.0` (no `Float{…}` needed) |
| scaffold | `jet new hello_probe` then `jet run` | 7 errors (see F15) **(recheck)** |
| glyphs | `jet explain ^`, `~`, `??`, `::` | internal constant names and decision IDs (F12) |
