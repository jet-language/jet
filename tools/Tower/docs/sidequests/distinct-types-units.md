# Distinct types & units
**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c23

**Reopens D-SUGAR4** (newtype keyword declined 2026-06-16, "one-field struct
covers it") and **D-SUGAR3** (transparent type alias declined). This is a
justified reopen, not a re-litigation: the new evidence is the three deltas a
one-field struct can't deliver — guaranteed zero-cost `#[repr(transparent)]`
lowering, controlled same-type arithmetic, and primitive feel (no `.value`
noise). If those don't move the owner, this stays an I8 reject and D-SUGAR4
holds. Note also this is **not** the transparent alias D-SUGAR3 declined: an
alias is the *same* type (interchangeable with its base); a distinct type is a
*separate* type that shares only the representation.

## Problem & why it matters

A program that passes IDs, money, indices, and physical quantities as plain
`Int`/`Float` has no compiler help against mixing them. `charge(user_id,
product_id)` compiles even when the two arguments are swapped — both are `Int`.
The bug ships. This is the classic "primitive obsession" failure.

The obvious objection (and the first thing the simplicity ratchet, I8, asks):
**a `struct` already does this.** `struct UserId { value: Int }` is nominal —
sema rejects passing a `ProductId` where a `UserId` is expected. So why a new
feature?

The delta a distinct type buys over a one-field struct:

1. **Primitive feel, no `.value` noise.** With a struct you write
   `id.value == other.value`, `UserId { value: n }` to build, `id.value` to
   read everywhere. A distinct type is constructed `UserId(n)`, unwrapped with
   one named method, and (where allowed) compared directly — it reads like the
   `Int` it wraps, while staying a separate type.
2. **Controlled arithmetic.** A struct gives you no `+`. A distinct numeric type
   can opt into the base type's operators in a controlled way — `Meters + Meters`
   works, but `Meters + Seconds` and `Meters + Float` do not. You can't express
   "behaves like an Int for `==`/`<` but is its own type" with a plain struct
   without hand-writing every trait impl.
3. **Zero cost, guaranteed.** Codegen lowers a distinct type to the underlying
   Rust type behind `#[repr(transparent)]` — no wrapper struct in the binary,
   no field load. Priority #3 (runtime performance) is preserved by construction,
   and I3 holds: sema owns the type identity, codegen emits the bare underlying
   type.

If the owner concludes the struct already covers the need, this card is a
legitimate I8 reject — but the three points above are the case for shipping it.

Units of measure (a `Meters` that multiplies with `Seconds` to yield
`MetersPerSecond`) are a *strict superset* of nominal distinct types and are
treated as a separate, deferrable question (see D-DIST2).

## Prior art (terse)

- **Haskell `newtype`** — zero-cost single-constructor wrapper; no operators
  inherited unless you derive them. Nominal, explicit unwrap via the field
  accessor. The model Jet is closest to.
- **F# units of measure** — `[<Measure>]` annotations on `float`; full
  dimensional algebra (`m/s`, `m*s`), checked at compile time, erased at
  runtime. The full-units end state, and the reason units are heavier.
- **Odin `distinct`** — `UserId :: distinct int`. A distinct type shares the
  base's representation and operations but is a separate type for assignment and
  passing. Spelling lines up with Jet's ratified `::` binding sigil (D-BIND1).
- **Rust `#[repr(transparent)]` newtype** — the codegen target. A struct with
  one field, same ABI as the field.
- **Go named types** (`type UserId int`) — distinct for assignment, but Go's
  implicit conversions in literals leak; Jet wants stricter (no implicit either
  direction).

## Proposed design (worked Jet example: a UserId mixup caught at compile time)

Spelling below uses the **recommended** form from D-DIST1
(`UserId :: distinct Int`), pending owner ratification — the body commits
nothing; the cards own the choice.

```jet
UserId    :: distinct Int
ProductId :: distinct Int

fn charge(user: UserId, product: ProductId) -> Int {
    // ... bill the user for the product ...
    user.raw() + product.raw()
}

fn main() {
    u :: UserId(42)
    p :: ProductId(7)

    print("{charge(u, p)}")    // ok
    print("{charge(p, u)}")    // arguments swapped — caught below
}
```

The swapped call does not compile:

```
Error [E0125]: a `ProductId` can't be used where a `UserId` is expected
  --> mixup.jet:13:18
     |
  13 |     print("{charge(p, u)}")
     |                    ^
 Why: `UserId` and `ProductId` are distinct types — each is its own type even though both are built on `Int`, so one is never accepted in place of the other
 Fix: pass a `UserId` here, or convert with `UserId(p.raw())` if the swap was intentional
```

Construction is explicit (`UserId(42)`); there is no implicit `Int` → `UserId`.
Unwrapping is explicit too — one named method (`raw()` shown; spelling is
D-DIST3) — so a distinct value never silently decays back to its base. A raw
`Int` literal where a `UserId` is expected is the same E0125 with a "wrap it:
`UserId(...)`" fix.

**Derives.** A distinct type over a base whose underlying type is `Equatable`
auto-gets `Equatable` (S55), so two `UserId`s compare with `==` directly — no
`.raw()` needed and no cross-type mixing (a `UserId == ProductId` is E0125).
`Comparable`/`Serialize` follow S55's explicit opt-in (`#Comparable` on the
line above the declaration — D-ATTR1 marker sigil) when the base supports them.

