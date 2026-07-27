# Surface research: yielding loops, comprehensions, and typed task groups

## Revision 3: arrow means return

This revision supersedes revisions 1 and 2.
The peer-language research remains useful background.

## Arrow law

`->` marks a returned or evaluated body.
It does not mean “one-line body.”

The returned body can be one expression:

```jet
if ready -> run()
```

It can also be a block:

```jet
if ready -> {
    value :: run()
    value
}
```

Loops should use the same law.

## One loop body model

### Returned expression

```jet
loop item; items -> use(item)
```

`use(item)` returns `Void`, so the loop returns `Void`.

```jet
names :: loop user; users if user.active -> user.name
```

`user.name` returns `String`, so the source loop returns `[String]`.

### Returned block

Use a returned block when evaluation needs several lines:

```jet
names :: loop user; users if user.active -> {
    name :: user.name.trim()
    if user.admin { "admin:{name}" } else { name }
}
```

The tail expression is the returned iteration value.

### Effect block

Use a bare block when the loop only runs statements:

```jet
loop user; users {
    audit(user)
    update_metrics(user)
}
```

The distinction is now exact:

- `-> expression` returns the expression;
- `-> { ... }` returns the block tail;
- `{ ... }` runs an effect block and returns `Void`.

Braces appear only for several lines or explicit visual grouping.

## Result cardinality

The loop header selects result cardinality.
The returned body selects result type.

### Source, counted, and finite condition loops

- A `Void` body returns `Void`.
- A `T` body returns `[T]`.
- A key-value body returns `[K: V]`.
- A header guard or `next` omits the current iteration.
- Natural exhaustion returns the completed collection.
- `break` returns the collection built so far.

```jet
names :: loop user; users -> {
    if !user.active -> next
    user.name
}
```

### No-source loop

A returned body produces one final value.
`next` starts another attempt.

```jet
connection :: loop -> {
    attempt :: connect(server)
    if !attempt.ok -> next
    attempt
}
```

This is one return rule:

- a source header returns one body value per source item;
- no source returns the first body evaluation that completes.

A named payload exit remains available:

```jet
connection :: loop -> {
    loop reply; replies {
        if reply.ready -> break(connection, reply.value)
    }
    next
}
```

## Compact several-source loops

Simple comprehensions stay on one line:

```jet
names :: loop user; users if user.active -> user.name
```

Several sources are complex enough to format as clauses:

```jet
rows :: loop
    team; teams
    user; team.users if user.active
-> .{
    team: team.name,
    user: user.name,
}
```

Each header line nests under the previous line.
The final arrow returns one value for each accepted source tuple.

The parser treats each terminated header line as one source clause.
Indentation improves layout but does not define scope.

An optional comma keeps a short two-source form on one line:

```jet
pairs :: loop left; lefts, right; rights -> .{ left, right }
```

The formatter expands this form when it exceeds the width limit.

Lockstep iteration stays explicit:

```jet
pairs :: loop pair; lefts.zip(rights) -> pair
```

An explicit loop in the returned body creates a nested result:

```jet
groups :: loop team; teams ->
    loop user; team.users -> user.name
```

The rules remain visible:

- header clauses produce a flat comprehension;
- returned loop values produce nested collections.

## Maps without typed bindings

Jet retired typed local bindings.
The key-value returned body selects a map directly:

```jet
by_id :: loop user; users -> user.id: user
```

The loop remains eager.
The body shape selects the collector, not evaluation time.

## Current C-style header

The current explicit-state header needs no extra mechanism.
The returned body fits after its afterthought:

```jet
loop i := 0; i < limit; i++ -> draw(i)
```

`draw(i)` returns `Void`, so this remains an effect loop.

The same header can return a collection:

```jet
squares :: loop i := 0; i < limit; i++ -> i * i
```

A multiline projection keeps the arrow:

```jet
rows :: loop i := 0; i < limit; i++ -> {
    source :: inputs[i]
    normalize(source)
}
```

`next` still runs the afterthought before it retests the condition.
`break` returns the collection built so far.

A named C-style loop also stays unchanged:

```jet
search :: loop i := 0; i < limit; i++ -> {
    if wanted(items[i]) -> break(search, items[i])
    next
}
```

This adaptation is mechanically small.
Its main cost is the existing semicolon-heavy header.

## From-scratch ergonomic family

The current form is not the only answer.
This section ignores Jet compatibility and starts from control-flow roles.

Use four familiar controllers:

- `for` iterates a source or an explicit state header;
- `while` repeats while a condition is true;
- `forever` repeats until an exit;
- `TaskGroup` owns structured child work.

Use two body markers everywhere:

- `:` introduces one effect expression and discards its value;
- `->` introduces one returned expression or returned block.

Braces contain multiline bodies.

### Effect iteration

```jet
for user in users: audit(user)

for i := 0; i < limit; i++: draw(i)

while ready: poll()

forever: tick()
```

