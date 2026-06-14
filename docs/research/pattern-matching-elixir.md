# Pattern matching: the Elixir-style options brief

**Status:** unratified exploration — owner decision input. Nothing here is
implemented or promised. This is the briefing that *precedes* an Open
Decision row in docs/admin/02-syntax-decisions.md.

**Audience:** the owner, deciding which (if any) pattern-matching features
to add to Jet, and in what spelling. Written to be read cold — every
option has a concrete, in-context example, the languages that ship it, and
where the wider community lands on it.

---

## 0. Start here: what Jet *already* does

Before adding anything, know that Jet already has working pattern matching.
It just lives in two specific places and is **not** called `match` (Rust's
`match` is a deliberate teaching-error in S31).

```jet
// switch arms are full Bool conditions (S24), and `==` against a pattern
// destructures AND binds the payload (S31):
switch shape {
    shape == Circle(r)       -> { print("circle r={r}"); };
    shape == Rect(w, h)      -> { print("rect {w}x{h}"); };
}   // no `else` needed: sema checks the enum is fully covered

// the same `==` pattern test works in an `if`, binding into the body:
if user == value(name) {     // Option: T? / value(x) / null  (S32)
    print("hello {name}");
}

// Result-flavoured tests and fallbacks already exist (S35):
if parse(raw) == ok(n) { print("got {n}"); }
val port = parse(raw) or 8080;   // `or` fallback
val n = parse(raw)?;             // `?` propagation
```

So the question is **not** "should Jet have pattern matching" — it does.
The question is **which Elixir ergonomics to add on top**, where Jet has no
syntax today. Each section below is one such gap.

### The Elixir features, at a glance


| Feature                                   | Jet today               | This doc |
| ----------------------------------------- | ----------------------- | -------- |
| Destructuring **bind** (`{a,b} = t`)      | ❌ none                  | §1       |
| **List** patterns (`[h | t]`)             | ❌ none                  | §2       |
| **Guards** (a pattern *plus* a condition) | ⚠️ partial via S31 `&&` | §3       |
| **Nested** patterns (`ok(Rect(w,h))`)     | ⚠️ unclear              | §4       |
| **Tuples** + tuple patterns               | ❌ no tuple type at all  | §5       |
| Multi-clause **function heads**           | ❌ none                  | §6       |
| **Pin** operator (`^x`)                   | n/a by design           | §7       |


§8 is the cross-cutting safety decision (what happens when a bind *can*
fail). §9 is the recommendation.

---

## 1. Destructuring bindings — the big one

**What it is:** pull several values out of a structure in a single binding,
instead of one field at a time.

**Today in Jet** you must spell every field:

```jet
val x = p.x;
val y = p.y;
val w = rect.width;
val h = rect.height;
```

**With destructuring bindings:**

```jet
val Point { x, y } = p;            // struct: bind x and y at once
val Rect(w, h) = rect;             // single-payload enum / positional
```

### Real-world example: a 2D vector add

```jet
// Without (today):
fn add(a: Point, b: Point) -> Point {
    Point { x: a.x + b.x, y: a.y + b.y }
}

// With destructuring in parameters or body:
fn add(a: Point, b: Point) -> Point {
    val Point { x: ax, y: ay } = a;
    val Point { x: bx, y: by } = b;
    Point { x: ax + bx, y: ay + by }
}
```

The win grows with nesting and with "unpack a result then use three of its
fields" code — the daily texture of Elixir, Rust, and Swift programs.

### Who ships it

- **Elixir / Erlang:** the `=` "match operator" is the *whole language*.
`%{name: n, age: a} = person` is idiomatic everywhere.
- **Rust:** `let Point { x, y } = p;`, `let (a, b) = pair;` — extremely
common, considered a core ergonomic.
- **Swift:** `let (x, y) = point`, `case let .rect(w, h)`.
- **JavaScript / TypeScript:** `const { x, y } = p;`,
`const [a, b] = arr;` — destructuring is one of ES6's most-loved adds.
- **Python:** `a, b = pair`, structural pattern matching (3.10+ `match`).

### Community sentiment

Near-universally positive. The JS destructuring add is routinely cited as a
top quality-of-life ES6 feature. The standard *caution* is readability when
people rename-and-nest aggressively (`const { a: { b: { c } } } = x`) — a
style problem, not a feature problem. **No mainstream community regrets
adding it.**

### The catch (see §8)

Struct and single-variant binds **cannot fail** — they're irrefutable and
totally safe. But `val value(n) = someOption;` *can* fail (the option might
be `null`). That refutable case is the one real design decision, deferred
to §8.

### How it fits Jet

