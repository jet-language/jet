# Choosing: one table, one pattern grammar

Proposal, 2026-08-07. First-principles audit of patterns, matching, and control flow. Owner choices are the four D-CHOOSE ballots on the audit card. Every line of code marked "proposed" is not ratified; everything else compiles today or is already ratified law.

## Executive summary

Jet already made the big call: there is no `match` keyword. One `if` builds every branch, one `loop` builds every repeat, and today's D-CONC-CHAN2 ruling put the channel wait on the same arm table. That call is right, and this audit does not reopen it.

The finding is that the pieces of that one construct still speak four dialects. Jet has two pattern grammars doing one job: the arm grammar knows ranges, or-patterns, and string patterns but not lists; the bind grammar knows lists and tuples but not ranges or strings. The loop head special-cases `(key, value)` instead of taking a pattern. A search loop that finds nothing has no answer, so for-else lives nowhere. Multi-head functions are a second costume for the arm table, flagged twice by the isomorphic audit and never balloted. And ratified prose still disagrees with ratified law: the yielding-loops proposal spells loop headers with `;` that D-LOOP-COMMA1 retired, and 33 frozen codegen tests still parse the dead grammar.

The one idea: **a program chooses in exactly one way — an ordered table of heads; the first head that fits binds its names, teaches the checker its facts, and runs its body; a miss falls to the next head; a table that runs out follows the failure rail.** Every ratified rule in this area is already a theorem of that law. The four ballots finish the law's reach: one pattern grammar in every pattern position (D-CHOOSE-PAT1), one draw-head shape for loops and waits (D-CHOOSE-DRAW1), searches that find nothing ride `??` (D-CHOOSE-FIND1), and multi-head functions become sugar for the table (D-CHOOSE-HEADS1).

What the owner gets on the page: `[first, ..rest] :: xs ?? return` works; `"v{major:Int}.{minor:Int}" :: version ?? return usage()` parses and binds in one line; `loop .{name, age}, users { }` destructures in the head; `hit :: loop u, users if u.id == 7 -> break u ?? None` is find-first with for-else for free; and `fn area(...)` multi-heads are the same table wearing a signature. No mechanism is added. One grammar, one special case, and one unfiled gate are deleted.

What does not change: `if` stays the only branching keyword (S68). The comma law (D-LOOP-COMMA1) stands. The comma-less guard (D-LOOP-GUARD1) stands. The wait table (D-CONC-CHAN2=D) stands, and this proposal names one wall around it on purpose: wait arms take no guards and no refutable patterns, because a filter after a draw would lose the drawn item.

## The problem, briefly

Every construct below answers the same question — "which body runs, with which names bound?" — and each answers it with its own grammar, its own parser, and its own gaps.