### Returned iteration

```jet
names :: for user in users if user.active -> user.name

squares :: for i := 0; i < limit; i++ -> i * i
```

### Multiline effect body

```jet
for user in users {
    audit(user)
    update_metrics(user)
}
```

### Multiline returned body

```jet
labels :: for user in users -> {
    name :: user.name.trim()
    if user.admin -> "admin:{name}" else -> name
}
```

The same control distinction can apply to `if`:

```jet
if ready: run()

label :: if ready -> "ready" else -> "waiting"
```

Colon means “run and discard this.”
Arrow means “return this.”

In this from-scratch family, spelling—not inferred body type—sets the loop
contract:

- `for source: body` returns `Void`, even when `body` produces a value;
- `for source -> body` returns one body value per accepted item;
- a returned source body that resolves to `Void` is rejected with a fix to use
  `:`;
- `while` and `forever` follow the same effect/returned distinction.

This prevents a library function changing from `Void` to `T` from silently
turning an effect loop into a collection.

### Several sources

```jet
rows :: for team in teams,
             user in team.users
             if user.active
-> Row.{
    team: team.name,
    user: user.name,
}
```

Each `name in source` clause nests under the prior clause.
Comma-separated clauses form one flat comprehension.

Lockstep iteration stays explicit:

```jet
pairs :: for pair in lefts.zip(rights) -> pair
```

### Collector types

An omitted collector defaults to `List`.
An explicit type head changes collection shape and timing visibly:

```jet
names :: for user in users -> user.name

by_id :: Map for user in users -> (user.id, user)

unique_names :: Set for user in users -> user.name

lazy_names :: Iter for user in users -> user.name
```

`Map for` requires a returned `(K, V)` pair.
`Iter for` is lazy because `Iter` is written in the expression.

This is not type-directed timing from an annotation.
The collector is an explicit expression head.

### Retry and search

```jet
connection :: forever -> {
    attempt :: connect(server)
    if !attempt.ok: next
    attempt
}
```

A named outer search uses its result binding:

```jet
found :: forever -> {
    for row in rows {
        for cell in row {
            if wanted(cell): found.break(cell)
        }
    }
    next
}
```

### From-scratch strengths

- Source loops use familiar `for name in source`.
- C-style loops keep their familiar three clauses.
- Colon clearly marks one effect expression.
- Arrow clearly marks returned data.
- Braces appear only for multiline work.
- `for`, `while`, and `forever` reveal controller purpose.
- Collector types make List, Map, Set, and Iter behavior visible.
- Complex comprehensions remain source-first.

### From-scratch costs

- Jet gains `for`, `in`, `while`, and `forever`.
- One `loop` controller becomes several words.
- Colon gains a control-body role.
- `Map for` and `Iter for` need type-headed comprehension grammar.
- Existing loop code and decisions need a full migration.

This family has better first-read ergonomics than the current semicolon source
header.
It also costs more language surface.

## Adapt or restart

The minimal adaptation is:

```jet
names :: loop user; users if user.active -> user.name
squares :: loop i := 0; i < limit; i++ -> i * i
```

The from-scratch form is:

```jet
names :: for user in users if user.active -> user.name
squares :: for i := 0; i < limit; i++ -> i * i
```

Only the source header changes in these examples.
That source header carries most of the visual difference.

If the current semicolon header remains law, adapt it.
If beginner readability wins over one-controller minimalism, restart with the
`for` family.

## Named loops

The left binding already names a loop:

```jet
outer :: loop row; rows {
    loop cell; row {
        if cell.bad -> break(outer)
    }
}
```

A returned loop should reuse that same name.
Do not add a second label:

```jet
connection :: loop -> {
    attempt :: connect(server)
    if !attempt.ok -> next
    attempt
}
```

Inside the loop, `connection` is its control name.
After the loop, `connection` is its returned value.

Nested code can use the same name:

```jet
connection :: loop -> {
    loop reply; replies {
        if reply.ready -> break(connection, reply.value)
    }
    next
}
```

This shape stays rejected:

```jet
result :: outer :: loop -> { ... }
```

The result name already gives the loop a clear control name.

## Scoped task-group binding

Type-first `TaskGroup g` looks unlike ordinary Jet bindings.
Scoped-call lambdas add punctuation.
Repeating `group.task` on every child is also noise.

The from-scratch form can make both the group and its children scoped type
expressions:

```jet
profile :: TaskGroup -> {
    user :: Task -> fetch_user(id)
    billing :: Task -> fetch_billing(id)
    Task.all(user, billing)
}
```

This form gives each token one job:

- `profile` names the returned group result;
- `::` binds it;
- `TaskGroup` opens a structured scope;
- `Task` creates a child in the nearest lexical group;
- each arrow marks a returned body;
- braces appear because the group has several lines.