Very cleanly. It reuses the exact pattern grammar already in S31 (`Point { x, y }`, `Rect(w, h)`, `value(n)`, `ok(v)`), just in binding position
instead of `==` position. The spelling stays `val <pattern> = expr;` — no
new keyword, consistent with S2 (`val`/`var`).

---

## 2. List / sequence patterns

**What it is:** match a list by shape — first element(s) plus "the rest" —
the backbone of recursive list processing.

```jet
switch items {
    items == []             -> { print("empty"); };
    items == [only]         -> { print("one item: {only}"); };
    items == [first, ...rest] -> { print("head {first}, {rest.len()} more"); };
}
```

(`...rest` is a *placeholder spelling* — the spread sigil is itself an open
syntax question; alternatives include `[first | rest]` à la Elixir/Haskell,
or `[first, rest @ ..]` à la Rust.)

### Real-world example: sum a list recursively

```jet
fn sum(xs: List<Int>) -> Int {
    switch xs {
        xs == []            -> { 0 };
        xs == [head, ...tail] -> { head + sum(tail) };
    }
}
```

### Who ships it

- **Elixir / Erlang / Haskell / Prolog:** `[head | tail]` is *the* idiom;
recursion over lists is built on it.
- **Rust:** slice patterns `[first, rest @ ..]`, `[a, b, c]`.
- **Python:** `first, *rest = items` and `case [x, *xs]:`.
- **Scala:** `case head :: tail =>`.

### Community sentiment

Loved in FP communities — it *is* how you write list code there. In
imperative communities it's more niche: iterators / `for` loops cover most
needs, and naïve `[head | tail]` recursion can be a performance footgun
(non-tail-recursive, copies the tail) unless the compiler is careful.

### How it fits Jet — with a real tension

Jet's priority #3 is zero-cost performance and it has **no GC**. `[head, ...tail]` over a `List<T>` (a Rust `Vec`) means **copying the tail every
recursion** — O(n²) and surprising. Elixir gets away with this because its
lists are persistent cons-cells where `tail` is O(1) and shared; Jet's
`List<T>` is a flat growable array.

So list patterns are attractive *grammatically* but carry a semantic
mismatch with Jet's data model. Options if pursued: (a) allow only
fixed-length list patterns (`[a, b, c]`, no rest) — safe, no copy
surprise; (b) make `...rest` borrow a slice, not copy — but tier-1 Jet
forbids stored/returned references (docs/00 C1); (c) lint loudly (like the
existing L0501 slice-in-loop lint). **This needs care before it's a clear
win.**

---

## 3. Guards — a pattern *plus* a condition

**What it is:** match a shape *and* require an extra boolean — the classic
`when` clause.

```jet
switch shape {
    shape == Rect(w, h) when w == h -> { print("square {w}"); };
    shape == Rect(w, h)             -> { print("rect {w}x{h}"); };
    shape == Circle(r)              -> { print("circle"); };
}
```

### Where Jet already half-has this

S31 says `shape == Rect(w, h)` produces a `Bool`, and S24 arms are Bool
conditions joined with `&&`/`||`. So this *might already parse*:

```jet
shape == Rect(w, h) && w == h -> { ... };
```

The open question is purely **binding scope**: are `w` and `h` in scope on
the right of `&&`, and in the arm body, when the match is part of a larger
boolean? That's underspecified today. A `when` keyword would make the
"pattern, then guard" structure explicit and unambiguous; reusing `&&`
keeps the grammar smaller (no new keyword) at the cost of subtle scope
rules.

### Who ships it

- **Elixir:** `when` guards, with a restricted set of guard-safe functions.
- **Rust:** `Pattern if condition =>`.
- **Swift:** `case let .rect(w, h) where w == h:`.
- **Haskell:** guards with `|`.

### Community sentiment

Positive and uncontroversial *where the binding scope is clear*. Elixir's
one friction point — guards may only call a whitelisted set of "pure-ish"
functions — occasionally annoys people but is widely accepted as the price
of predictability. Jet wouldn't necessarily inherit that restriction.

### How it fits Jet

If we keep `&&` (no new keyword): smallest surface, but we must *ratify the
scope rule* (bound names flow rightward and into the body). If we add
`when`: clearer, but it's a new keyword competing with `&&` for the same
job — and S14 ("one obvious way") frowns on two spellings. **Lean: nail the
`&&` scope rule rather than add `when`.**

---

## 4. Nested patterns

**What it is:** patterns inside patterns, so one arm reaches several levels
deep.

```jet
switch result {
    result == ok(Rect(w, h))  -> { print("ok rect {w}x{h}"); };
    result == ok(Circle(r))   -> { print("ok circle {r}"); };
    result == err(e)          -> { print("failed: {e}"); };
}
```

