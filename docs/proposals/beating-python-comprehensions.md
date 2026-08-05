# Beating Python comprehensions

Measured base: docs/audits/surface-frequency-audit-2026-08-04.md. Compiler
checks in this proposal ran against today's `target/debug/jet`.

## Words used here

- **Comprehension** — one expression that walks a source, keeps some items, and
  yields a new list. Python: `[u.name for u in users if u.active]`.
- **Header clause** — one comma-separated part of a `loop` head: binding,
  source, stride.
- **Guard** — the `if` test in the head that keeps or skips an item.
- **Subject** — the current item a bare `.member` reads from.
- **Token cost** — lexical token count of the same finished task, one counter
  for every language.

## The audit number was wrong about today's Jet

The audit scored Jet's comprehension at 26 tokens against Python's 18. That
snippet did not use the header guard. The guard form ships today and runs:

```jet
names :: loop u, users if u.active -> u.name        // 14 tokens — compiles and runs today
```

The same task, counted with the same counter:

| Form | Tokens |
| --- | --- |
| **Jet today, guard form** | **14** |
| Python `[u.name for u in users if u.active]` | 15 |
| Jet method chain (audit form) | 33 |
| TypeScript `filter(u => …).map(u => …)` | 22 |
| Rust `iter().filter(…).map(…).collect()` | ~40 |

**Jet already beats Python.** Nobody can see it. Two defects hide the win:

1. **The natural spelling misparses.** D-LOOP-COMMA1 says commas separate
   header clauses, so users will write `loop u, users, if u.active -> …`. The
   parser eats `, if …` as a *stride* expression, then fails with an unrelated
   value-`if` error (E0003 "both branches must produce one"). Only the
   undocumented comma-less `users if u.active` parses. One clause disobeys the
   comma law, and the failure story lies.
2. **The form is invisible.** Zero examples use a guard. Two files use the
   yield arrow at all. spec.md says "a header guard filters items" and never
   shows the spelling. The audit team itself could not find it — that is the
   proof of invisibility.

## Options

Every option keeps D-ARROW-CONTROL1: `->` on arms, `=>` on callables. Every
option keeps the method-chain plane and `.lazy()` untouched.

### A — Document what ships (zero code change)

Keep `loop u, users if u.active -> u.name` as canonical. Add examples, spec
spelling, and a real diagnostic for the comma spelling. 14 tokens; beats
Python by 1.

- Cost: the guard permanently disobeys the comma law of D-LOOP-COMMA1. The
  head reads as two laws: commas everywhere, except before `if`.

### B — Make the guard obey the comma law

`loop u, users, if u.active -> u.name` becomes the canonical spelling; the
parser treats `, if` as a guard clause, never a stride; fmt rewrites the
comma-less form. 15 tokens; ties Python.

- Cost: a stride expression may no longer start with a value-`if` without
  parentheses. Real strides are numbers; the loss is theoretical.

```jet
evens :: loop n, nums, if n % 2 == 0 -> n * n
```

### C — Implicit subject in bindingless loops (B plus one rule)

When the head names no binding, the item becomes the **subject**, and bare
`.member` reads from it — the same desugar dispatch arms already ratified in
D-IFDIST1 (`if user == { .active -> … }`). Not a lambda; the body stays an
inline arm.

```jet
names :: loop users, if .active -> .name             // 11 tokens
squares :: loop nums -> .value * .value
totals :: loop orders, if .paid -> .total ?? 0
```

11 tokens against Python's 15 on the guarded task; 7 against 11 on the plain
map. **Beats Python by 4 on the most common transform in the corpus.**

Sharp edges to pin before ratify:

- Nested loops: an inner bindingless loop would shadow the subject. Rule:
  nesting requires named bindings; the compiler teaches this with a diagnostic.
- Precedence: bare `.member` resolves against the loop subject before the
  expected-type static shorthand (D-SHAPE3a). One ordered rule, stated once.
- `(key, value)` sources always name their bindings; no subject there.

### D — One contextual-subject rule everywhere (C plus lambdas)

Extend the same rule to argument position whose expected type is a
one-parameter callable: `.member…` chain is short for that callable.

```jet
names :: users.filter(.active).map(.name)            // 15 tokens, was 33
oldest :: users.max_by(.age)
```

The chain plane drops from 33 to 15 — cheaper than TypeScript's 22 and level
with Python, while staying eager and typed. Swift ships this as `\.keyPath`;
Kotlin as `it`; Jet's spelling reuses the bare-member atom it already has.
One rule, three homes: dispatch arms (ratified), loop heads, callable
arguments. That is a generalization of an existing mechanism, not a new one
(I8).

- Sharp edge: the rule fires only when the expected type is a one-parameter
  callable; two-parameter callables still need a written lambda. No `$0`/`$1`
  family enters the language.

## Scoreboard if D lands

| Task | Python | TypeScript | Rust | Jet today | Jet after D |
| --- | --- | --- | --- | --- | --- |
| map | 11 | 18 | ~30 | 10 | **7** |
| filter + map | 15 | 22 | ~40 | 14 | **11** |
| chain spelling | — | 22 | ~40 | 33 | **15** |

Jet becomes the cheapest sound language on the corpus's most common transform,
in both of its spellings, without touching arrow law, eagerness, or types.

## Passes

- **Beginner:** `loop users, if .active -> .name` reads as the sentence it is.
  No lambda concept, no capture rules, `?`/`next`/`break` all work. The comma
  law holds everywhere.
- **Expert:** named bindings remain for nesting and clarity; the chain plane
  and `.lazy()` remain; nothing is removed.

## Recommendation

Ratify **B + C + D as one ruling** (the contextual-subject rule plus the comma
guard), and land A's example/doc coverage with it. If the lambda half needs
more scrutiny, C stands alone; D can follow as its own ballot.

Independent of any pick: the `, if` stride misparse is a bug against
D-LOOP-COMMA1's stated law and deserves a card now.