A multiline task uses a returned block:

```jet
profile :: TaskGroup -> {
    user :: Task -> {
        record :: fetch_user(id)
        validate(record)?
        record
    }
    user.join()
}
```

An effect-only group needs no binding or arrow:

```jet
TaskGroup {
    Task -> refresh_cache()
    Task -> refresh_index()
}
```

A helper can keep a returned group on one line:

```jet
profile :: TaskGroup -> launch_profile_tasks()
```

Expert settings stay on the scoped type:

```jet
profile :: TaskGroup(limit: 8) -> {
    ...
}
```

`TaskGroup` remains scope-bound:

- `Task` is valid only inside a lexical `TaskGroup`;
- every child joins or cancels before body exit;
- borrowed captures remain sema-checked;
- `TaskGroup.new()` does not exist;
- the group cannot move, store, return, or escape.

`Task.all`, `Task.race`, and `Task.any` use the nearest lexical group and reject
handles owned by another group.

This is a compiler-known scoped type family.
It looks like a type but is not a normal constructible stateful value.

That buys the clean surface but creates real costs:

- `Task` has an ambient lexical owner instead of an explicit receiver;
- nested groups require the compiler and editor to reveal which group owns a
  task;
- apparently ordinary PascalCase names have privileged construction rules;
- passing a task-group capability through generic code is harder;
- future third-party scoped types cannot copy the mechanism unless Jet exposes
  a general scoped-type protocol.

Making `TaskGroup` a fully ordinary type avoids that compiler privilege, but
reintroduces construction, activation, escape, and cleanup states. The user can
then hold a group without entering it or leave it without joining children.
That is a worse default for structured concurrency.

## Recommended package

```jet
// One-line effect: colon means run.
for item in items: audit(item)

// One-line collection: arrow means return.
names :: for user in users if user.active -> user.name

// Multiline returned projection.
labels :: for user in users -> {
    name :: user.name.trim()
    if user.admin -> "admin:{name}" else -> name
}

// Current C-style state header also works.
squares :: for i := 0; i < limit; i++ -> i * i

// Several dependent sources.
rows :: for team in teams,
             user in team.users
             if user.active
-> .{ team: team.name, user: user.name }

// Retry until one body evaluation returns.
connection :: forever -> {
    attempt :: connect(server)
    if !attempt.ok: next
    attempt
}

// Typed structured scope and typed child expressions.
profile :: TaskGroup -> {
    user :: Task -> fetch_user(id)
    billing :: Task -> fetch_billing(id)
    Task.all(user, billing)
}
```

## Costs that remain

The from-scratch family adds `for`, `in`, `while`, and `forever`.

Colon gains one precise role: introduce a single effect body and discard its
value.

The body marker and source header jointly select result cardinality.

The bound result name has control meaning inside and value meaning after a
returned loop.

`TaskGroup` and `Task` need compiler-known scoped-type behavior.

These costs buy the clean surface:

- no `yield`;
- no `collect`;
- no mandatory one-line braces;
- no scoped-call lambda;
- no repeated group receiver;
- no lazy timing hidden in a type.

## Status

This report gives research and design options. It does not ratify syntax.

## Outcome

The current loop ballot is hard to judge because it splits one user experience
across three decisions:

- `D-LOOPEVAL1` chooses the per-item body.
- `D-COMPREHENSION1` chooses the result type and evaluation time.
- `D-LOOPSTATE1` chooses one whole-loop result.

Those ballots contain 5, 5, and 4 options. Not every combination is valid, but
the owner must still reason about a large cross-product.

The strongest peer designs do not use one rule for all three jobs.

- Scala and Elixir use clauses for per-item projection.
- F# uses explicit `yield` and `yield!` for zero-to-many production.
- Python uses delimiters to select eager or lazy results.
- Rust uses `break value` only for one whole-loop result.
- C# accepts a large query vocabulary for joins, grouping, and ordering.

Jet should ballot complete packages. It should not ask the owner to assemble a
language from independent punctuation and timing choices.

My best conservative package is:

1. Use `yield` or `->` as the terminal projection boundary.
2. Add multiple source clauses for complex comprehensions.
3. Make evaluated source loops eager `List` or `Map` construction.
4. Keep lazy work in the existing `Iter` adapter model.
5. Use `break value` for one whole-loop result.
6. Do not make a type annotation change evaluation time.

For task groups, use a type-named scoped operation:

```jet
TaskGroup.scope(g => {
    a :: g.task { fetch_a() }
    b :: g.task { fetch_b() }
    g.all([a, b])
})
```

`TaskGroup` should not become a freely constructible or escapable value.

## Why the current loop package feels uneven

### The body options have different power

The arrow and terminal `yield` options produce exactly one value for each
accepted iteration. The builder and `emit` options can produce zero or many
values from arbitrary nested control flow.

These are different mechanisms. They are not punctuation choices.