### Real-world example: HTTP response handling

```jet
switch response {
    response == ok(Response { status, body }) when status == 200
        -> { print("body: {body}"); };
    response == ok(Response { status, body })
        -> { print("unexpected status {status}"); };
    response == err(e)
        -> { print("network error: {e}"); };
}
```

### Who ships it

Everyone with pattern matching: Rust, Swift, Elixir, Haskell, OCaml,
Scala, Python 3.10+. It's table stakes once you have patterns at all.

### Community sentiment

Strongly positive — it's the feature that makes pattern matching *pay off*
versus chained `if`s. The only caution is the same as §1: deeply nested
patterns can get hard to read, a style concern.

### How it fits Jet

This is arguably the most natural extension: S31 already defines the
pattern grammar; "patterns nest" just removes the (implicit) restriction
that a payload slot must be a bare name. Low conceptual cost, high payoff.
**Lean: yes, alongside §1.**

---

## 5. Tuples + tuple patterns

**What it is:** an anonymous fixed-size, mixed-type group — `(Int, String)`
— plus matching on it. Elixir's `{:ok, value}` is a tuple; Jet has no
tuple type at all today.

```jet
fn divmod(a: Int, b: Int) -> (Int, Int) {   // tuple return
    (a / b, a % b)
}
val (q, r) = divmod(17, 5);                   // tuple destructure
```

### Who ships it

- **Elixir / Erlang:** tuples + the `{:ok, _}` / `{:error, _}` convention
are the *primary* return idiom.
- **Rust / Swift / Python / Haskell / OCaml:** all have first-class
tuples.
- **Go:** multiple return values (not a true tuple type, but the same
ergonomic).

### Community sentiment

Mixed-to-positive, with a real divide:

- **Loved** for quick multi-returns (`divmod`, `(min, max)`).
- **Criticized** when overused: `t.1`, `t.2` positional access is
unreadable, and many style guides (notably in Rust and Swift) say *"past
two or three fields, use a struct with names."* Elixir's `{:ok, val}`
works precisely because it's tiny and conventional.

### How it fits Jet — biggest scope, weakest fit

This is the **largest** option by far: it's a whole new *type*, not just
new matching syntax. It touches the type system, codegen, printing (S55
auto-`Printable`), and would invite a second "bag of values" concept next
to structs. Jet already chose **named** everything — struct literals with
required field names (S29), named multi-field enum variants (S30). Tuples
cut against that grain. And Jet's `T ? E` (S34) already covers the
`{:ok,_}/{:error,_}` use case that drives most Elixir tuple usage — so the
single biggest motivation for tuples is *already handled differently*.

**Lean: defer or decline.** If multi-return is the real need, the cheaper,
more on-brand answer is "return a small named struct" or extend `T ? E`.
Worth a separate ballot only if concrete demand shows up.

---

## 6. Multi-clause function heads

**What it is:** define the same function several times, once per pattern;
the runtime picks the matching clause.

```elixir
# Elixir
def area({:circle, r}), do: 3.14 * r * r
def area({:rect, w, h}), do: w * h
```

### Who ships it

Elixir, Erlang, Haskell, Prolog, Scala (via `match`). **Not** Rust, Swift,
Go, Python, JS — they put the `match`/`switch` *inside* one function body.

### Community sentiment

Beloved in the Elixir/Haskell world — it's central to their style and reads
beautifully for recursive and protocol code. But it's also the feature
**most foreign** to imperative programmers: "where is the function
defined?" "what order do clauses run?" "why are there three `area`s?" It
trades a single source of truth per name for distributed definition.

### How it fits Jet — direct conflict

This collides with several ratified decisions and the constitution:

- **docs/00 priority #4 (smallness, "one obvious way")** and **S14 (no
aliases / one canonical form)** — multi-clause heads are a *second* way
to express what `switch` already does inside one body.
- **Beginner experience (priority #2):** "first compiled language" readers
expect one `fn name` = one definition.

The exact same logic is already expressible as one `fn` with a `switch`.
**Lean: decline for v1.** This is the one Elixir feature I'd actively
recommend against.

---

## 7. Pin operator (`^`)

**What it is:** in Elixir, a bare name in a pattern *binds* (rebinds, even).
To instead match *against the current value* of an existing variable, you
"pin" it: `^expected = actual`.

```elixir
expected = 200
^expected = response.status   # match only if status == 200
```

### Why Jet probably never needs it

The pin operator exists to resolve an ambiguity Elixir *created* by making
`=` both bind and match, and by letting patterns rebind existing names.
Jet doesn't have that ambiguity: S31's rule already says **"a bare name on
the right is a variable when one is in scope; an unqualified name otherwise
binds,"** and to test a unit value you qualify it (`Light.Red`). So Jet
resolves bind-vs-match by *scope and qualification*, not a sigil.

**Lean: not needed.** Mentioned only for completeness; no action.

---

## 8. The cross-cutting decision: refutable binds

This applies to §1 (and §2). A pattern in a `val` binding falls into two
classes:

- **Irrefutable** — *cannot* fail: structs (`Point { x, y }`),
single-variant enums. Always safe. Everyone agrees these are fine.
- **Refutable** — *can* fail: `value(n)` on a `T?` that might be `null`;
`Rect(w,h)` on an enum that has other variants. What happens on a
mismatch is the decision.

Three industry answers:


| Option                        | Spelling                                  | Languages                                                            | Tradeoff                                                                                                                               |
| ----------------------------- | ----------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| **A. Reject; teach `switch*`* | `val value(n) = opt;` → **compile error** | (Jet-specific)                                                       | Safest, most Jet-like. No hidden runtime failure. Costs one extra line (`switch`/`if`) when you genuinely don't handle the other case. |
| **B. Require `or` fallback**  | `val value(n) = opt or return;`           | (reuses Jet S35 `or`)                                                | Explicit failure path, reuses existing machinery, no new concept. Slightly more to type. Reads well.                                   |
| **C. Runtime panic (Elixir)** | `val value(n) = opt;` panics if `null`    | Elixir `MatchError`, Rust `let-else` is the *opposite*, Swift `try!` | Most familiar to Elixir users; but a *hidden* runtime panic directly fights Jet priority #2 and the "no surprise panics" stance.       |


### Community sentiment on this specific axis

- Elixir's refutable `=` (option C) is loved *in Elixir* because the whole
language is built around "let it crash" + supervision trees. Outside that
context, surprise `MatchError`s are a common beginner complaint.
- Rust deliberately went the **other** way: `let` patterns must be
irrefutable, and refutable ones force `if let` / `let ... else` (closest
to options A/B). This is widely regarded as the right call for a
systems/safety language — and Jet is closer to Rust's risk posture than
Elixir's.

**Recommendation: B as the primary, A as the fallback.** Allow refutable
binds *only* with an explicit `or` (option B) — it reuses S35, keeps the
failure visible, and matches the "errors are values, never surprises"
spirit. A bare refutable `val` with no `or` produces the teaching error
from option A pointing at `or` / `switch` / `if`.

---

## 9. Summary recommendation

If the goal is "Elixir's ergonomic feel, kept in Jet's safety and
smallness lane," the high-value / low-conflict bundle is:

1. **§1 Destructuring bindings** — yes. The signature win, reuses S31
  grammar, no new keyword.
2. **§4 Nested patterns** — yes, in the same change. Cheap, high payoff,
  makes §1 and `switch` far more useful.
3. **§3 Guards** — yes, but by **ratifying the `&&` binding-scope rule**,
  not by adding a `when` keyword (smallness, S14).
4. **§8 Refutable policy** — option **B** (`or` fallback), with option A's
  teaching error as the no-`or` path.

Hold or decline:

1. **§2 List patterns** — attractive but has a real performance mismatch
  with Jet's flat `List<T>` (tail-copy). Pursue only with a clear story
   (fixed-length only, or a careful slice design). Separate ballot.
2. **§5 Tuples** — large scope, cuts against Jet's named-everything grain,
  and its main motivator (`{:ok,_}`) is already covered by `T ? E`.
   Defer / decline.
3. **§6 Multi-clause function heads** — **decline for v1**; direct conflict
  with "one obvious way."
4. **§7 Pin operator** — not needed; Jet resolves bind-vs-match by scope.

### What happens next (per the syntax protocol)

Nothing is built until you ratify. When you've decided, I'll: add the
chosen items as a row in docs/admin/02-syntax-decisions.md Open Decisions
(or directly to Ratified with your option), then — and only then — update
`src/syntax.rs`, write the failing ui fixtures + examples first, and
implement parser → sema → codegen with snapshots, per the workflow loop.

### Open spellings still to pin (if we proceed)

- Destructuring in **parameters** too, or bindings only? (`fn f(Point { x, y }: Point)`)
- Guard keyword: confirmed `&&` reuse, or a `when` keyword after all?
- If list patterns: spread sigil — `[h, ...t]` vs Elixir `[h | t]` vs Rust
`[h, t @ ..]`.
- Refutable: confirm option B (`or`) is the canonical path.

