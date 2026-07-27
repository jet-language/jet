# Collection construction shapes for Jet

## Question

Jet needs a compact way to filter and project collections without creating a second iterator model. The form must read in execution order and fit Jet's existing surface.

## Jet constraints

- `loop binding; source { ... }` is the ordinary iteration form.
- `if` owns filtering and `=>` owns lambdas.
- Single `|` means alternatives. Jet has no general pipe operator.
- Iterator adapters stay lazy. Collection construction stays explicit and eager.
- `yield` already emits values from a `Stream<T>` function.
- A trailing block is an existing call form, but it currently accepts only a zero-parameter function.

## Peer ideas

### F# list and sequence expressions

F# puts normal `for`, `if`, and `yield` statements inside a list or sequence expression. Nested loops keep their normal block order.

Jet use:

```jet
names :: [loop user; users {
    if user.active { yield user.name }
}]
```

This shape reuses Jet's normal control flow. The collection brackets give `yield` an eager target.

Failure to avoid: do not make the last expression emit implicitly. Jet does not use implicit returns, so hidden collection emission would be inconsistent.

Source: [Microsoft Learn: F# sequences](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/sequences)

### Elixir comprehensions

Elixir writes generators and filters before the body. It can also select a target collection with `into`.

Jet use: keep source bindings and filters before output. Let the expected collection type choose List or Map instead of adding a separate `into` option.

Failure to avoid: do not import `<-`, `for`, `do`, or keyword-option syntax. Those forms conflict with Jet's `loop`, braces, and named arguments.

Source: [Elixir comprehensions](https://hexdocs.pm/elixir/1.18.4/comprehensions.html)

### Raku gather and take

Raku separates a collection scope from the points that emit values. Normal control flow can contain several emission points.

Jet use:

```jet
names :: [String].collect() {
    loop user; users {
        if user.active { yield user.name }
    }
}
```

This shape treats collection construction as a named constructor with an ordinary trailing block.

Failure to avoid: do not add a second word such as `take`. Jet already has `yield`, and two emission words would split one mechanism.

Source: [Raku gather and take](https://docs.raku.org/syntax/gather%20take)

### C# query expressions

C# starts with `from`, then applies `where`, and ends with `select`. The order is readable, but the feature owns a separate query vocabulary.

Jet use: preserve the source-first order without importing a query language.

Failure to avoid: do not add `from`, `where`, and `select`. Jet already has `loop`, `if`, and value expressions.

Source: [Microsoft Learn: C# query expressions](https://learn.microsoft.com/en-us/dotnet/csharp/linq/get-started/query-expression-basics)

## Candidate families

1. Preserve the compact source-first clause form.
2. Put an ordinary `loop` block inside collection brackets and use `yield`.
3. Use a typed `.collect()` constructor with a trailing block and `yield`.
4. Let `.collect()` accept guarded dispatch arms as a compact partial mapping function.
5. Keep pipelines and loops only.

The bracketed loop builder fits Jet best. It preserves normal control flow, supports nested loops, and gives eager collection construction a visible boundary. Its cost is more braces than a clause list.