| The same job | Spelled today | Home | Defect |
|---|---|---|---|
| Test a value, run a body | `if code == { 200 -> ok() 301 \| 302 -> redirect() else -> log(code) }` | S68; `crates/jet-parser/src/Parser/Statements/conditionals.rs:63` | canonical — this is the law's home |
| Test conditions in order | `if { a > b -> "up" else -> "flat" }` | D-IFGUARD1; `conditionals.rs:70` | canonical |
| Wait on sources | `if { job, jobs -> handle(job) after 100ms -> retry() }` | D-CONC-CHAN2=D, ratified 2026-08-07 | ratified, unbuilt; shipped builder chain still live with a phantom `SelectBuilder` type |
| Repeat over a source | `loop job, jobs { handle(job) }` | S19; `crates/jet-parser/src/Parser/Statements/control.rs:1195` | binding is a name or the special pair `(k, v)`, never a pattern |
| Filter while repeating | `loop u, users if u.active -> u.name` | D-LOOP-GUARD1=A | desugars to an arm table internally (`control.rs:201`); the comma spelling misparses as a stride (#1419) |
| Bind or bail | `Val(n) :: maybe_port() ?? return` | S74, `docs/spec/syntax-decisions.md:523` | spec law with no parser path, no test, no card — promised, unbuilt |
| Handle failure | `if r == { .Ok(n) -> use(n) .Err(e) -> log(e) }` plus `?? default / return / break / next / panic` | S34; `AST/patterns.rs:249` | the `??` routes are the table's else-arm in operator clothing; nobody says so |
| Dispatch on argument shape | `fn area(Circle(r: Float)) => Float = ...` / `fn area(Rect(...)) => ...` | S83, `syntax-decisions.md:262` | second costume for the arm table; isomorphic audit carried the owner-gate twice (07-27, 07-28), never filed |
| Patterns for arms | `enum Pattern`: variants, ranges, or, struct, string, binary | `crates/jet-foundation/src/AST/patterns.rs:37` | no list or tuple patterns |
| Patterns for binds | `enum BindPattern`: struct, list, tuple | `AST/patterns.rs:210` | no ranges, no or, no strings; list form has no rest (`..`), exact length only (E0315) |

And the written record disagrees with itself:

| Conflict | Side A | Side B |
|---|---|---|
| Loop-header separator | D-LOOP-COMMA1=A: "the semicolon disappears from loop headers entirely" | `docs/proposals/yielding-loops.md:19,30,72` still teaches `loop user; users`; D-LOOP-HEADER3's own ratified option text shows `;`; 33 frozen jet-codegen unit tests parse the dead grammar (#1650) |
| The wait spelling | D-CONC-CHAN2=D (2026-08-07): subjectless `if` table | `docs/proposals/concurrency-work-is-a-value.md:219` shows a `select` keyword; `docs/spec/spec.md:2351` still documents the builder chain |
| The construct's name | Spec: "`if` is the only branching keyword" (S68) | Compiler: `Stmt::Switch`, `KW_SWITCH = "if"`, `switches.rs` — code readers conclude Jet has a `switch` |
| Tier parity | I9: one meaning on every tier | `tests/jit_gaps.txt:442,454-456`: value-position `if` pattern dispatch unsupported on the JIT across four stems |

## The law

State it once, then read every ratified rule as an instance of it.

**A choice is an ordered table. Each head is tried in order. A head that fits binds its names, teaches the checker its facts, and runs its body. A head that misses falls to the next. A table that runs out follows the failure rail.**

- S68's "first matching or true head wins" — the law's ordering clause.
- D-IFGUARD1's ordered guards — heads that are plain `Bool` expressions.
- D-CONC-CHAN2's wait — the same table where a head fits when its source is ready. `after 100ms` is a head whose source is the clock.
- A `loop` — a one-head table run repeatedly; the "next arm" is the next item.
- D-LOOP-GUARD1's guard — an arm guard on the loop's one head; the compiler already desugars it to an arm table (`control.rs:201`).
- The `??` routes (`default`, `return`, `break`, `next`, `panic`) — the else-arm of a one-head table, written as an operator. S74's `Val(n) :: maybe_port() ?? return` is that table.
- `else`, `??`, and E0307's exhaustiveness proof — three faces of "the table ran out": handle it, route it, or prove it cannot happen.
- D-FLOWTYPE1's narrowing and D-FACT-FLOW1's one flow-fact store joining "at every if, loop, and arm table" — the "teaches facts" clause. A fit is knowledge, and the checker's ledger records it. The expert reads that ledger with `$x` (D-FACT-READ1).

Nothing above is new. That is the point: the law is already ratified in pieces. The four ballots below extend it to the places it does not reach yet.

## Element 1 — one pattern grammar (D-CHOOSE-PAT1)

Today the arm grammar and the bind grammar are two separate enums with two parsers, and each has powers the other lacks. Merge them: **one pattern grammar, legal in every pattern position** — arm heads, `::`/`:=` binds, loop bindings. A pattern is a pattern wherever it stands.

What the beginner types today stays exactly as it is. Every line below works now and keeps working:

```jet
first :: names[0]
.{id, severity: sev, ..} :: incident        // struct destructure (S74)
[a, b] :: point                             // list destructure, exact length
loop (key, value), counts { print(key) }    // pair iteration
```

What the merge adds — bind side gains the arm grammar's powers:

```jet
// proposed: list rest patterns (today: E0315 unless lengths match exactly)
[first, ..rest] :: queue ?? return

// proposed: string patterns as binds — parse and bind in one line
"v{major:Int}.{minor:Int}" :: version ?? return usage()

// proposed: refutable binds generalize S74's promise
.Ok(config) :: load("app.jet") ?? panic(err.message)
```

And the arm side gains the bind grammar's powers:

```jet
// proposed: list patterns in arm heads
if packet == {
    []            -> idle()
    [only]        -> single(only)
    [head, ..rest] -> stream(head, rest)
}
```

The rule that makes refutable binds safe is already ratified: S74 says a refutable bind must carry a `??` route. The merge keeps that rule and extends it to every *shape* pattern — enum variants, strings, ranges, or-patterns. Two forms keep today's meaning on purpose: an irrefutable bind (`.{name, ..} :: user`) needs no route, and a fixed-length list or tuple bind stays a runtime-checked bind (E0315), so `[a, b] :: point` compiles unchanged — it gains a route only when you write one (`[a, b] :: point ?? return`). So the beginner's common case gains zero ceremony, and the compile error for a missing route names the fix. The `??` route here is the same operator as everywhere, so the ambient `err` name (D-FAIL-BIND1) reaches it — a scope extension named as an amendment in the PAT1 ballot.

The loop head's `(key, value)` special case dissolves. Under one grammar, `(key, value)` is simply a tuple pattern, and D-LOOP-COMMA1's "wrap the names in parentheses" respelling turns out to be the general rule wearing its special case. The surface does not change; the rule that explains it gets shorter. And the head now takes any pattern:

```jet
// proposed: destructure in the head — no body line, no second name
loop .{name, age}, users { greet(name, age) }
```

Expert exits for the one piece of magic here (the checker proving a bind irrefutable): see what it concluded with `$x`, the fact-ledger read ratified by D-FACT-READ1; spell it explicitly by writing the `??` route anyway, which is always legal; refuse nothing, because the proof adds no behavior — it only decides whether a route is required.

Marked: **amends S74 (extends the bind grammar), D-DESTRUCT1 (struct patterns shared), S19 (loop binding becomes a pattern position). Deletes `BindPattern` as a separate grammar. Surface of D-LOOP-COMMA1 unchanged.**

## Element 2 — the draw head (D-CHOOSE-DRAW1)

D-CONC-CHAN2 ratified wait arms of the shape `binding, source`. S19's loop head is the same shape. Name it once: a **draw head** — `pattern, source` — draws the next item from the source into the pattern. In a `loop` it repeats; in a wait table it fires once for whichever source is ready first. One shape, two cadences. Learning one teaches the other, which is exactly the argument D-CONC-CHAN2's ratification already made.

The open question the ballot settles: what does a *refutable* pattern in a loop's draw head mean?

```jet
// proposed: the pattern filters — a miss falls through to the next item
loop .Ok(reading), sensor_log { record(reading) }
```

The law answers it: a miss falls to the next head, and a loop's next head is the next item. So a refutable draw pattern skips items that do not fit — the same meaning the ratified guard already has. `loop .Ok(r), log` and `loop r, log if r.ok` are one thing, and the ballot's recommendation makes that the rule. The genuine alternative (a miss stops the loop, Rust's while-let) is on the ballot, because a reader from Rust will expect it and the owner should pick with both meanings on the table.

One wall, kept on purpose: **wait arms take no refutable patterns and no guards.** A wait table draws from whichever source is ready. A filter that runs *after* the draw would have already consumed the item; a miss would drop it silently. Erlang affords selective receive by re-queuing a mailbox; Jet channels are not mailboxes. Until someone designs re-queue semantics, the honest rule is: a wait arm's pattern is irrefutable, and filtering happens in the body where the item is visibly yours. This wall is written into the DRAW1 ballot so it is law, not folklore.

Marked: **amends S19 (draw-head semantics named; refutable pattern = filter). Confirms D-CONC-CHAN2 unchanged and adds the no-filter wall to it.**

## Element 3 — finding nothing is absence (D-CHOOSE-FIND1)

A search loop that finds nothing currently has no answer — the pattern every language struggles with (Python grew for-else for it). Jet does not need a construct, because Jet already has a word for "no value": absence, and absence already rides `??`.

Today, find-first is a method chain or a mutable flag:

```jet
// today
found := None
loop u, users {
    if u.id == target { found = Val(u) break }
}
result :: found ?? panic("no such user")
```

Proposed: a loop in value position whose exits are `break value` has type `T?`. Falling off the end — the loop ran out without breaking — is `None`. Then the failure rail does the rest, with every route it already has:

```jet
// proposed: find-first with a fallback — for-else with zero new words
hit :: loop u, users if u.id == target -> break u ?? default_user()

// proposed: or bail out entirely
admin :: loop u, users if u.role == .Admin -> break u ?? return Err("no admin")
```

Python's for-else is a theorem of this: the `else` block is just the `??` route, and unlike Python's, nobody has to remember which way around it fires. The collecting loop (`names :: loop u, users -> u.name`) is untouched — it already has an answer for "nothing matched": the empty list.

Beginner rung: nothing — statement loops and collecting loops are unchanged. Intermediate: `break u` in a value loop, `??` when you want a fallback. Expert: the full route family (`?? return`, `?? panic(...)`) and the loop label forms compose unchanged. No upper rung changes what the lower rungs do.

Marked: **new typing rule for value-position loops (amends S68's value-table law to cover `loop`, and narrows S23/E0075: a value-position loop whose exits are `break v` is a value loop, not a collecting loop, so the payload-break rejection does not apply to that one shape). The break family's spellings are unchanged.**

## Element 4 — multi-head functions are the table (D-CHOOSE-HEADS1)

S83 lets a function dispatch by argument shape across several heads. The isomorphic audit called it "two costumes for one elim" twice (2026-07-27 finding 8, carried 2026-07-28) and asked for an owner ballot that was never filed. This slate files it.

```jet
// today (S83) — a second dispatch mechanism with its own exhaustiveness rule
fn area(Circle(r: Float)) => Float = 3.14 * r * r
fn area(Rect(w: Float, h: Float)) => Float = w * h

// what it means — the table, written once
fn area(shape: Shape) => Float = if shape == {
    .Circle(r)  -> 3.14 * r * r
    .Rect(w, h) -> w * h
}
```

The ballot's options are honest and complete: define the multi-head form as sugar that desugars to the one table (keep the surface, delete the mechanism — the exhaustiveness proof becomes E0307, the same proof every table gets); keep it as a separate walled mechanism; or retire it and let people write the table. The recommendation is sugar: the surface reads well in API docs and costs nothing once its meaning is the table's meaning.

Marked: **amends S83 whichever way the owner picks.**

## The fact connection (no ballot needed)

A fit teaches. This is already law in three ratified pieces, and the proposal only asks that the docs say it once instead of three times: D-FLOWTYPE1 (a `!= None` test narrows `T?` to `T`), D-TYPE2's knowledge grades (ranges, units, exactness — knowledge grows silently, is never lost silently), and D-FACT-FLOW1 (one flow-fact store, join rules applied "at every if, loop, and arm table"). Under the law, these are one sentence: **the head that fit is a fact, and the checker's ledger holds it.** The expert reads the ledger with `$x` (D-FACT-READ1); the explicit spelling is a pattern bind; and there is nothing to refuse because facts only permit, never act.

## Notation notes (scoped)

The NOTATION audit owns glyph unification, including the `#` false rhyme the isomorphic audit carries; this proposal stays off it. What this area contributes to the lexical map: `match`, `switch`, `case`, and `when` are free identifiers forever — S68 means Jet never spends those words, and user code may use them as names. `select` stays a free identifier (D-CONC-CHAN2=D). `_` remains payload-slot-only; there is no bare `_` arm because `else` is the one out-word, and the `??` routes are that same word in operator position. `after` is contextual inside wait tables, not a keyword. This proposal reserves nothing new.

## The final vision

One real program, today and after. A worker that loads config, drains jobs, and reports the first bad record.

**Today:**

```jet
fn run() {
    cfg_result :: load("app.jet")
    cfg :: if cfg_result == {
        .Ok(c)  -> c
        .Err(e) -> panic(e.message)
    }

    parts :: version_parts(cfg.version)     // hand-written split + parse, ~8 lines elsewhere

    bad := None
    loop r, records(cfg) {
        if !r.valid {
            bad = Val(r)
            break
        }
    }
    if bad == {
        .Val(r) -> report(r)
        else    -> print("clean")
    }
}
```

**Proposed (each new line marked):**

```jet
fn run() {
    .Ok(cfg) :: load("app.jet") ?? panic(err.message)               // proposed: refutable bind, PAT1
    "v{major:Int}.{minor:Int}" :: cfg.version ?? return usage()     // proposed: string-pattern bind, PAT1

    loop .{id, valid}, records(cfg) if !valid -> break report(id)   // proposed: head destructure + find, PAT1+FIND1
        ?? print("clean")                                           // proposed: for-else as the rail, FIND1
}
```

The wait table (ratified today, shown here composing with the same heads):

```jet
taskgroup g {
    (s1, jobs)    :: tasks.channel<Int>()
    (s2, control) :: tasks.channel<Int>()
    loop {
        if {
            job, jobs    -> handle(job)     // ratified: D-CONC-CHAN2=D
            msg, control -> obey(msg)
            after 100ms  -> retry()
        }
    }
}
```

The structure of the end state — one engine, many doors:

```
                    the choice plane
                    ────────────────
 surface doors                          one machinery
 ┌────────────────────────────┐
 │ if subj == { pat -> body } │──┐
 │ if { cond -> body }        │──┤
 │ if { pat, src -> body }    │──┤     ┌───────────────────┐
 │ loop pat, src { body }     │──┼──►  │ one Pattern grammar│
 │ loop pat, src if g -> v    │──┤     │ one arm-table check│
 │ pat :: expr ?? route       │──┤     │ one exhaustiveness │
 │ fn f(pat) = body  (S83)    │──┘     │ proof (E0307)      │
 └────────────────────────────┘        │ one flow-fact store│
        else / ?? / proof              │ (D-FACT-FLOW1)     │
        = "the table ran out"          └───────────────────┘
```

Today that right-hand box is two pattern enums, two parsers, a special-cased pair binding, a builder chain, and a separate multi-head rule. After: one box.

## What this unlocks

- **Parsing-shaped work**: `"{name}: {count:Int}" :: line ?? next` turns a read-split-parse-validate block into one loop line. Log crunching, CSV-ish input, version strings, CLI args.
- **Protocol and systems code**: binary patterns already exist for arms; with PAT1 they bind too — `[U8].{"{ver:U4}{rest:...}"} :: frame ?? return Err("short frame")`.
- **Data pipelines**: head destructure plus guard plus collect — `loop .{user, amount}, ledger if amount > limit -> user` — reads as the sentence it is.
- **Concurrency**: the wait table and the drain loop are one shape, so the step from "loop one channel" to "wait on three" is learning zero new grammar.
- **Teaching**: one law to teach. "First fit wins; a fit binds; a miss falls through; running out follows the rail" covers if, loop, waits, binds, and failure handling in one breath.
- **Tooling and Canvas**: one pattern grammar means the visual editor's pattern-arm transactions (#375, #866, #877) target one node kind, not two.

## What stays

- **`if` as the only branching keyword (S68)** — validated by the frequency audit (Python `match`: zero uses in 20 mined projects). Stays.
- **`if subj == { }` dispatch marker** — the isomorphic audit flagged the `==` as a clarity risk and recommended teaching, not respelling. This proposal agrees: it stays, on merit — distributed compare (D-IFDIST1) genuinely reads as "if code is one of these".
- **The comma law (D-LOOP-COMMA1) and the comma-less guard (D-LOOP-GUARD1)** — both stand; the misparse fix is owned by #1416/#1419 and is a diagnostic, not a design change.
- **No bare `_` arm; `else` is the out-word** — kept; the wildcard lives in payload slots where it means "this slot, any value".
- **Exhaustiveness stays a proof, not a promise** — open scalars still need `else` (E0307); integer-range exhaustiveness proofs stay out.
- **The wait-arm wall** — no guards, no refutable patterns on wait arms, named in DRAW1, kept until someone designs re-queue semantics that do not drop drawn items.
- **Parameter destructuring stays declined** (D-PAT6) — HEADS1 decides multi-heads; it does not sneak destructuring into ordinary single-head parameters.

## Decisions for the owner

Each ballot stands alone; any subset can be adopted. Full ballots on the audit card.

| Ballot | Question | Recommendation |
|---|---|---|
| D-CHOOSE-PAT1 | One pattern grammar in every pattern position (merge `BindPattern` into `Pattern`; list rest; string/range/or patterns in binds; refutable binds require a `??` route per S74) | Adopt — one grammar, one engine |
| D-CHOOSE-DRAW1 | The draw head: refutable pattern in a loop head filters (miss = skip), and wait arms stay unfiltered (wall) | Adopt filter semantics |
| D-CHOOSE-FIND1 | Value-position loops with `break v` type as `T?` and ride `??` (find-first, for-else) | Adopt |
| D-CHOOSE-HEADS1 | Multi-head functions (S83): sugar for the one table / keep separate / retire | Sugar for the table |

## Implementation shape

**Phase A — re-found, no surface change.** Merge the two pattern enums into one grammar with position-legality checks; rename the internal `Stmt::Switch`/`KW_SWITCH` fossil so code readers stop inventing a `switch`; point the guard desugar and the (k,v) form at the shared grammar. All tests green, zero visible change. Close the written contradictions in the same pass: yielding-loops prose to comma law, spec.md's builder-chain select section marked superseded, the 33 frozen tests (#1650) re-pointed at ratified grammar.

**Phase B — land ratified-but-unbuilt on the new substrate.** S74's refutable bind (`Val(n) :: ... ?? return`) — already law, currently zero code; build it once on the merged grammar. D-CONC-CHAN2's wait table lands as a table over draw heads, sharing the loop's head parser. The four `jit_gaps` pattern-dispatch stems close here — value-position tables are I9 debt regardless of any ballot.

**Phase C — balloted surface work, one coherent migration each.** PAT1's new forms (list rest, string binds, head destructure) with examples and goldens per I5; FIND1's value-loop typing; HEADS1's chosen outcome; DRAW1's filter semantics with its teaching lint. Each deletes what it replaces; nothing keeps a legacy spelling.

**After ratification**: reconcile the e3 board — the outcomes here touch cards #1416, #1419, #1420, #1453, #1560, #1650 and the D-CONC/D-FAIL implementation slates; add, update, or retire cards so one unified plan remains. That reconciliation is an exit criterion on the audit card, gated on the owner's picks.
