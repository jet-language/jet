# Yielding loops

**Status: ratified 2026-07-26.**

Authority:

- D-ARROW-CONTROL1=A;
- D-LOOPEVAL1=A;
- D-LOOPSTATE1=A;
- D-COMPREHENSION1=A.

## Rule

Jet keeps one `loop` controller.

An effect-only loop has no arrow:

```jet
loop user, users audit(user)

loop user, users {
    audit(user)
    notify(user)
}
```

A finite loop uses `->` when each accepted iteration yields one value:

```jet
names :: loop user, users -> user.name

labels :: loop user, users -> {
    name :: user.name.trim()
    if user.admin -> "admin:{name}" else -> name
}
```

The yielding body must return a non-unit value. The loop runs immediately and
returns `List<T>` in iteration order.

Braces only group a multiline body. They do not select effect or value
semantics.

## Existing headers

All current loop headers remain:

```jet
loop tick()
loop ready poll()
loop item, items audit(item)
loop (key, value), map audit(key)
loop i, 0..<limit, 2 audit(i)
loop i := 0, i < limit { draw(i); i += 1 }
```

Only source loops may yield items:

```jet
names :: loop user, users -> user.name
squares :: loop i, 0..<limit -> i * i
```

Bare infinite and condition-only loops have no exhaustion edge. They cannot use
a yield arrow.

## Filtering

A header guard filters before the body:

```jet
names :: loop user, users if user.active -> user.name
```

`next` omits the current item:

```jet
names :: loop user, users -> {
    if !user.active next
    user.name
}
```

## Several sources

Source clauses nest from left to right:

```jet
rows :: loop team, teams,
             user, team.users if user.active
-> Row.{
    team: team.name,
    user: user.name,
}
```

One header yields one flat List. An explicit inner yielding loop preserves
nesting:

```jet
groups :: loop team, teams ->
    loop user, team.users -> user.name
```

Lockstep iteration stays explicit through `zip`.

## Result shape

A yielding loop always returns an eager List:

```jet
names :: loop user, users -> user.name
```

Other collectors stay visible:

```jet
by_id := [UserId: User].{}
loop user, users by_id.add(user.id, user)

unique_names :: Set.from(loop user, users -> user.name)
lazy_names :: users.map(user => user.name)
```

Jet does not select allocation or evaluation timing from an expected type or
source family. It does not flatten, skip failures, or choose a custom collector
implicitly.

## Whole-loop values

An ordinary loop returns one final value only through a break payload:

```jet
connection :: loop {
    attempt :: connect(server)
    if !attempt.ok next
    break attempt
}
```

Every reachable payload exit must have one compatible type. Body completion
does not return the loop value.

## Named exits

A loop name stays on the declaration:

```jet
outer :: loop row, rows {
    ...
}
```

The complete exit family is:

```jet
break
break value
break(outer)
break(outer, value)

next
next(outer)
```

Parentheses immediately after `break` or `next` select a loop target. A bare
break operand is the payload for the innermost loop.

Example:

```jet
found :: loop {
    loop row, rows {
        loop cell, row {
            if wanted(cell) break(found, Val(cell))
        }
    }

    break None
}
```

Dot exits are retired.

## Exits from yielding loops

For a yielding loop:

- `break` returns the accumulated List;
- `break(name)` returns the named yielding loop's accumulated List;
- `next` omits the current item;
- `next(name)` omits the named loop's current item;
- `break value` and `break(name, value)` are rejected.

A payload conflicts with the loop's fixed `List<T>` result.

## Effects, failure, and ownership

The loop is eager. Its source and body run when the binding is evaluated.
Effects and failures occur there.

The loop borrows its source only while it runs. It does not create a new lazy
capture boundary or a second iterator mechanism.

A fallible item keeps its real type. Jet does not silently drop failures or
return from the surrounding function.

Mutable-view output remains gated by the existing view and lending laws.

## Formatter

Short loops stay on one line:

```jet
loop item, items audit(item)
values :: loop item, items -> transform(item)
```

Several operations use braces. Long source headers wrap before the yield
arrow. Nested yielded loops indent after the outer arrow.

The formatter never rewrites loops into adapters or adapters into loops.

## Required diagnostics

The implementation must register and snapshot diagnostics for:

- a yield arrow on a non-finite loop;
- a () yielding body;
- a yielded path with no item;
- incompatible item types;
- an invalid break payload in a yielding loop;
- incompatible ordinary-loop break payloads;
- a named exit with no enclosing target;
- a retired dot exit;
- a statement loop used where a value is required;
- ownership, failure, or mutable-view violations.

## Migration

```jet
// Before
outer.break()
outer.next()

// After
break(outer)
next(outer)
```

Existing statement loops lose braces only when the body is one clear line:

```jet
// Before
loop item, items { audit(item) }

// After
loop item, items audit(item)
```

Braces remain valid when they improve clarity.
