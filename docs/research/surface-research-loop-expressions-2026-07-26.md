# Surface research: loop expressions beyond comprehensions

## Question

How can Jet make one `loop` composable as either a statement or an evaluated
expression without looking like Python syntax inside Jet?

The target is not the shortest isolated list example. The target is one surface
that still reads well with filters, nested sources, maps, effects, failures,
lazy consumers, and whole-loop results.

## Jet constraints

- Keep the five ratified `loop` headers.
- Keep `{ ... }` as the statement body.
- Keep `->` as Jet's evaluated control-flow arrow. Keep `=>` for lambdas.
- Keep one lazy `Iter<T>` mechanism.
- Do not make brackets or typed literals execute an unrelated mini-language.
- Do not hide filtering, flattening, allocation, or failure propagation.
- Preserve bare `break` and `next`, plus ratified named dot actions.

## Peer evidence

### Scala 3: one clause frame, two body markers

Scala uses the same `for` clauses for effects and values. `do` introduces the
statement body. `yield` introduces the evaluated body. Filters and nested
generators stay in the clause frame.

```scala
for user <- users do audit(user)

for
  user <- users
  if user.active
yield user.name
```

This is the strongest evidence for changing the body marker instead of wrapping
the loop in collection syntax. The weakness for Jet is that `yield` already
means stream suspension.

Sources:

- [Scala 3 control structures](https://docs.scala-lang.org/scala3/book/control-structures.html)
- [Scala 3 control syntax](https://docs.scala-lang.org/scala3/reference/other-new-features/control-syntax.html)

### Julia: delimiters choose eager or lazy

Julia keeps result-first comprehension order. Brackets build an array;
parentheses create a lazy generator.

```julia
[f(x) for x in xs if p(x)]
(f(x) for x in xs if p(x))
```

The eager/lazy distinction is visually clear. The cost is a second clause
grammar and delimiter-dependent meaning. That conflicts with Jet's goal of
making `loop` the one controller.

Source:

- [Julia arrays, comprehensions, and generators](https://docs.julialang.org/en/v1.0.4/manual/arrays/)

### Kotlin: explicit eager and lazy builders

Kotlin separates eager collection builders from lazy sequence builders.
`buildList` mutates a scoped builder with `add`. A sequence builder emits with
`yield`, and a terminal such as `toList` materializes it.

```kotlin
buildList {
    for (user in users)
        if (user.active) add(user.name)
}

sequence {
    for (user in users)
        if (user.active) yield(user.name)
}.toList()
```

The timing is explicit, but the common transformation is command-heavy.

Sources:

- [Kotlin `buildList`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/build-list.html)
- [Kotlin sequences](https://kotlinlang.org/docs/sequences.html)

### Swift: result-builder blocks

Swift result builders let ordinary expressions, conditions, switches, and loops
contribute values to an enclosing result. This creates a clean declarative body
without an explicit `yield`.

```swift
List {
    for user in users {
        if user.active {
            Text(user.name)
        }
    }
}
```

This is the most radical useful option for Jet. Its advantage is visual calm.
Its cost is contextual emission: an expression that normally computes one value
now contributes to a hidden builder, and nested control flow can flatten.

Source:

- [Swift result-builder attribute](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/attributes/)

### Rust: whole-loop values, not finite-loop projection

Rust lets `loop` and labeled blocks return a value through a break operand.
Finite `for` and `while` loops remain statement-like.

```rust
let connection = loop {
    if let Ok(value) = connect() {
        break value;
    }
};
```

This supports a clean Jet whole-loop result, but it does not solve per-iteration
projection.

Source:

- [Rust loop expressions](https://doc.rust-lang.org/stable/reference/expressions/loop-expr.html)

### Go: producer callbacks expose early stop

Go can range over a producer function that receives a `yield` callback. The
callback result tells the producer whether iteration should continue.

This keeps cleanup and early stop explicit at the protocol boundary, but it is
an implementation model rather than a pleasant surface for everyday projection.

Sources:

- [Go range functions](https://go.dev/blog/range-functions)
- [Go language specification](https://go.dev/ref/spec)

## Candidate Jet families

### 1. Projection arrow

```jet
loop user; users { audit(user) }

names :: loop user; users if user.active -> user.name
by_id :: loop user; users -> user.id: user
```

The body marker chooses the role: braces execute statements; the arrow
evaluates a projection. This is the smallest Jet-native extension because `->`
already marks evaluated control-flow arms.

### 2. Yield clause

```jet
names :: loop user; users if user.active yield user.name
by_id :: loop user; users yield user.id: user
```

This is highly readable and proven by Scala. It gives `yield` a second role,
however: body delimiter here, suspension statement in a stream function.

### 3. Result-builder body

```jet
names :: loop user; users {
    if user.active { user.name }
}
```

Expressions contribute to the loop result. Conditions omit values and nested
loops flatten. This is the calmest surface and the largest semantic departure.

### 4. Explicit emissions

```jet
names :: loop user; users {
    if user.active { emit user.name }
}
```

One body can emit zero, one, or many values. A new `emit` keyword avoids
overloading stream suspension. The behavior is explicit, but the common
one-input/one-output case becomes a command inside a block.

## Recommendation

Use the projection arrow:

```jet
names :: loop user; users if user.active -> user.name
```

It looks like Jet because the loop header remains the controller, `if` remains
the guard, and `->` remains the boundary between control and evaluation.
Statement and evaluated bodies are visibly different without collection
wrappers, terminal noise, or a new query order.

Keep eager versus lazy as a separate semantic decision. The same clean surface
can return an eager collection, a lazy iterator, or a source-shaped collection.
Do not corrupt the syntax ballot with materialization punctuation.

Retain the result-builder body as the ambitious alternative. It is the only
other family that materially rethinks the model instead of respelling the same
projection.