**Methods.** S27 attaches: `impl UserId { fn is_admin(self) -> Bool { ... } }`
works, with `self` carrying the distinct type. Distinct types are ordinary type
names everywhere a name is allowed — `UserId?`, `[UserId]`, `Map<UserId, V>`,
function returns.

**Numeric distinct + arithmetic (units preview).** Inheriting the base's
arithmetic is **opt-in**, not automatic — exactly the S55 culture (auto
`Equatable`/`Printable`, explicit opt-in for the rest). A distinct numeric type
marked `#Numeric` inherits its base's arithmetic *within the same distinct
type*; an unmarked distinct type (a `UserId`) gets `==` only and no arithmetic.
This is what keeps `UserId` and `Meters` — both distinct-over-numeric — on
opposite sides of the arithmetic rule (see D-DIST3):

```jet
#Numeric
Meters :: distinct Float

a :: Meters(3.0)
b :: Meters(4.0)
total :: a + b           // Meters(7.0) — same distinct type, allowed
bad   :: a + 4.0         // E0125: `Float` isn't `Meters`
```

```jet
UserId :: distinct Int   // not #Numeric

bad :: UserId(1) + UserId(2)   // E0127: a `UserId` is an id, not a number
```

This is as far as v1 goes (nominal wrappers with opt-in same-type arithmetic).
True
units — `Meters / Seconds -> MetersPerSecond` — are deferred (D-DIST2).

## Implementation sketch — pipeline touchpoints

**Syntax.rs (I7).** Add the `distinct` keyword constant with its decision ID.
If the keyword-first family wins D-DIST1 (`distinct UserId = Int`), it's a new
item keyword like `struct`/`enum`; if the binding-form family wins
(`UserId :: distinct Int`), `distinct` is a contextual marker in the value
position of a `::` type-level binding.

**Parser.** New item: a distinct-type declaration capturing `name`, the base
type, and `pub` (S18). Base type is any existing type name (`Int`, `Float`,
`String`, `U8`, … — a sized menu member per S42, or another type). Reject a
distinct over a distinct in v1 unless the owner wants chaining (open in D-DIST1
notes). Emit a teaching error for `type X = Y` (a plain alias) and
`struct UserId(Int)` positional form (already E0048/E0049 territory, S73) so
neither is silently accepted.

**Sema — type identity & coercion (the core).**
- Register each distinct type as its own `TypeId`, recording its base. Two
  distinct types with the same base are unequal; a distinct type and its base
  are unequal.
- `UserId(expr)` is a constructor: `expr` must be the base type; result is the
  distinct type. No implicit base → distinct anywhere (E0125).
