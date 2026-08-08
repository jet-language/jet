# Choosing: one table, one pattern grammar

Proposal, 2026-08-07, revision 3. Rev 2 swung too far toward compression: pattern-left binds read backwards, the loop-head forms hid their meaning, and several spellings collided with planned law. This revision was rebuilt from an adversarial syntax audit (inference load, garden-path parses, left-to-right verbalization, parser collisions) and a full cross-reference against ratified-but-unbuilt and planned cards. Every code block is labeled: **A — today** compiles now or is ratified law, **B — proposed** needs a ballot. Withdrawn rev-2 forms are shown struck through with the reason, so nothing disappears silently.

## Executive summary

The ideas stand; the spellings changed. The ceremony-shortening for breaking down a result or optional stays, but every proposed form now reads subject-first, left to right, with the failure word written on the line. The adversarial audit's core finding: rev 2's forms all moved the pattern in front of the subject and deleted the word for a miss — every parser collision and misreading followed from that one move. The fix is one already-parsing shape: `subject == pattern ?? route`.

Three short laws now carry the whole design, and each one is an indication rule, not an inference rule:

1. **`::` always succeeds.** If a line binds with `::`, it cannot fail. No exceptions, so no guessing.
2. **Testing uses `==`; the miss uses `??`.** A line that can fail says so with `==` (the test) and answers "and if not?" with a written route. The bound names exist afterward only because the route left.
3. **Proposed on your suggestion: `::` defines, `=` fills.** Aliases (D-ALIAS-OP1=B, card #1513 — this is the alias ruling, not D-NAME-ALIAS1) and module instantiation (D-CONF-GENSPELL1) already moved definitions to `::`. A single-line function body is a definition too: `fn area(r: Float) => Float :: 3.14 * r * r`. What stays `=`: filling a slot — reassignment, parameter and field defaults, enum discriminants.

The slate is now six ballots: PAT1 (one pattern grammar in every *test* position — binds unchanged), TEST1 (the statement test-bind law above), FIND1 (search loops, revised: the route is mandatory, nothing inferred), HEADS1 (rescoped: one coverage proof now, surface reconciliation named), FNBODY1 (your `::` function-body suggestion), and the withdrawn DRAW1 is deleted — its question dissolved when head patterns died.

## How to read the code

| Sigil | Says | Example |
|---|---|---|
| `::` | bind — always succeeds | `x :: 5` |
| `:=` | same bind, mutable | `i := 0` |
| `==` | test — this line can miss | `x == .Ok(n)` |
| `??` | the route on a miss: a default, `return`, `break`, `next`, `panic(...)` | `?? return` |
| `->` | arm arrow: when this head fits, do this | `200 -> ok()` |
| `=>` | function arrow (declares the return type) | `fn f() => Int` |
| `.{ }` / `.Name` / `[ ]` / `( )` | shapes: struct, enum case, list, tuple | `.Ok(n)`, `[a, b]` |

## The catalog — every case, before and after

### 1. Branch on a value — unchanged

```jet
grade :: if score == {
    90..100 -> "A"
    80..89  -> "B"
    else    -> "C"
}
```

### 2. Handle a result

**A — today.** The arm table handles both sides; it stays canonical. And a value fallback already has its form — this line ships in the examples today:

```jet
if parse_age(input) == {
    .Ok(n)  -> use(n)
    .Err(e) -> print(e.message)
}

n :: Int.parse(line) ?? next     // value fallback: works today
```

**B — proposed (TEST1).** When the miss should exit, test subject-first and route the miss. Spoken left to right: "if parse_age of input is an Ok of n — otherwise return."

```jet
parse_age(input) == .Ok(n) ?? return
use(n)
```

The law that makes this safe is written, not inferred: the route must leave (`return`, `break`, `next`, `panic`) — a value route is illegal here, because `n` would be unproven. `n` exists after the line only because the miss left. A pattern test with no `??` route binds nothing.

~~Rev 2: `.Ok(n) :: parse_age(input) ?? return`~~ — withdrawn: pattern before subject reads backwards, the failure was invisible, and the leading `.Ok(` collides with the scope-member statement grammar.

### 3. Destructure a struct — unchanged

The bind carries the type head; this is the shipped form (rev 2 mislabeled the headless spelling as law):

```jet
Incident.{id, severity: sev, ..} :: incident
```

### 4. List shapes in tests

**A — today.** Lists destructure only beside `::`, exact length only:

```jet
[a, b] :: point
```

**B — proposed (PAT1).** List and tuple shapes join the test grammar — arm heads and `==` tests — with a rest spelled `...rest`. Three dots is the ratified capture-and-name convention (`{rest:...}` in string patterns, `...xs` spread); two dots stays discard-only (`..` in struct binds). Rev 2's `..rest` broke that split and is withdrawn.

```jet
if queue == {
    []              -> idle()
    [only]          -> single(only)
    [head, ...rest] -> stream(head, rest)
}

queue == [head, ...rest] ?? return
```

The bind side does not change: `[a, b] :: point` keeps its exact-length runtime check, and no new bind forms are added anywhere in this proposal.

### 5. Parse a string

**A — today.** String patterns exist, but only as arm heads, so one field costs three lines:

```jet
if version == {
    "v{major:Int}.{minor:Int}" -> use(major, minor)
    else                       -> return usage()
}
```

**B — proposed (TEST1).** Subject first, same pattern, route on the miss. Spoken: "if version looks like v-major-dot-minor — otherwise return usage."

```jet
version == "v{major:Int}.{minor:Int}" ?? return usage()
use(major, minor)
```

Because the reader meets `==` before the quote, the holes are announced as bindings before they appear — the interpolation confusion of rev 2's string-on-the-left form (~~`"v{major:Int}..." :: version`~~) cannot start.

### 6. Iterate — unchanged

```jet
loop { poll() }                          // forever
loop fuel > 0 { fuel -= 1 }              // while a condition holds
loop i, 1..5 { print(i) }                // over a source
loop (key, count), counts { show(key) }  // pair iteration
loop i, 0..10, 2 { probe(i) }            // with a stride
```

### 7. Filter and collect — unchanged

```jet
names :: loop u, users if u.active -> u.name
```

### 8. Destructure while iterating — withdrawn, stays as it is

~~Rev 2: `loop .{name, age}, users { }`~~ — withdrawn on your call, and the audit agrees: the head falls into the condition-loop parse and garden-paths, and the pattern head added characters without adding clarity. The head stays a name or a `(key, value)` pair. Fields come off the binding, or off one explicit bind line when a body uses many:

```jet
loop u, users {
    greet(u.name, u.age)
}
```

### 9. Skip the items that do not fit

~~Rev 2: `loop .Ok(r), readings { }`~~ — withdrawn: silent data loss with no word on the line for it, exactly the objection that walls the wait table.

**A — today.** The arm table already says the skip out loud, and this compiles now:

```jet
loop r, readings {
    if r == {
        .Ok(reading) -> record(reading)
        else         -> next
    }
}
```

**B — proposed (TEST1, same law as case 2).** For a long happy path, the flat form: one line, subject first, skip written:

```jet
loop r, readings {
    r == .Ok(reading) ?? next
    record(reading)
}
```

This is not new loop syntax — it is case 2's statement inside a loop body, where `?? next` is already a ratified route. One mechanism, and the more verbose form is the clearer one, as requested.

### 10. Find the first — and what if there is none?

**A — today.** The ratified idiom is D-LOOPSTATE1's labeled value loop — explicit, and the rev-2 draft failed to cite it:

```jet
found :: loop {
    loop u, users {
        if u.role == .Admin break(found, Val(u))
    }
    break None
}
admin :: found ?? return Err("no admin")
```

**B — proposed (FIND1, revised).** A finite loop in value position with `break v` exits — and the route is *mandatory*, so nothing is inferred: the loop's own type is the break value's type, and the written route answers exhaustion. `?? next` and `?? break` are illegal immediately after the loop's `}` (they would read as controlling the loop that just closed); use the labeled form above for that.

```jet
admin :: loop u, users {
    if u.role == .Admin break u
} ?? return Err("no admin")
```

Python's for-else, with the else spelled by the same `??` as every other miss in the language.

### 11. Wait on channels — already ratified, shown for the picture

**B — ratified (D-CONC-CHAN2=D, not built, card #1560).** No change proposed here:

```jet
if {
    job, jobs    -> handle(job)
    msg, control -> obey(msg)
    after 100ms  -> retry()
}
```

The wall stands: wait arms take no patterns and no guards — a filter after a draw from many sources would drop the drawn item. Filtering a wait happens in the body (case 9's form), where the item is visibly yours.

### 12. Dispatch by argument shape — rescoped

**A — today (S83).**

```jet
fn area(Circle(r: Float)) => Float = 3.14 * r * r
fn area(Rect(w: Float, h: Float)) => Float = w * h
```

**B — proposed (HEADS1, rescoped).** The audit found the honest blocker: S83 heads are a second pattern dialect — bare names where every arm head requires `.Circle` (D-ENUMDOT1), typed sub-bindings that no arm head has. So the ballot now asks only for the unification that is safe today: coverage and overlap are checked by the table's proofs (E0307, unreachable-arm lint), one error copy instead of two. Whether the *surface* becomes literal table sugar is a named follow-up that must first reconcile the head dialect with D-ENUMDOT1 and D-PAT6.

### 13. Single-line function bodies — your suggestion, balloted

**A — today (D-ARROW-CONTROL1=A).**

```jet
fn area(r: Float) => Float = 3.14 * r * r
```

**B — proposed (FNBODY1).** A one-line body is a definition, and definitions are moving to `::`: aliases (`alias Parsed<T> :: T ? AppError`, D-ALIAS-OP1=B) and module instantiation (`module int_cache :: cache<Int>(64)`, D-CONF-GENSPELL1=A) already made the move. The law: **`::` defines, `=` fills.** Defaults (`timeout: Int = 30`), field defaults, enum discriminants, and reassignment fill a slot inside something being defined — they keep `=`.

```jet
fn area(r: Float) => Float :: 3.14 * r * r
```

Named honestly in the ballot: this amends D-ARROW-CONTROL1, and D-ALIAS-OP1's recorded rationale explicitly chose to leave the fn-body `=` alone when aliases moved — the ballot quotes that reasoning so the pick is made with it on the table.

## The one law behind the catalog

A choice is an ordered table of heads; the first head that fits binds its names and runs its body; a miss falls to the next head; a table that runs out follows the `??` rail. The arm table tries arms. The loop tries items. The wait tries sources. The statement test (case 2) is a one-head table whose else is its route. Exhaustiveness (E0307) is the proof a table cannot run out. And the three indication laws keep every form honest: `::` cannot fail, `==` can, `??` says what happens then.

## Cross-reference against planned law

The full sweep ran against every ratified-but-unbuilt decision and planning/ready/implement card. Verdicts:

| Proposal element | Planned law it touches | Verdict |
|---|---|---|
| TEST1 statement test-bind | S31 (pattern `==` binds in conditions) — scope extension to statements is new law, stated in the ballot; E0405 (`??` needs a fallible left side) gains one named exception: a pattern-test left side with a diverging route | Named amendment |
| TEST1 replaces the pattern-left refutable bind | S74's `Val(n) :: maybe_port() ?? return` — ratified, zero code (card #1652) | Amends S74: the unbuilt pattern-left form is retired before it is ever built; #1652 re-points to TEST1 |
| `...rest` in list patterns | Two-dot discard (S74 struct `..`), three-dot capture (`{rest:...}` D-PARSESTR1/D-BINPAT1, `...xs` D-VARIADIC1) | Meshes — rev 2's `..rest` collided and is withdrawn |
| FIND1 value loops | D-LOOPSTATE1 (labeled search loop ships today), S23/E0075 (payload breaks illegal in yielding loops), D-LOOP-STMT-ARROW1 (arrow statement loops), D-LOOP-SUBJECT1 (bindingless arrow loops) | Cited; FIND1 is brace-form only, arrow forms unchanged; the bindingless arrow form cannot host `break v` (E0075) and the ballot says so |
| FNBODY1 | D-ARROW-CONTROL1=A (fn `= expr`), D-ALIAS-OP1=B (aliases to `::`; its rationale kept fn `=`), D-CONF-GENSPELL1=A (modules to `::`), S61/D-FIELDDEF1/D-META-CONST1 (defaults and discriminants keep `=`) | Named amendment; the fill-vs-define split leaves every `=` fill-site untouched |
| Refinement types as patterns (`Int(0..100)` in a test) | D-TYPE2-SPELL1=A gives type-position refinements and `.from_int` conversion only | Deferred, named in What stays out — not folded in silently |
| Wait table | D-CONC-CHAN2=D, card #1560 | Unchanged, wall restated |

## Decisions for the owner

| Ballot | Decides | Case | Recommendation |
|---|---|---|---|
| D-CHOOSE-PAT1 | List and tuple shapes join arm heads and `==` tests with `...rest`; binds unchanged; loop heads unchanged | 4 | Adopt |
| D-CHOOSE-TEST1 | The statement test-bind: `subject == pattern ?? route`, diverging routes only, bindings survive because the route left; replaces S74's unbuilt pattern-left form | 2, 5, 9 | Adopt |
| D-CHOOSE-FIND1 | Finite value loop with `break v` and a mandatory `??` route; `?? next`/`?? break` illegal after the loop's `}` | 10 | Adopt |
| D-CHOOSE-HEADS1 | Multi-head coverage checked by the table's proofs now; surface desugar deferred to a named follow-up | 12 | Adopt the proof half |
| D-CHOOSE-FNBODY1 | Single-line fn bodies: `= expr` becomes `:: expr` under "`::` defines, `=` fills" | 13 | Owner-raised; recommended with the collision named |
| ~~D-CHOOSE-DRAW1~~ | Deleted — head patterns are withdrawn everywhere, so the question it asked no longer exists | 8, 9 | — |

## What stays, and what stays out

- **`if` as the only branching keyword; the arm table; the comma law; the comma-less guard; the wait table** — all stand untouched.
- **Binds gain nothing and lose nothing.** `x :: 5`, `Incident.{..} :: incident`, `[a, b] :: point`, `(x, y) :: point` are exactly today's law. `::` cannot fail — that is now a stated law, not an accident.
- **No head patterns in loops** — withdrawn, on your call and the parser's.
- **Refinement patterns (`Int(0..100)` as a test head) stay out** — D-TYPE2's `.from_int` remains the check-and-convert; folding refinements into the pattern grammar is future work, named here so it is not invented twice.
- **Binary patterns in TEST1 position** — `frame == [U8].{"{ver:U4}{rest:...}"} ?? return` follows from the one-engine law (D-BINPAT1) and is included in TEST1's technical text, not left to inference.
- **Parameter destructuring stays declined (D-PAT6).**

## Implementation shape

**Phase A — re-found, zero surface change.** One pattern engine behind arm heads and `==` tests; rename the internal `Stmt::Switch` fossil; stale-prose cards (#1654) and frozen tests (#1650) close in the same pass.

**Phase B — land ratified-but-unbuilt once.** The wait table (#1560) and the TEST1 statement (which supersedes #1652's S74 form if TEST1 ratifies) build on the shared engine. The four JIT pattern-dispatch gaps (#1653) close here.

**Phase C — the balloted surface, one clean migration each.** PAT1's list/tuple test shapes, TEST1 with its two written laws, FIND1's mandatory-route value loop, HEADS1's proof unification, FNBODY1's formatter-driven migration of every `= expr` body.

**After ratification:** criterion 5 on the audit card — record outcomes in spec, re-point #1652, reconcile #1416/#1419/#1420/#1453/#1560/#1650, mint implementation cards per outcome.
