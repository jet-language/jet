# Surface research: split callable and control arrows

Vocabulary: [Jet vocabulary](../spec/vocabulary.md).

## Status

Research baseline accepted and ratified on 2026-07-26 through
D-ARROW-CONTROL1=A, D-LOOPEVAL1=A, D-LOOPSTATE1=A, and
D-COMPREHENSION1=A. Normative law lives in `docs/spec/syntax-decisions.md`.

This revision replaces the prior one-arrow proposal.

## Outcome

Jet should not force one arrow across callable definitions and control flow.

Use two distinct families:

- `=>` defines a callable result;
- `=[Effects]=>` defines an effectful callable result;
- `->` selects or yields a value;
- no arrow means effect-only control;
- `{ ... }` groups a multiline body.

Keep current Jet control names:

- `if`;
- `loop`;
- `taskgroup`;
- `return`;
- `break`;
- `next`;
- `comptime if`.

Do not add:

- `for`;
- `while`;
- `yield` for collection loops;
- `then`;
- `do`;
- colon control bodies;
- a general pipeline arrow.

## Token law

### Callable arrow

`=>` maps callable inputs to a result type or body.

```jet
fn double(value: Int) => Int

value => value * 2

attack: Int => strength * 2
```

### Effectful callable arrow

An effect row sits inside the callable arrow:

```jet
fn fetch(url: String) =[Net]=> Response

fn save(record: Record) =[DB.Write, Log]=> Void

fn hash(data: [U8]) =[]=> Digest
```

`=[]=>` states that the callable is pure.

### Control arrow

`->` selects or yields a value:

```jet
label :: if ready -> "ready" else -> "waiting"

.Ready -> start()

names :: loop user; users -> user.name
```

### No arrow

Effect-only control uses no arrow:

```jet
if ready run()

loop user; users audit(user)
```

Multiline effect control uses braces:

```jet
if ready {
    prepare()
    run()
}

loop user; users {
    audit(user)
    notify(user)
}
```

The syntax now states intent before type inference:

- no arrow: run and discard;
- `->`: produce a selected or yielded value;
- `=>`: define a callable result.

## Why this split works

