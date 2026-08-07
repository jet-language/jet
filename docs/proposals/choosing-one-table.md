# Choosing: one table, one pattern grammar

Proposal, 2026-08-07, revision 2 (example-led rewrite). First-principles audit of patterns, matching, and control flow. Owner choices are the four D-CHOOSE ballots on the audit card. Every code block is labeled: **A — today** compiles now (or is ratified law), **B — proposed** needs a ballot. Unlabeled code is unchanged law shown for the full picture.

## Executive summary

Jet already made the big call: no `match` keyword. One `if` builds every branch, one `loop` builds every repeat, and the fresh D-CONC-CHAN2 ruling put the channel wait on the same arm table. This audit does not reopen that. It found that the remaining pieces are fragmented: two pattern grammars that each know forms the other lacks, a loop head that takes only a name or one special pair, a search loop with no answer for "found nothing", and multi-head functions as a second dispatch mechanism nobody balloted.

The fix is one rule, applied everywhere: **a pattern is a shape that looks like the value, with names where data should land — and the same shapes work in every position.** Four ballots deliver it. Nothing a beginner writes today changes. Every case below is shown as an A/B pair so the win is visible on the page.

## How to read the code

Six sigils carry everything. One line each:

| Sigil | Says | Example |
|---|---|---|
| `::` | bind: the left side receives the right side's value | `x :: 5` |
| `:=` | same bind, mutable | `i := 0` |
| `->` | arm arrow: "when this head fits, do this" | `200 -> ok()` |
| `=>` | function arrow (declares the return type; never an arm) | `fn f() => Int` |
| `??` | "and if not?" — the route when there is no value | `?? return` |
| `.{ }` / `.Name` / `[ ]` / `( )` | shapes: a struct, an enum case, a list, a tuple | `.Ok(n)`, `[a, b]` |

Three reading rules, and every line in this document follows them:

1. **Left of `::` receives.** `x :: 5` and `.Ok(n) :: parse()` are the same sentence: a shape on the left, a value on the right, names get filled. A plain name is just the simplest shape.
2. **A table tries its heads top to bottom; the first fit wins.** `if` runs the table once. `loop` runs its one head once per item.
3. **`??` always answers "and if not?".** Its routes are a default value, `return`, `break`, `next`, or `panic(...)` — the same five everywhere it appears.

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

**A — today.** The arm table handles both sides. This stays and stays canonical:

```jet
if parse_age(input) == {
    .Ok(n)  -> use(n)
    .Err(e) -> print(e.message)
}
```

**B — proposed (PAT1).** When only the happy path matters, move the shape to the receiver seat and route the miss. This is S74's own promised form, finally general:

```jet
.Ok(n) :: parse_age(input) ?? return
use(n)
```

Read it with rule 1: `.Ok(n)` receives; if the value is not an `Ok`, the `??` route answers.

### 3. Destructure a struct — unchanged

```jet
.{id, severity: sev, ..} :: incident
```

### 4. Destructure a list

**A — today.** Exact length only; anything else is an error (E0315):

```jet
[a, b] :: point
```

**B — proposed (PAT1).** Rest patterns, and a route when the shape can miss. The exact-length form above stays exactly as it is — no new ceremony:

```jet
[head, ..rest] :: queue ?? return
```

### 5. Parse a string

**A — today.** String patterns exist, but only inside an arm table, so one field costs three lines:

```jet
if version == {
    "v{major:Int}.{minor:Int}" -> use(major, minor)
    else                       -> return usage()
}
```

**B — proposed (PAT1).** The identical pattern, moved to the receiver seat:

```jet
"v{major:Int}.{minor:Int}" :: version ?? return usage()
use(major, minor)
```

### 6. Iterate — unchanged

All loop forms as they are today, commas between clauses (D-LOOP-COMMA1):

```jet
loop { poll() }                          // forever
loop fuel > 0 { fuel -= 1 }              // while a condition holds
loop i, 1..5 { print(i) }                // over a source
loop (key, count), counts { show(key) }  // pair iteration
loop i, 0..10, 2 { probe(i) }            // with a stride
```

### 7. Filter and collect — unchanged

The ratified comprehension (D-LOOP-GUARD1); the guard follows the source with no comma:

```jet
names :: loop u, users if u.active -> u.name
```

### 8. Destructure in the loop head

**A — today.** The head takes a name; fields come off it in the body:

```jet
loop u, users {
    greet(u.name, u.age)
}
```

**B — proposed (PAT1).** Any shape works in the head, because the head is a pattern position now. `(key, count)` above was never special — it is just a tuple shape:

```jet
loop .{name, age}, users {
    greet(name, age)
}
```

### 9. Skip the items that do not fit

**A — today.** Test and skip in the body:

```jet
loop r, readings {
    if !r.ok next
    record(r)
}
```

**B — proposed (DRAW1).** Say the shape you want; a miss skips, exactly like the guard in case 7:

```jet
loop .Ok(r), readings {
    record(r)
}
```

One wall, on purpose: this works in a `loop`, never in a wait table (case 11). A wait draws from whichever channel is ready; a filter after the draw would drop the drawn item. Filtering a wait happens in the body, where the item is visibly yours.

### 10. Find the first — and what if there is none?

**A — today.** A mutable flag, a break, a second table:

```jet
found := None
loop u, users {
    if u.role == .Admin {
        found = Val(u)
        break
    }
}
admin :: found ?? return Err("no admin")
```

**B — proposed (FIND1).** A loop used as a value, whose exits are `break value`, is worth `T?`: the found value, or `None` when the loop runs out. Then `??` answers "and if not?" like everywhere else:

```jet
admin :: loop u, users {
    if u.role == .Admin break u
} ?? return Err("no admin")
```