### The recommended whole-loop arrow adds a second meaning

In a source loop, `->` means “project one value for this item.”

In the recommended no-source loop, `->` means “repeat on `next`, but return when
the body completes.”

That second rule is useful, but it is not the same operation. Rust uses
`break value` for this job and avoids the extra completion rule.

### Type-directed timing hides a runtime change

The recommended materialization option lets an expected type choose eager
`List` or lazy `Iter`.

This is concise, but changing an annotation can move effects and errors from
the binding site to a later consumer. Type inference should not quietly move
I/O, mutation, or failure in time.

### Simple examples hide the hard questions

A filter and projection are easy:

```jet
names :: loop user; users if user.active -> user.name
```

The design becomes harder with:

- several dependent sources;
- Cartesian products versus `zip`;
- local computed bindings;
- nested results versus flattening;
- zero or many outputs;
- maps and repeated keys;
- eager versus lazy effects;
- fallible projections;
- asynchronous sources;
- search and retry loops;
- labeled exits.

The ballot should show these cases together.

## Peer-language evidence

### Python: compact result-first comprehensions

Python puts the projected value first. Delimiters select the result family.

```python
names = [user.name for user in users if user.active]
by_id = {user.id: user for user in users}
lazy_names = (user.name for user in users if user.active)
```

Dependent sources and filters stay in execution order after the result:

```python
pairs = [
    (i, j)
    for i in range(limit)
    for j in range(i, limit)
    if i + j == target
]
```

Python also permits asynchronous sources and projections:

```python
decoded = [
    await decode(packet)
    async for packet in packets
    if packet.valid
]
```

Strengths:

- The simple form is short.
- List, set, map, and lazy generator results are visible.
- Dependent sources and asynchronous sources compose.

Costs:

- The result appears before the control that produces it.
- Long forms read in two directions.
- Delimiters carry collection and timing semantics.
- The comprehension runs in an implicit nested scope.

Jet lesson: copy neither the result-first order nor the bracket mini-language.
Keep the useful visible distinction between eager and lazy work.