Elixir and OCaml use arrows for callable clauses and pattern clauses. Jet can
keep that idea but separate callable construction from control selection.
[Elixir syntax reference](https://hexdocs.pm/elixir/syntax-reference.html)
[OCaml values and functions](https://ocaml.org/docs/values-and-functions)

Kotlin uses `->` for selected `when` branches and `=` for concise named
function bodies. Jet can keep `->` on selection while using its existing `=>`
for callable definitions.
[Kotlin control flow](https://kotlinlang.org/docs/control-flow.html)
[Kotlin functions](https://kotlinlang.org/docs/functions.html)

Scala separates effect loops from yielding comprehensions with `do` and
`yield`. Jet can make the same semantic split without two new keywords.
[Scala 3 control syntax](https://docs.scala-lang.org/scala3/reference/other-new-features/control-syntax.html)

Rust shows that loops can return values through explicit exits. Jet can keep
bare loops effect-oriented and use `break value` for their final result.
[Rust loop expressions](https://doc.rust-lang.org/reference/expressions/loop-expr.html)

## Functions

### Pure return contract

Replace function result `->` with `=>`:

```jet
fn double(value: Int) => Int {
    return value * 2
}
```

With a returned block tail:

```jet
fn normalize(value: Int) => Int {
    adjusted :: clamp(value)
    adjusted * 2
}
```

`return value` remains for early exits:

```jet
fn normalize(value: Int) => Int {
    if value < 0 return 0

    adjusted :: clamp(value)
    adjusted * 2
}
```

### Concise body

Use `=` because `=>` already introduces the result type:

```jet
fn double(value: Int) => Int = value * 2

fn User.label(self) => String = self.name
```

This avoids:

```jet
// Reject.
fn double(value: Int) => Int => value * 2
```

### Void function

A function with no result or effect contract needs no arrow:

```jet
fn log_user(user: User) {
    audit(user)
}
```

### Effects

Put effects inside the callable arrow:

```jet
fn fetch(url: String) =[Net]=> Response ? HTTPError

fn store(record: Record) =[DB.Write, Log]=> Receipt ? StoreError

fn checksum(data: [U8]) =[]=> Digest
```

An explicit effect row always names a result type. Use `Void` when needed:

```jet
fn run() =[IO]=> Void {
    print("ready")
}
```

Open effect rows stay inside:

```jet
fn apply<T, E>(value: T, body: fn(T) =[..E]=> T) =[..E]=> T
```

### Function types

Use the same callable arrow:

```jet
fn(Int) => String

fn(Request) =[Net, Log]=> Response ? HTTPError

fn() =[]=> Int
```

Named functions, methods, trait methods, callbacks, FFI declarations, and
function types now use one result syntax.

### Multi-head functions

```jet
fn area(Circle(radius: Float)) => Float =
    3.14 * radius * radius

fn area(Rect(width: Float, height: Float)) => Float =
    width * height
```

The parameter pattern selects a function head. `=>` still defines the callable
result.

## Lambdas

Keep lambda `=>`:

```jet
names :: users.map(user => user.name)

total :: values.fold(
    0,
    (sum: Int, value: Int) => sum + value,
)

task :: tasks.spawn(() => work())
```

Multiline:

```jet
users.each(user => {
    audit(user)
    notify(user)
})
```

### Captures

Do not use the stale `take(...)` capture prefix.

Escaping closures infer owned captures from use. An owned non-scalar capture
moves into the closure when required.

```jet
task :: tasks.spawn(() => process(file))
```

Copyable captures stay available without a preparatory binding:

```jet
task :: tasks.spawn(() => process(config))
inspect(config)
```

The closure gets its owned copy at closure creation.
The source binding remains available.

An owned non-copyable capture still moves into the closure.
No syntax can keep two owners of one non-copyable resource.

Mutable and borrowed captures remain subject to sema ownership checks.
This arrow proposal adds no capture syntax.

## Computed fields

Keep computed fields on the callable arrow:

```jet
struct Player {
    strength: Int
    gear_mod: Int
    attack: Int => strength * 2 + gear_mod
}
```

Multiline:

```jet
threat: Int => {
    base :: attack + gear_mod
    clamp(base)
}
```

A computed field defines how to produce its value. It does not select a control
branch.

## Effect-only `if`

### One-line body

Use no arrow:

```jet
if ready run()

if invalid return error

if !user.active next

if wanted(cell) break(outer, cell)
```

The parser reads one complete condition, then one complete body statement.
Jet calls require parentheses, so adjacent expressions remain separable:

```jet
if is_ready() run()
```

The condition is `is_ready()`. The body is `run()`.

### Else

```jet
if ready run() else wait()
```

### Multiline body

```jet
if ready {
    prepare()
    run()
} else {
    explain_wait()
    wait()
}
```

No arrow means that branch values are discarded.
A must-use value in an effect branch remains an error.

### Nested effect control

Use braces when direct nesting needs clarity:

```jet
if connected {
    if authenticated serve()
}
```

The parser must reject a direct adjacent nested `if` without grouping.

## Value `if`

Use `->` only when branches produce the `if` result:

```jet
state :: if ready -> .Ready else -> .Waiting
```

Multiline returned branch:

```jet
state :: if ready -> {
    record :: inspect()
    record.state
} else -> {
    explain_wait()
    .Waiting
}
```

A value `if` must:

- have an `else`;
- or exhaust a closed subject;
- produce one unified non-`Void` type.

Jet does not create an implicit optional for a missing branch.

### Preferred multi-branch form

```jet
grade :: if {
    score >= 90 -> .A
    score >= 80 -> .B
    score >= 70 -> .C
    else -> .F
}
```

Jet prefers one ordered arm-table model. Name a subject when that improves
clarity, or omit it when it does not. A head may be a value or structural
pattern against the subject, or any Boolean expression evaluated as written;
the same table may mix unrelated expressions. The first matching or true head
wins. Chained `else if` remains legal, but there should rarely be a reason to
prefer it and it is not a canonical teaching form. Each arrow means “this
selected arm yields this value.”

## Pattern dispatch

Pattern arms keep `->` in both statement and value contexts:

```jet
if status == {
    .Ready -> start()
    .Waiting(reason) -> wait(reason)
    .Failed(error) -> fail(error)
}
```

Value dispatch:

```jet
label :: if {
    status == .Ready -> "ready"
    status == .Waiting(reason) -> reason
    status == .Failed(_) -> "failed"
}
```

Arrow remains because each pattern selects one arm.
The arm result may be `Void`.

Multiline arm:

```jet
if status == {
    .Ready -> {
        prepare()
        start()
    }
    else -> report(status)
}
```

## Arm tables without a named subject

Omitting a subject does not select a lesser mechanism. It keeps the same
ordered arm-table model, with each head evaluated as a Boolean expression:

```jet
label :: if {
    score >= 90 -> "excellent"
    score >= 70 -> "good"
    else -> "retry"
}
```

Statement table:

```jet
if {
    unavailable -> report()
    stale -> refresh()
    else -> run()
}
```

Simple effect guards use no arrow:

```jet
if unavailable report()
```

## `comptime if`

Use the same split.

Effect selection:

```jet
comptime if debug enable_checks()
```

Multiline effect selection:

```jet
comptime if debug {
    register_debug_views()
    enable_checks()
}
```

Value selection:

```jet
backend :: comptime if build.os == {
    .Linux -> LinuxBackend.{}
    .MacOS -> MacOSBackend.{}
    .Windows -> WindowsBackend.{}
}
```

`comptime` changes evaluation time only.

## Effect loops

Effect loops use no arrow.

### Existing headers

```jet
loop tick()

loop ready poll()

loop item; items audit(item)

loop key, value; scores print("{key}: {value}")

loop i; 0..<limit; 2 inspect(i)

loop i := 0; i < limit; i++ inspect(i)
```

### Multiline bodies

```jet
loop item; items {
    audit(item)
    notify(item)
}
```

Braces group multiple statements. They do not mark return behavior.

### Discard law

An effect loop returns `Void`.
Its body result is discarded.

A must-use body value produces an error:

```jet
// Error if inspect returns a must-use Report.
loop item; items inspect(item)
```

The user must consume or deliberately discard that result.

## Collecting loops

Only finite loops accept a yield arrow.

```jet
names :: loop user; users -> user.name

squares :: loop i := 0; i < limit; i++ -> i * i
```

`->` now has one loop-specific meaning: each accepted iteration yields one
value.

The yielding body must produce a non-`Void` value.
The loop returns an eager list.

```jet
// Reject: audit returns Void.
reports :: loop user; users -> audit(user)
```

The fix removes `->`:

```jet
loop user; users audit(user)
```

### Multiline yielding body

```jet
labels :: loop user; users -> {
    name :: user.name.trim()
    if user.admin -> "admin:{name}" else -> name
}
```

### Filters

`next` omits the current item:

```jet
names :: loop user; users -> {
    if !user.active next
    user.name
}
```

A header guard remains shorthand:

```jet
names :: loop user; users if user.active -> user.name
```

### Several sources

```jet
rows :: loop team; teams,
             user; team.users if user.active
-> Row.{
    team: team.name,
    user: user.name,
}
```

Source clauses nest left to right.
One header yields one flat list.

An inner collecting loop preserves nesting:

```jet
groups :: loop team; teams ->
    loop user; team.users -> user.name
```

Lockstep stays explicit:

```jet
pairs :: loop pair; lefts.zip(rights) -> pair
```

### Other collectors

The collecting loop returns a list.
Use collection APIs for other shapes:

```jet
by_id :: (loop user; users -> (user.id, user)).to_map()

unique_names :: (loop user; users -> user.name).to_set()

lazy_names :: users.map(user => user.name)
```

This avoids collector keywords and expected-type-dependent timing.

## Bare loops

A bare loop is always effect control:

```jet
loop tick()

loop {
    poll()
    update()
}
```

A bare loop cannot use `->` because it has no exhaustion edge.

Return a value with `break value`:

```jet
connection :: loop {
    attempt :: connect(server)
    if !attempt.ok next
    break attempt
}
```

This rule avoids type-directed changes to repetition.

## Loop names and exits

Keep current loop declarations:

```jet
outer :: loop row; rows {
    ...
}
```

Replace dot exits:

```jet
break(outer)
next(outer)
break(outer, value)
```

With target arguments:

```jet
break(outer)
next(outer)
break(outer, value)
```

Complete family:

```jet
break
break value

break(outer)
break(outer, value)

next
next(outer)
```

Examples:

```jet
outer :: loop row; rows {
    loop cell; row {
        if cell.bad break(outer)
    }
}
```

Returned search:

```jet
found :: loop {
    loop row; rows {
        loop cell; row {
            if wanted(cell) break(found, cell)
        }
    }

    next
}
```

Inside the loop, `found` names the control target.
After the loop, `found` names the returned value.

Do not add a second result label.

### Collecting-loop exits

For a yielding finite loop:

- `break` returns the partial list;
- `break(name)` returns the named loop's partial list;
- `next` omits the current item;
- `next(name)` omits the named loop's current item;
- `break value` is rejected because the loop result is already `[T]`.

For a bare loop:

- `break` returns `Void`;
- `break value` returns the value;
- `break(name, value)` returns from a named outer loop;
- all value exits must unify.

## Task groups

Keep the current `taskgroup` form in this proposal.
The keyword-versus-type question remains separate.

Task bodies define callables, so use `=>`:

```jet
taskgroup group {
    user :: group.task => fetch_user(id)
    billing :: group.task => fetch_billing(id)
    profile :: group.all([user, billing])
}
```

Multiline:

```jet
taskgroup group {
    user :: group.task => {
        record :: fetch_user(id)
        validate(record)?
        record
    }
}
```

`group.task => body` defines a zero-parameter child callable.
It replaces `group.task(() => body)` without changing ownership.

General task spawning remains an ordinary lambda:

```jet
task :: tasks.spawn(() => process(file))
```

Copyable captures become owned task copies.
Non-copyable captures move as required.
No capture list or preparatory binding appears.

## Error flow

Keep:

```jet
record :: load()?

record :: maybe_load() ?? return fallback

item :: iterator.next() ?? next

return value
break
break value
break(outer, value)
next
next(outer)
```

Do not add either arrow to exit payloads.

```jet
// Reject.
return => value
break -> value
```

Exit keywords already state the transfer.

## Conversions

Conversions define callable mappings, so move them to `=>`:

```jet
impl SourceError => ApiError {
    ApiError.Source(self)
}
```

Migration type changes also use `=>`:

```jet
change score: Int => Rank via {
    value => Rank.{ value }
}
```

This groups:

- functions;
- lambdas;
- function types;
- computed fields;
- conversions;
- migration converters.

All define how inputs produce a result.

## Protocol cleanup

Remove transport arrows:

```jet
// Retire.
client -> server: Pay(amount: Int)
server -> client: Receipt(id: String)
```

For a two-endpoint protocol, the sender determines the receiver:

```jet
protocol Payment {
    client: Pay(amount: Int)
    server: Receipt(id: String)
}
```

`client:` means the client sends.
`server:` means the server sends.

If Jet later adds several endpoints, it needs a separate route form.
It must not reuse either result arrow.

## Scope blocks

Keep policy and lifetime scopes arrow-free:

```jet
#Unsafe("reason") {
    ...
}

#Transact(tx) {
    ...
}

#Shield {
    ...
}

#Context(deadline: limit) {
    ...
}

taskgroup group {
    ...
}
```

These heads set scope policy.
Braces provide audit and lifetime visibility.

`defer close(^resource)` stays unchanged.

## Formatting

### One-line effect control

```jet
if ready run()

loop item; items audit(item)
```

### Multiline effect control

```jet
if ready {
    prepare()
    run()
}

loop item; items {
    audit(item)
    notify(item)
}
```

### One-line value control

```jet
label :: if ready -> "ready" else -> "waiting"

names :: loop user; users -> user.name
```

### Multiline value control

```jet
label :: if ready -> {
    state :: inspect()
    state.label
} else -> {
    explain_wait()
    "waiting"
}
```

### Long loop header

```jet
rows :: loop team; teams,
             user; team.users if user.active
-> Row.{ team: team.name, user: user.name }
```

### Callable contracts

```jet
fn parse(text: String) => Number ? ParseError

fn fetch(url: String) =[Net, Log]=> Response ? HTTPError
```

Do not align effect arrows with added spaces.

## Grammar sketch

```text
callable-arrow = "=>" | "=[" effects "]=>"

function = "fn" head [ callable-arrow type ] function-body

function-body = block | "=" expression

lambda = lambda-head "=>" result-body

effect-if = "if" condition effect-body
            [ "else" effect-body ]

value-if = "if" condition "->" result-body
           "else" ( value-if | "->" result-body )

dispatch-arm = pattern "->" result-body

effect-loop = loop-header effect-body

yield-loop = finite-loop-header "->" result-body

effect-body = statement | block

result-body = expression | block

loop-exit = "break" [ expression ]
          | "break" "(" loop-name [ "," expression ] ")"
          | "next"
          | "next" "(" loop-name ")"
```

`->` is not a general infix operator.
`=>` is not a general infix operator.

The parser accepts each only in its listed grammar.

Parentheses directly after `break` always select a named loop.
An innermost payload stays `break value`.

## Parser boundary for adjacent effect bodies

The no-arrow one-line form needs a strict parse rule.

```jet
if condition statement

loop header statement
```

The parser:

1. parses one complete condition or loop source;
2. stops when the next token cannot continue that expression;
3. parses exactly one non-`if` statement as the effect body;
4. requires braces for a nested control statement.

Examples:

```jet
if ready run()

if count > 0 consume(count)

loop item; source() consume(item)

loop i := 0; i < limit; i++ draw(i)
```

Risk: error recovery is harder than a delimiter-based grammar.
The parser must give a direct fix when it cannot find a unique split.

If this grammar proves fragile, the fallback is one-line braces:

```jet
if ready { run() }

loop item; items { audit(item) }
```

Do not add a third delimiter only to solve that parse issue.

## Type law

### Effect control

No-arrow `if` and `loop` return `Void`.
Body values are discarded.
Must-use values still require explicit handling.

### Value `if`

Arrow branches must:

- cover all outcomes;
- return one non-`Void` type;
- unify ownership and view provenance.

### Pattern dispatch

Pattern arrows select arms.
Arm results can be `Void` or a unified value type.

### Collecting loops

A finite loop with `->`:

- requires a non-`Void` body result;
- yields one value per accepted iteration;
- skips on `next`;
- returns an eager `[T]`;
- returns the partial list on `break`.

### Bare loops

A bare loop:

- never accepts `->`;
- repeats on normal body completion;
- returns through `break value`;
- retries through `next`.

### Callables

`=>` or `=[Effects]=>` declares the returned type.
The body tail and every early `return` must match it.

## Diagnostics

Required teaching fixes:

- `fn f() -> T` → use `fn f() => T`;
- `fn f() --[E]-> T` → use `fn f() =[E]=> T`;
- `fn f() --[]-> T` → use `fn f() =[]=> T`;
- `fn(T) -> R` → use `fn(T) => R`;
- `impl A -> B` → use `impl A => B`;
- `if cond -> effect()` → remove `->` when the result is `Void`;
- `loop source -> effect()` → remove `->` when the result is `Void`;
- a non-`Void` no-arrow body → consume or discard its must-use result;
- a value `if` without `else` → add `else`;
- `->` on a bare loop → use `break value`;
- `outer.break(value)` → use `break(outer, value)`;
- `outer.next()` → use `next(outer)`;
- `client -> server:` → use `client:`;
- stale `take(...)` capture syntax → rely on current capture ownership and
  let copyable captures copy at closure creation.

## Migration

### Callable rewrite

```text
fn f(...) -> T             → fn f(...) => T
fn f(...) --[E]-> T        → fn f(...) =[E]=> T
fn f(...) --[]-> T         → fn f(...) =[]=> T
fn(T) -> R                  → fn(T) => R
impl A -> B                 → impl A => B
change f: A -> B            → change f: A => B
```

Lambdas and computed fields already use `=>`.

### Control rewrite

```text
if cond -> effect           → if cond effect
if cond -> { effects }      → if cond { effects }
if cond { effects }         → unchanged
loop header { effects }     → unchanged
loop header -> value        → collecting loop
```

Current block-form effect control remains close to source-compatible.

### Exit rewrite

```text
outer.break()               → break(outer)
outer.break(value)          → break(outer, value)
outer.next()                → next(outer)
```

### Protocol rewrite

```text
client -> server: Msg       → client: Msg
server -> client: Msg       → server: Msg
```

### Capture cleanup

Remove stale explicit capture-list examples.
Do not add a replacement syntax in this proposal.
Use current implicit capture ownership and capture-time copies.

## Benefits

- `=>` owns callable definitions.
- Effect rows fit inside the callable arrow.
- `->` owns selected and yielded values.
- Effect control has no arrow noise.
- Existing multiline `if` and `loop` blocks stay clean.
- One-line effect control becomes minimal.
- Collecting loops are visible before type inference.
- A `Void` body cannot silently become a collection.
- Named exits keep the control keyword first.
- Protocol direction loses unrelated arrow use.
- Capture syntax stays outside this decision.

## Costs

- Every function result signature changes.
- Every function type changes.
- Every explicit effect row changes.
- Error conversions and migrations change.
- `=[Effects]=>` is a dense glyph cluster.
- Adjacent one-line effect bodies need careful parsing and diagnostics.
- `break(name, value)` gives named and unnamed exits different payload shapes.
- Collecting loops still need substantial sema, TIR, JIT, and codegen work.
- Protocol migration assumes two endpoints.

## Risks

### Dense effect arrow

```jet
=[Net, DB.Write, Log]=>
```

This is visually heavier than `--[Effects]->`.
Its benefit is categorical separation from control flow.

The formatter must not add spaces inside the token frame.

### Adjacent body parsing

```jet
if condition action()
```

This is clean when valid. Parse errors can become harder to explain.
The grammar must reject ambiguous splits instead of guessing.

### Arrow meaning

Pattern arrows can return `Void`:

```jet
.Ready -> start()
```

Therefore, `->` means selected arm or yielded item, not strictly non-`Void`
return.

### Function migration size

Function signatures dominate Jet source.
This proposal causes more churn than the prior control-only change.

The migration is mechanical and removes category overlap.

## Canonical package

```jet
fn classify(score: Int) => Grade =
    if score == {
        90..100 -> .A
        80..89 -> .B
        else -> .C
    }

fn active_names(users: [User]) => [String] {
    loop user; users if user.active -> user.name
}

fn notify_active(users: [User]) =[Net]=> Void {
    loop user; users {
        if !user.active next
        notify(user)
    }
}

fn connect_until_ready(server: Server) =[Net]=> Connection {
    connection :: loop {
        attempt :: connect(server)
        if !attempt.ok next
        break attempt.value
    }

    connection
}

fn search(rows: [[Cell]]) => Cell? {
    found :: loop {
        loop row; rows {
            loop cell; row {
                if wanted(cell) break(found, Val(cell))
            }
        }

        break None
    }

    found
}

taskgroup group {
    user :: group.task => fetch_user(id)
    billing :: group.task => fetch_billing(id)
    profile :: group.all([user, billing])
}

protocol Payment {
    client: Pay(amount: Int)
    server: Receipt(id: String)
}
```

## Recommendation

Adopt this split as the new research baseline:

1. `=>` defines callables.
2. `=[Effects]=>` defines effectful callables.
3. `->` selects or yields.
4. No arrow means effect control.
5. Braces group multiline bodies.
6. Named exits use `break(name)`, `break(name, value)`, and `next(name)`.
7. Protocol lines name the sender.
8. Captures stay implicit: Copy values copy at closure creation; owned
   non-Copy values move.

Test adjacent one-line effect parsing before balloting the whole package.
If parsing is not robust, keep one-line braces instead of adding another token.