Python needed a special for-else construct for this, with a firing rule people look up. Here it is the same `??` from cases 2, 4, and 5.

### 11. Wait on channels — already ratified, shown for the picture

**A — today (shipping).** A method chain on the group handle:

```jet
winner :: g.select().recv(jobs).recv(control).wait()
```

**B — ratified 2026-08-07 (D-CONC-CHAN2=D, not built yet).** The wait is an `if` table; each head is a binding and a source, the same head shape a drain loop uses:

```jet
if {
    job, jobs    -> handle(job)
    msg, control -> obey(msg)
    after 100ms  -> retry()
}
```

This proposal changes nothing here. It only names the wall (case 9) and reuses the head shape.

### 12. Dispatch by argument shape

**A — today (S83).** Multi-head functions are their own mechanism with their own coverage rule:

```jet
fn area(Circle(r: Float)) => Float = 3.14 * r * r
fn area(Rect(w: Float, h: Float)) => Float = w * h
```

**B — proposed (HEADS1).** The surface stays letter-for-letter. Its *meaning* becomes the table below, so coverage is E0307 — the same proof every table gets, one error copy instead of two:

```jet
fn area(shape: Shape) => Float = if shape == {
    .Circle(r)  -> 3.14 * r * r
    .Rect(w, h) -> w * h
}
```

## The one law behind the catalog

Every A and B above is one rule wearing different clothes: **a choice is an ordered table of heads; the first head that fits binds its names and runs its body; a miss falls to the next head; a table that runs out follows the `??` rail.** The arm table tries arms. The loop tries items. The wait tries sources. The bind is a one-head table whose `else` is spelled `??`. Exhaustiveness (E0307) is the proof that a table cannot run out, which is why exhaustive tables need no `else`. And a head that fits also teaches the checker its facts — that is D-FACT-FLOW1's ratified flow-fact store, already law.

The audit's evidence for why this pass is needed now, in one table:

| Fragment | Where | Defect |
|---|---|---|
| Two pattern grammars | `AST/patterns.rs:37` (arms) vs `:210` (binds) | arms lack lists/tuples; binds lack ranges/strings/or |
| S74 refutable bind | `syntax-decisions.md:523` | spec law, zero code (card #1652) |
| Loop head | `control.rs:1195` | takes a name or one special pair, never a shape |
| Found-nothing | — | no answer; flag pattern everywhere |
| Multi-heads (S83) | `syntax-decisions.md:262` | second dispatch mechanism; owner-gate carried twice, never filed |
| Stale prose | `yielding-loops.md`, `spec.md:2351` | teaches retired `;` headers and the dead select chain (card #1654) |
| JIT parity | `jit_gaps.txt:442,454-456` | value-position pattern dispatch missing on JIT (card #1653) |

## Decisions for the owner

Each ballot stands alone; any subset can be adopted. Full ballots on the audit card.

| Ballot | Decides | Catalog cases | Recommendation |
|---|---|---|---|
| D-CHOOSE-PAT1 | One pattern grammar in every position; refutable shape binds take a `??` route (S74's law made general); list/tuple exact-length binds keep today's meaning untouched | 2, 4, 5, 8 | Adopt |
| D-CHOOSE-DRAW1 | A refutable loop-head pattern skips the miss (same meaning as the guard); wait arms stay walled | 9 | Adopt skip |
| D-CHOOSE-FIND1 | A value loop with `break v` exits is worth `T?` and rides `??` (names its narrowing of S23/E0075) | 10 | Adopt |
| D-CHOOSE-HEADS1 | Multi-heads become sugar for the table; coverage is E0307 | 12 | Sugar |

Amendments named per ballot: PAT1 amends S74, D-DESTRUCT1, S19, and D-FAIL-BIND1 (ambient `err` reaches the bind-form route). DRAW1 amends S19. FIND1 amends the S68 value law and narrows S23/E0075. HEADS1 amends S83. Nothing else moves: the comma law, the comma-less guard, `if` as the only branching keyword, the wait table, and the `==` dispatch marker all stand.

## What stays, and why

- **`if` as the only branching keyword (S68)** — validated by the frequency audit; `match`, `switch`, `case`, `when` stay free identifiers forever.
- **`if subj == { }`** — the marker reads as "if subj is one of these"; the isomorphic audit said teach it, not respell it. Stays on merit.
- **No bare `_` arm** — `else` is the one out-word in tables, and `??` is the same word beside a bind. The wildcard lives in payload slots.
- **Exhaustiveness stays a proof** — open scalars still need `else`; no integer-range proofs.
- **The wait-arm wall** — no guards or refutable patterns after a draw from many sources; a miss there would drop data.
- **Parameter destructuring stays declined (D-PAT6)** — HEADS1 rules the multi-head form only; single-head parameters stay plain names.

## Implementation shape

**Phase A — re-found, zero surface change.** One pattern grammar behind both parsers; rename the internal `Stmt::Switch` fossil; all tests green. Close the stale prose in the same pass (card #1654) and re-point the 33 frozen tests (#1650).

**Phase B — land ratified-but-unbuilt once.** S74's refutable bind (card #1652) and D-CONC-CHAN2's wait table both build on the merged grammar. The four JIT pattern-dispatch gaps close here (card #1653) — that is I9 debt regardless of any ballot.

**Phase C — the balloted surface, one clean migration each.** PAT1's new forms with examples and goldens per I5, DRAW1's skip with its teaching lint, FIND1's typing rule, HEADS1's desugar. Each deletes what it replaces.

**After ratification:** reconcile the e3 board — record outcomes in spec, mint implementation cards per outcome, and update or retire #1416, #1419, #1420, #1453, #1560, #1650 so one plan remains. That is criterion 5 on the audit card.