- The unwrap method (`raw()` or D-DIST3 spelling) is the only base ← distinct
  path; no implicit decay.
- Operator rules (D-DIST3): arithmetic is opt-in via the `#Numeric` marker. For
  a `#Numeric` distinct type, allow base operators when **both** operands are the
  *same* distinct type, producing that distinct type; reject distinct-vs-base and
  distinct-vs-other-distinct (E0125). An unmarked distinct type (the ID case)
  inherits only `==` via derived `Equatable` (S55) and nothing arithmetic —
  arithmetic on it is E0127. The marker is what discriminates `Meters` (a
  quantity) from `UserId` (an id), since both are distinct-over-numeric.
- Derive resolution (S55): a distinct type forwards auto-derive eligibility
  (`Equatable`, `Printable`) and explicit opt-in (`#Comparable`/`#Serialize`,
  `#Numeric`) to its base.
- Methods (S27) and `T?`/collections compose with no special cases — a distinct
  type is just a nominal type to the rest of sema.

**Codegen (dumb, I3).** Emit
`#[repr(transparent)] struct UserId(i64);` (or the base's Rust type). The
constructor lowers to the tuple-struct call, `raw()` to field `.0`, operators to
the inner op then re-wrap. No checking in codegen — sema has already proven
every use is well-typed; rustc only verifies (I2). The transparent repr makes it
zero-cost (priority #3).

**Diagnostics (I4).** New sema code **E0125** (the next free core code — E0124
is the last sema code in the registry; E0125–E0127 are all unused, verified).
**E0126** for "a distinct type's base must be a concrete value type" (e.g.
distinct over a trait or over `()` rejected) and **E0127** for "operator not
available on this distinct type — it's an id, not a number; mark it `#Numeric`
or use `.raw()`" with a workaround. Each new code needs a registry row in
diagnostics.md and a `tests/ui/` snapshot, or it doesn't exist.

## Test plan — ui snapshots + example

UI snapshots (`tests/ui/`, blessed with `UPDATE_EXPECT=1`):

- `distinct_mixup.jet` — swapped `UserId`/`ProductId` args → E0125 (the worked
  example above, exact render pinned).
- `distinct_raw_int.jet` — a bare `Int` literal where a distinct is expected →
  E0125 with the "wrap it" fix.
- `distinct_unwrap_required.jet` — using a `UserId` where a base `Int` is
  expected without `.raw()` → E0125.
- `distinct_arith_same.jet` — `#Numeric Meters + Meters` compiles;
  `Meters + Float` → E0125. (Skip/adjust if D-DIST3 lands no-arithmetic.)
- `distinct_arith_on_id.jet` — `UserId + UserId` (no `#Numeric`) → E0127 (ids
  aren't numbers).
- `distinct_bad_base.jet` — `distinct` over a trait/`()` → E0126.
- `distinct_eq.jet` — `UserId == UserId` compiles via derived `Equatable`;
  `UserId == ProductId` → E0125.

Executable example (I5), golden-tested:

- `examples/features/NN_distinct_types.jet` + `.out` — declares `UserId`/
  `ProductId`/`Meters`, constructs, compares, unwraps, does same-type
  arithmetic, prints results. Demonstrates the happy path end to end.

Codegen verification: a runtime test that a distinct value round-trips
(`UserId(42).raw() == 42`) and that `#[repr(transparent)]` holds (size equals
the base) — the differential check that codegen stayed dumb and zero-cost.

## Risks & invariant check

- **I8 (simplicity ratchet) — the main risk.** Must clear the "why not a
  struct?" bar (see Problem). If the owner judges struct-nominal-typing enough,
  reject the card. The arithmetic-passthrough and primitive-feel deltas are the
  case for it.
- **I1 (safe by default):** distinct types are pure surface-level type identity;
  no `unsafe`, no expert gate. Safe.
- **I2 / I3:** codegen emits a transparent wrapper and bare ops; sema owns all
  checking. rustc only verifies. Holds.
- **I4:** every new code (E0125–E0127) gets a registry row + ui snapshot.
- **I7:** `distinct` keyword/marker recorded in Syntax.rs with the D-DIST1 id.
- **Scope creep into units:** keep units out (D-DIST2) or the card balloons into
  dimensional algebra. "Measure twice, cut once" — ship nominal wrappers first.
- **Interaction with `T ? E` / `Fallible`:** a distinct type as an error type is
  fine (it's a nominal type); no special handling.

## Open decisions

1. Declaration spelling — keyword-first item vs. `::`-binding form (D-DIST1).
2. Units of measure now or deferred (D-DIST2). Recommend defer.
3. Coercion + unwrap + arithmetic rules, incl. the `#Numeric` arithmetic-opt-in
   marker and the unwrap spelling (D-DIST3).
4. (Notes only, fold into D-DIST1) distinct-over-distinct chaining: rejected in
   v1 unless the owner wants it.

## Proposed decision card(s)

### D-DIST1 — Declaration spelling for distinct types (rec C)

Two families. **Keyword-first** matches existing type declarations (`struct`,
`enum` are keyword-led items). **Binding-form** matches Odin's exact `distinct`
spelling and reuses Jet's ratified `::` immutable-binding sigil (D-BIND1). Both
introduce a new word `distinct`; the question is where it sits and what
separator joins the name to the base. `=` is free in type-declaration position
(it's reassignment only in expression position). `struct UserId(Int)` is **not**
an option — positional tuple structs and `.0` access are rejected (S73, E0048/
E0049).

- **Option A — `distinct UserId = Int` (keyword-first, recommended-adjacent).**
  Reads like a type declaration; `distinct` is an item keyword beside `struct`/
  `enum`. `=` joins name to base (only ambiguous in expression position, not
  here).

    ```jet
    distinct UserId = Int
    distinct Meters = Float
    ```

- **Option B — `distinct type UserId = Int` (keyword + `type`).** Closest to
  Go/Rust alias spelling, but adds a second word `type` that exists nowhere else
  in Jet today (no plain `type` alias is ratified). Heavier.

    ```jet
    distinct type UserId = Int
    ```

- **Option C — `UserId :: distinct Int` (binding form, recommended).** Reuses
  the ratified `::` immutable binding (D-BIND1): a type-level constant whose
  value is "a distinct version of `Int`." Exactly Odin's word in Jet's sigil.
  Reads "UserId is a distinct Int." Strongest consistency with the binding
  culture; no new separator token. The `distinct` keyword is load-bearing —
  `UserId :: Int` (no keyword) would be a transparent alias, which D-SUGAR3
  declined; the keyword is what makes this a *separate* type, not the rejected
  alias.

    ```jet
    UserId    :: distinct Int
    Meters    :: distinct Float
    ProductId :: distinct Int
    ```

- **Option D — `UserId := distinct Int` (mutable-binding form).** Rejected on
  sight — `:=` is the *mutable* binding sigil; a type is never reassigned.
  Listed only to close it.

    ```jet
    UserId := distinct Int   // wrong sigil; types aren't mutable
    ```

**Recommendation:** **Option C** — `UserId :: distinct Int`. It is Odin's spelling,
it reuses the already-spent `::` immutable sigil with no new token, and it reads
as plain English. Option A is the runner-up if the owner prefers type
declarations to stay keyword-first beside `struct`/`enum`. (Note: whichever
wins, distinct-over-distinct chaining is rejected in v1 — base must be a
built-in or struct/enum type.)

### D-DIST2 — Units of measure: in scope now, or deferred (rec defer)

Nominal distinct types (a `Meters` that won't mix with `Seconds`) are the small,
contained feature. *Units of measure* add **dimensional algebra**: multiplying
and dividing distinct numeric types yields *derived* units, and the compiler
tracks the dimension through expressions.

- **Option A — distinct types only now; units deferred (recommended).**
  `Meters + Meters` works (opt-in same-type arithmetic via `#Numeric`, D-DIST3);
  `Meters * Seconds` is E0127 ("can't multiply two different distinct types").
  No derived units. Small, shippable, doesn't foreclose units later.

    ```jet
    #Numeric
    Meters  :: distinct Float
    #Numeric
    Seconds :: distinct Float

    d :: Meters(100.0)
    t :: Seconds(9.58)
    speed :: d / t          // E0127: dividing two different distinct types isn't defined
                            //  (units of measure are a future feature)
    ```

- **Option B — full units of measure now.** `Meters / Seconds` yields a derived
  `Float<m/s>`; the compiler does the dimensional bookkeeping. Powerful, but it
  is a whole type-algebra subsystem (derived-unit synthesis, normalization,
  display) — far larger than nominal wrappers, and it pulls forward design the
  v1 type system isn't sized for.

    ```jet
    Meters  :: unit Float
    Seconds :: unit Float

    d :: Meters(100.0)
    t :: Seconds(9.58)
    speed :: d / t          // type: MetersPerSecond, derived automatically
    print("{speed.raw()} m/s")
    ```

**Recommendation:** **Option A — defer units.** Ship nominal distinct types; leave a
clean seam (`E0127` already says "units are a future feature"). Units are a
strict superset and deserve their own card when the type system can carry
dimensional algebra. Aligns with I8 and "measure twice, cut once."

### D-DIST3 — Coercion, unwrap, and arithmetic rules (rec A)

How a distinct type relates to its base. The safety of the whole feature lives
here: any *implicit* base↔distinct coercion defeats the point.

- **Option A — explicit both ways; opt-in same-type arithmetic (recommended).**
  Construct with `UserId(expr)`. Unwrap with one named method `.raw()` (named-
  method style matches S42's `.to_int()`/`.to_float()` casts). **No** implicit
  coercion either direction. Arithmetic is **opt-in** via a `#Numeric` marker
  (S55 culture: auto `==`, explicit opt-in for arithmetic): a `#Numeric` distinct
  type inherits base operators only when both operands are the *same* distinct
  type, yielding that type; an unmarked distinct type gets `==` (derived
  `Equatable`) but no arithmetic (E0127). The marker is the discriminator —
  `UserId` and `Meters` are both distinct-over-numeric, so without it sema
  couldn't tell an id from a quantity.

    ```jet
    UserId :: distinct Int   // no #Numeric -> id, no arithmetic

    #Numeric
    Meters :: distinct Float

    u :: UserId(42)          // explicit construct
    n :: u.raw()             // explicit unwrap -> Int
    m :: Meters(3.0) + Meters(4.0)   // -> Meters(7.0)  (#Numeric)
    bad :: u + UserId(1)     // E0127: a UserId is an id, not a number
    ```

- **Option B — implicit unwrap to base (one-way coercion).** A distinct value is
  accepted anywhere its base is expected (distinct → base flows implicitly); only
  base → distinct needs `UserId(...)`. More convenient, but a `UserId` silently
  becoming an `Int` argument re-opens the mixup the feature exists to prevent.

    ```jet
    fn log_id(n: Int) { print("{n}") }
    u :: UserId(42)
    log_id(u)                // compiles under B — UserId silently decays to Int
    ```

- **Option C — explicit, but unwrap via field-like accessor `.value`.** Same as
  A but the unwrap reads `u.value` instead of a method `u.raw()`. Risk: looks
  like struct-field access and invites treating the distinct type as a struct;
  `.raw()` reads as a deliberate conversion.

    ```jet
    u :: UserId(42)
    n :: u.value             // unwrap via accessor
    ```

**Recommendation:** **Option A.** Explicit both directions keeps the safety
guarantee whole; opt-in same-type-only arithmetic gives `#Numeric` distinct
types primitive feel without leaking; `.raw()` reads as an intentional conversion in
the S42 named-cast family. The unwrap *spelling* (`raw()` vs `value()` vs
`unwrap()`) is the one sub-choice worth the owner's eye — `raw()` is the
recommendation.