Source: [Python expression reference](https://docs.python.org/3/reference/expressions.html)

### Scala 3: one clause frame, `do` or `yield`

Scala keeps generators, guards, and local values in source-first order.

```scala
val names =
  for
    user <- users
    if user.active
  yield user.name
```

Several generators form a dependent product:

```scala
val rows =
  for
    team <- teams
    user <- team.users
    if user.active
  yield (team.name, user.name)
```

The same clause frame can run effects:

```scala
for
  user <- users
  if user.active
do audit(user)
```

Scala translates the value form through `withFilter`, `map`, and `flatMap`.
The first generator helps determine the result container.

Strengths:

- The simple and multiline forms keep execution order.
- Guards and dependent sources scale well.
- `do` and `yield` clearly split effects from values.
- Existing collection operations define composition.

Costs:

- `yield` is a terminal delimiter, not a suspension point.
- Result type and timing can follow the source family.
- The same syntax can target collections or effect types.
- Translation details can surprise users in complex cases.

Jet lesson: this is the strongest model for a source-first comprehension.
Jet should keep one known eager collector instead of copying source-family
timing.

Sources:

- [Scala for comprehensions](https://docs.scala-lang.org/tour/for-comprehensions.html)
- [Scala 3 better fors](https://docs.scala-lang.org/scala3/reference/other-new-features/better-fors.html)

### Elixir: generators, filters, target, and reduction

Elixir uses a `for` special form with generators and filters.

```elixir
for user <- users, user.active, do: user.name
```

Several generators and local bindings scale vertically:

```elixir
for dir <- dirs,
    file <- File.ls!(dir),
    path = Path.join(dir, file),
    File.regular?(path) do
  File.stat!(path).size
end
```

The `into` option selects another collection:

```elixir
for {key, value} <- scores, into: %{} do
  {key, value * value}
end
```

Comprehensions can also reduce:

```elixir
for item <- cart, reduce: 0 do
  total -> total + item.count
end
```

Strengths:

- Complex control remains source-first.
- Pattern generators filter failed matches.
- The collector target is explicit.
- Reduction uses the same clause frame.

Costs:

- Options add a local query language.
- Reduction changes the body contract.
- `into` can target many protocols with different behavior.

Jet lesson: multi-source clauses are valuable. `into`, `reduce`, and protocol
targets would duplicate Jet collectors and reducers.

Source: [Elixir comprehensions](https://hexdocs.pm/elixir/comprehensions.html)

### F#: producer blocks with `yield` and `yield!`

F# sequence expressions put ordinary control flow inside a lazy sequence
builder.

```fsharp
seq {
    for user in users do
        if user.Active then
            yield user.Name
}
```

Nested loops produce a flat multiplication table:

```fsharp
seq {
    for i in 1..9 do
        for j in 1..9 do
            yield (i, j, i * j)
}
```

`yield!` explicitly flattens another sequence:

```fsharp
let rec inorder tree =
    seq {
        match tree with
        | Branch(value, left, right) ->
            yield! inorder left
            yield value
            yield! inorder right
        | Leaf value ->
            yield value
    }
```

Strengths:

- Arbitrary control flow can produce zero or many values.
- `yield!` makes flattening visible.
- The `seq` wrapper makes laziness and the yield target clear.

Costs:

- The common projection is verbose.
- The wrapper owns special builder semantics.
- Jet has rejected wrapper-heavy collection forms.
- Jet would need a flattening form equal to `yield!`.

Jet lesson: explicit output scales better than a projection when one iteration
can emit several values. The missing wrapper makes the yield target harder in
Jet.

Source: [F# sequences](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/sequences)

### C#: a full query language

C# query expressions support several sources, local values, joins, filters,
ordering, grouping, projection, and continuation.

```csharp
var report =
    from customer in customers
    join order in orders
        on customer.Id equals order.CustomerId into customerOrders
    let count = customerOrders.Count()
    where count >= 10
    orderby count descending
    select new { customer.Name, Count = count };
```

C# translates this syntax to methods such as `Where`, `Select`, `SelectMany`,
`Join`, `GroupJoin`, `OrderBy`, and `GroupBy`.

Strengths:

- Complex data work remains readable.
- Joins and grouping are first-class.
- Contextual keywords limit global reservation.

Costs:

- The feature adds many words and translation rules.
- It creates a second control language.
- Method availability changes which query syntax works.

Jet lesson: stop the loop feature before joins, grouping, and ordering. Those
operations belong to typed library methods.

Source: [C# query expression specification](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/expressions#1223-query-expressions)

### Haskell: Cartesian products and parallel zip are different

An ordinary Haskell comprehension with two generators forms a product:

```haskell
[(x, y) | x <- xs, y <- ys]
```

GHC parallel comprehensions use separate branches and stop at the shortest:

```haskell
[(x, y) | x <- xs | y <- ys]
```

Strengths:

- The syntax distinguishes product from zip.
- The distinction scales to several branches.

Costs:

- A small punctuation change has large semantic weight.
- The parallel form needs an extension and more translation rules.

Jet lesson: several source clauses should mean dependent nested iteration.
Use the existing `zip` operation for lockstep iteration.

Source: [GHC parallel list comprehensions](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/parallel_list_comprehensions.html)

### Rust: one whole-loop result through `break value`

Rust keeps finite `for` and `while` loops statement-like. An infinite `loop`
can return one value through `break`.

```rust
let connection = loop {
    match connect(server) {
        Ok(value) => break value,
        Err(_) => backoff(),
    }
};
```

A labeled `break` can return through an outer loop.

Strengths:

- Every returning path is visible.
- The body keeps ordinary statement meaning.
- No separate completion expression is needed.
- The rule does not overlap per-item projection.

Costs:

- The normal success path uses an exit command.
- Finite loops do not get a nontrivial result.

Jet lesson: use `break value` and `break(outer, value)` for one whole-loop
result. Do not make `loop ->` mean both projection and successful completion.

Source: [Rust loop expressions](https://doc.rust-lang.org/reference/expressions/loop-expr.html)

## Cross-language design map

| Question | Strong peer answer | Jet risk |
|---|---|---|
| One output per item | Scala terminal `yield` | `yield` already suspends streams |
| Zero or many outputs | F# `yield` and `yield!` | Jet lacks a clear builder owner |
| Several dependent sources | Scala and Elixir clauses | This extends the loop header grammar |
| Product versus zip | Haskell uses different forms | Tiny punctuation can hide major behavior |
| Eager versus lazy | Python and Julia use delimiters | Jet should not add collection wrappers |
| Collector selection | Elixir uses `into` | A collector protocol can duplicate existing APIs |
| One whole-loop result | Rust uses `break value` | Success uses an exit command |
| Joins and grouping | C# adds a query language | This would violate Jet’s one-controller goal |

## Jet shot 1: source-first `yield`

This option follows Scala. It adds a complete clause form instead of an
evaluated body mode.

### Simple projection

```jet
names :: loop user; users if user.active yield user.name
```

### Map construction

```jet
by_id :: loop user; users yield user.id: user
```

### Local work

```jet
labels :: loop user; users if user.active yield {
    name :: user.name.trim()
    if user.admin { "admin:{name}" } else { name }
}
```

### Several dependent sources

```jet
rows :: loop
    team; teams
    user; team.users
    if user.active
yield .{
    team: team.name,
    user: user.name,
}
```

Each source clause nests under the previous clause. The result is flat because
this is one comprehension with one terminal projection.

A nested evaluated loop remains a nested result:

```jet
groups :: loop team; teams yield
    loop user; team.users yield user.name
```

Use `.flatten()` when the nested result must become one list.

### Product versus zip

Two source clauses form a dependent product:

```jet
pairs :: loop
    left; lefts
    right; rights
yield .{ left, right }
```

Lockstep iteration stays explicit:

```jet
pairs :: loop pair; lefts.zip(rights) yield pair
```

### Whole-loop result

```jet
connection :: loop {
    attempt :: connect(server)
    if attempt.ok { break attempt }
    backoff()
}
```

Named exits use the current dot-action model:

```jet
result :: outer :: loop {
    loop reply; replies {
        if reply.ready { break(outer, reply.value) }
    }
}
```

### Result and timing law

- A value projection returns an eager `List<T>`.
- A key-value projection returns an eager `Map<K, V>`.
- Source and body effects run at the binding.
- Repeated map keys follow the existing map insertion law.
- Unbounded sources cannot use this eager form.
- Lazy transformations stay in `Iter` adapters and stream functions.
- Fallible projection follows a later explicit failure ruling.

### Tradeoffs

Benefits:

- The form is conventional and source-first.
- `yield` clearly names the produced value.
- Multiline clauses solve complex flattening without nested builders.
- `break value` keeps whole-loop results separate.
- Timing does not change with a type annotation.

Costs:

- `yield` has a second role beside stream suspension.
- The loop grammar gains several source clauses.
- The evaluated form is eager only.
- One iteration cannot emit several unrelated values.
- A map projection still gives `:` contextual meaning.

## Jet shot 2: Jet projection arrow, complete package

This option keeps the current arrow idea but removes the unusual whole-loop
arrow and the type-directed timing rule.

### Simple projection

```jet
names :: loop user; users if user.active -> user.name
```

### Several dependent sources

```jet
rows :: loop
    team; teams
    user; team.users
    if user.active
-> .{
    team: team.name,
    user: user.name,
}
```

### Map construction

```jet
by_id :: loop user; users -> user.id: user
```

### Whole-loop result

```jet
connection :: loop {
    attempt :: connect(server)
    if attempt.ok { break attempt }
    backoff()
}
```

### Result and timing law

The law is identical to shot 1:

- evaluated source loops build eager `List` or `Map` values;
- lazy work stays in `Iter`;
- whole-loop values use payload exits;
- expected types do not move evaluation in time.

### Tradeoffs

Benefits:

- `->` already marks evaluated Jet control flow.
- The simple form is short.
- The language adds no output word.
- `yield` keeps only its stream suspension meaning.

Costs:

- `->` already appears in function returns, effects, and dispatch.
- The arrow does not say “collect” or “produce.”
- A multiline terminal arrow can look detached from its sources.
- Users can mistake it for a lambda or mapping operator.
- The same contextual map-entry concern remains.

## Which loop shot is stronger

Shot 1 is more conventional. Scala proves the source-first `yield` boundary at
both small and large sizes.

Shot 2 is more Jet-specific. It reuses a known token, but it asks one arrow to
carry more visual jobs.

I slightly prefer shot 1. Its second meaning for `yield` is teachable:

- a stream function yields values over time;
- an eager comprehension yields values into its result.

Both mean “this is the produced value.” The runtime timing differs, but the
data-flow meaning stays stable.

If one word must have only one runtime meaning, select shot 2.

Do not select type-directed lazy timing in either package. If Jet later needs
lazy full-control producers, ballot a distinct producer block with a visible
`Iter` or `Stream` boundary.

## Recommended ballot repair

Replace the current cross-product with complete packages:

### Package A: source-first `yield`

- Multi-source `loop ... yield`.
- Eager `List` or `Map`.
- Existing adapters for lazy work.
- `break value` for one whole-loop result.

### Package B: Jet projection arrow

- Multi-source `loop ... ->`.
- Eager `List` or `Map`.
- Existing adapters for lazy work.
- `break value` for one whole-loop result.

### Package C: explicit lazy producer

- A visible `Iter` or `Stream` builder boundary.
- Explicit `yield` and explicit flattening.
- `.to_list()` or `.to_map()` for storage.
- `break value` remains separate.

### Package D: no evaluated loops

- Keep statement loops.
- Keep adapters, reducers, streams, and mutable builders.

Show the same examples for every package:

1. one filter and projection;
2. a map with repeated keys;
3. two dependent sources;
4. a Cartesian product and a zip;
5. local work in the projection;
6. nested results and explicit flattening;
7. fallible projection;
8. effects and evaluation time;
9. an unbounded source;
10. retry with a labeled result exit.

## Task groups: keyword or type

## Current Jet model

Jet writes:

```jet
taskgroup g {
    a :: g.task { fetch_a() }
    b :: g.task { fetch_b() }
    g.all([a, b])
}
```

The parser treats `taskgroup` as a contextual statement word. Sema binds `g`
with the known type `TaskGroup`.

The group:

- owns child tasks;
- joins or cancels them on every exit;
- permits ratified safe borrowed captures;
- exposes `.task`, `.all`, `.race`, `.any`, and `.select`;
- cannot escape its lexical scope.

The archived `D-TASKSCOPE1` ballot compared names. It did not compare a keyword
against a type-centered API.

Its Swift comparison was incomplete. Swift uses a `TaskGroup` type, but code
gets that value through `withTaskGroup`. Swift does not add a `taskgroup`
keyword.

## What peers do

### Swift: type passed by a scope function

```swift
await withTaskGroup(of: Result.self) { group in
    for input in inputs {
        group.addTask { await work(input) }
    }
    return await group.reduce(into: []) { $0.append($1) }
}
```

`TaskGroup<ChildResult>` is a type. The supported creation path is
`withTaskGroup`. The function waits for all children before it returns.

Useful properties:

- the type is visible in documentation and tools;
- the group can act as an `AsyncSequence`;
- the scope function can return a separate result;
- the type system limits group escape.

Cost:

- all child results in one group share one result type;
- throwing and discarding groups use separate type or function families;
- the API name and generic arguments are long.

Sources:

- [Swift `TaskGroup`](https://developer.apple.com/documentation/swift/taskgroup)
- [Swift `withTaskGroup`](https://developer.apple.com/documentation/swift/withtaskgroup%28of%3Areturning%3Aisolation%3Abody%3A%29)

### Kotlin: scope function with a typed receiver

```kotlin
val result = coroutineScope {
    val a = async { fetchA() }
    val b = async { fetchB() }
    combine(a.await(), b.await())
}
```

`CoroutineScope` is an interface. `coroutineScope` creates a lexical child
scope and passes it as the block receiver.

Useful properties:

- the block can return a value;
- helper functions can require a `CoroutineScope` receiver;
- the scope carries dispatcher and cancellation context.

Cost:

- the receiver is often implicit;
- extension methods can hide which scope owns a child;
- Kotlin needs `suspend` function coloring.

Sources:

- [Kotlin `coroutineScope`](https://kotlinlang.org/api/kotlinx.coroutines/kotlinx-coroutines-core/kotlinx.coroutines/coroutine-scope.html)
- [Kotlin `CoroutineScope`](https://kotlinlang.org/api/kotlinx.coroutines/kotlinx-coroutines-core/kotlinx.coroutines/-coroutine-scope/)

### Python: constructible type activated by a scope statement

```python
async with asyncio.TaskGroup() as group:
    a = group.create_task(fetch_a())
    b = group.create_task(fetch_b())
```

`TaskGroup` is a class and asynchronous context manager. Construction alone
does not make the group active. The `async with` statement defines its useful
lifetime.

Useful properties:

- the type is ordinary and inspectable;
- the scope boundary is explicit;
- child task result types can differ.

Cost:

- the surface needs both construction and `async with`;
- failure can produce an `ExceptionGroup`;
- runtime checks reject inactive-group use.

Source: [Python `asyncio.TaskGroup`](https://docs.python.org/3/library/asyncio-task.html#task-groups)

### Rust: type supplied by a scope function

```rust
let sum = std::thread::scope(|scope| {
    let a = scope.spawn(|| left.iter().sum::<i32>());
    let b = scope.spawn(|| right.iter().sum::<i32>());
    a.join().unwrap() + b.join().unwrap()
});
```

`Scope` is a type. `thread::scope` creates it and gives a borrowed handle to the
closure. The scope joins all child threads before it returns.

Useful properties:

- scoped children can borrow stack data;
- lifetimes state the escape law;
- the scope function returns the block result.

Cost:

- explicit lifetime machinery supports the guarantee;
- panic handling still needs careful joins;
- the API has closure punctuation.

Sources:

- [Rust `thread::scope`](https://doc.rust-lang.org/std/thread/fn.scope.html)
- [Rust `Scope`](https://doc.rust-lang.org/std/thread/struct.Scope.html)

### Go `errgroup`: ordinary type with manual wait

```go
group, context := errgroup.WithContext(context)
group.Go(fetchA)
group.Go(fetchB)
if err := group.Wait(); err != nil {
    return err
}
```

`errgroup.Group` is a normal type. It adds error propagation and cancellation
to a group of goroutines.

Useful properties:

- ordinary methods configure limits and launch work;
- the type is easy to pass to helpers;
- no language syntax is needed.

Cost:

- `Wait` is manual;
- a group can be misused or reused;
- lexical lifetime is a convention, not a language guarantee;
- a zero group has different cancellation behavior.

Source: [Go `errgroup`](https://pkg.go.dev/golang.org/x/sync/errgroup)

## Task-group surface options for Jet

### Option 1: keep the contextual keyword

```jet
taskgroup g {
    a :: g.task { fetch_a() }
    b :: g.task { fetch_b() }
    g.all([a, b])
}
```

Benefits:

- The lifetime boundary is unmistakable.
- The beginner form has little punctuation.
- `return`, `?`, panic, and cancellation can keep block control flow.
- The compiler already owns cleanup and auto-join insertion.

Costs:

- The lowercase name looks unlike Jet types.
- The construct needs a dedicated AST statement.
- The group looks less discoverable through type documentation.
- The block cannot naturally act like an expression without more syntax.
- Policy fields can turn the keyword header into another mini-language.

### Option 2: type-named scoped operation

```jet
result :: TaskGroup.scope(g => {
    a :: g.task { fetch_a() }
    b :: g.task { fetch_b() }
    g.all([a, b])
})
```

Expert policy can use normal named arguments:

```jet
result :: TaskGroup.scope(
    limit: 8,
    cancellation: .FailFast,
    g => {
        handles :: loop item; items yield g.task { work(item) }
        g.all(handles)
    },
)
```

Benefits:

- `TaskGroup` follows PascalCase type naming.
- Documentation and completion start from one type.
- The body can return a value.
- Policy uses ordinary typed call arguments.
- The parser does not need a new statement grammar.
- The form follows Swift, Kotlin, and Rust.

Costs:

- The beginner form gains call and lambda punctuation.
- A normal closure changes `return` and control-flow meaning.
- D-TASKBORROW1 still needs compiler-known scoped lifetime checking.
- Cleanup must run on every nonlocal exit.
- A library-looking call can hide a strong concurrency boundary.
- The implementation remains intrinsic even if the parser becomes ordinary.

Required rule:

`TaskGroup.scope` must take a nonescaping scoped body. The `TaskGroup` handle,
child handles, and borrowed results must not escape.

The group itself should stay nongeneric. `Task<T>` handles carry result types.
This keeps one group able to own tasks with different result types.

### Option 3: PascalCase special block

```jet
TaskGroup g {
    a :: g.task { fetch_a() }
    b :: g.task { fetch_b() }
    g.all([a, b])
}
```

Benefits:

- The surface changes little.
- The name now looks like a type.
- The lifetime boundary remains visually strong.

Costs:

- It looks like construction but does not construct an ordinary value.
- It conflicts with Jet’s constructor idioms.
- It still needs dedicated parser and AST support.
- Users can reasonably expect to store or pass the apparent value.

This option gives type spelling without type behavior. I do not recommend it.

### Option 4: ordinary constructible `TaskGroup.new()`

```jet
group :: TaskGroup.new()
a :: group.task { fetch_a() }
b :: group.task { fetch_b() }
result :: group.all([a, b])
```

Benefits:

- The group is an ordinary stateful value.
- Helpers can accept and return it.
- No scoped callback syntax is needed.

Costs:

- The lexical join guarantee becomes hard to prove.
- A moved or stored group can outlive borrowed captures.
- Automatic cleanup cannot report child failures cleanly.
- Forgotten completion recreates unstructured concurrency.
- The design conflicts with ratified task-group safety.

Do not choose this option.

## Task-group tradeoff table

| Concern | Keyword block | `TaskGroup.scope` | `TaskGroup.new()` |
|---|---|---|---|
| Beginner brevity | Best | Good | Good at first |
| Type naming symmetry | Weak | Best | Best |
| Lexical lifetime | Built in | Built in if intrinsic | Easy to lose |
| Borrowed captures | Existing sema rule | Same sema rule | Unsafe without escape tracking |
| Return a group result | Needs new rule | Natural | Manual |
| Policy configuration | Header syntax | Named arguments | Methods |
| Type documentation | Split keyword and type | One type home | One type home |
| Parser cost | Dedicated statement | Ordinary call | Ordinary call |
| Compiler cost | Intrinsic | Still intrinsic | High safety cost |
| Nonlocal `return` | Natural block behavior | Needs an inline-body rule | Ordinary |
| Misuse after scope | Impossible | Impossible by type rule | Likely |

## Task-group recommendation

Reopen `D-TASKSCOPE1` only for the surface form. Keep its structured lifetime
law and all later ratified decisions.

Select `TaskGroup.scope(g => { ... })` if naming consistency matters more than
minimal punctuation.

Make it a compiler-known, nonescaping scoped operation. Do not expose a public
`TaskGroup.new()`.

The key distinction is:

- type-named does not mean freely constructible;
- type-centered does not mean runtime-only safety;
- ordinary call syntax does not remove sema ownership.

Swift and Rust both use this split. They expose a type but create its useful
value only through a lexical scope function.

## Proposed next decisions

1. Replace the three loop ballots with one package ballot.
2. Show all ten complex cases in each package.
3. Keep failure collection as a follow-up after syntax and timing are fixed.
4. Reopen the task-group surface ballot with keyword and scoped-type options.
5. Do not reopen structured lifetime, cancellation, borrowed capture, or
   combinator law.
