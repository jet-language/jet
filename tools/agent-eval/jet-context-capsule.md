# Jet cold-context capsule

Capsule format: UTF-8 Markdown. This file is the complete context given to a cold
agent. It is intentionally self-contained: the task prompt adds only the task
contract. It contains no generated answers, credentials, or model-specific advice.
It may name repository-owned canonical suites for examples. Release tooling can
ship this file as an artifact named `jet-context-capsule`.

## 1. Program shape

A Jet executable has a `fn run` entry point. A statement ends at a newline; use
braces for a multi-statement body. `::` creates an immutable binding, `:=`
creates a mutable binding, and `=` reassigns a mutable binding. `->` introduces
a concise body or a closure. The primitive types are `Int`, `Float`, `Bool`,
`String`, and `Char`; lists are `[T]`; optional values are `?T`; a fallible
value is `T ! E`.

```jet
fn run() {
    message :: "Hello, Jet"
    print(message)
}
```

Functions name parameter and return types. A block function can return with
`return`; a concise function uses `->`.

```jet
fn twice(n: Int) Int -> n * 2

fn run() {
    print(twice(21))
}
```

Use `if condition -> expression` for a one-line branch, or braces for a block;
`else` is attached to the same conditional. `loop condition { ... }` repeats
until the condition is false. `loop item in collection { ... }` iterates values;
`break` exits and `next` skips an iteration.

```jet
fn run() {
    total := 0
    loop i in 1..5 -> total += i
    if total == 10 -> print("sum={total}") else -> print("wrong")
}
```

Lists and maps are constructed with typed literals. Indexes are zero-based.
Methods and fields use the dot rule: put the value on the left of `.`, and put
an operation or field after it. Module functions also use a dotted alias.

```jet
fn run() {
    values := [Int]{3, 1, 2}
    values.sort()
    print(values[0])
}
```

Use `use core.module as alias` at the top of a file. Do not invent a module
alias or a `::` method call. A record is declared with `struct Name { field:
Type }` and constructed with `Name{field: value}`.

## 2. Dot rule and common calls

The left side of a dot is always the receiver or module alias:

```jet
use core.files as files
use core.process as process

fn run() {
    path :: process.argv().get(1) ?? panic("missing path")
    text :: files.read(path) ?? panic("read failed")
    print(text.trim())
}
```

`value.len()`, `value.get(index)`, `value.push(item)`, `value.sort()`,
`value.trim()`, `value.to_lower()`, `value.lines()`, and
`value.contains(needle)` are receiver calls. `value.filter(f)` and
`value.map(f)` transform a list; `value.is_empty()` tests a list or string.
A call that can fail returns an outcome; handle it instead of ignoring it.

For line transforms, use `text.lines()` and keep the result in a mutable list:
`lines := text.lines().filter(line -> !line.trim().is_empty()).map(line ->
line.trim().to_lower())`. Then call `lines.sort()` and loop over the list.

## 3. Memory and ownership verbs

- A bare parameter type `T` is read. `&T` gives exclusive write access, and
  `^T` takes ownership. The sigil is on the type: `fn edit(x: &T)` and
  `fn consume(x: ^T)`.
- Call sites mirror access: `edit(&value)` writes `value`; `consume(^value)`
  moves `value` into the callee. Plain `use(value)` reads without moving it.
- `~value` makes an independent copy. It is not a move marker and is useful
  when a copy must enter a stored slot.
- `shared Struct{...}` creates shared mutable state. A shared struct field can
  be read or assigned directly; `guard_read()` and `guard_edit()` keep a guard
  across calls when needed.
- A borrowed view must not outlive its source. Use `~value` or an owned value
  when data must enter storage or survive the source.

## 4. Effects and outcomes

A fallible return type uses `T !(E1 | E2)`; a unit-fallible return uses `!E`.
The ordinary `T` return uses Jet's implicit error route. An effect arrow declares
the allowed effects: `fn load() String -[FS, IO]> { ... }`. Pure code can use
`-[]>`. Common effects include `FS`, `IO`, `Net`, `Exec`, `Env`, `Mem.Alloc`,
`Mem.Rc`, `Panic`, and `Task`.

- `Ok(value)` is success and `Err(error)` is failure.
- `Val(value)` is a present optional value.
- `None` is the absent optional value.
- `??` supplies a fallback or handles a fallible result with a recovery
  expression; use it only on an optional or fallible expression.
- `?` propagates a failure from the current function.
- Match outcomes with patterns such as `.Ok(value)`, `.Err(error)`, `.Val(value)`,
  and `.None`; a wildcard is `_`. A Boolean pattern is a literal `true` or
  `false`, not a binding.

```jet
fn lookup(flag: Bool) ?Int -> {
    if flag -> return Val(7)
    return None
}

fn run() {
    value :: lookup(true) ?? 0
    print(value)
}
```

Do not silently turn an error into success. State the recovery policy in the
program and preserve the original error when propagation is intended.

## 5. Forty common library verbs

These are the common operation names to reach for. Import the shown core module
with `use core.<module> as <alias>` and call `<alias>.<verb>(...)`.

1. `term.print`  2. `term.eprint`  3. `term.input`  4. `term.readline`
5. `term.read_all_input`  6. `files.read`  7. `files.write`
8. `files.read_bytes`  9. `files.write_bytes`  10. `files.exists`
11. `files.list_dir`  12. `files.walk`  13. `process.argv`  14. `process.run`
15. `process.cmd`  16. `sys.get`  17. `sys.set`  18. `json.parse`
19. `json.decode`  20. `json.to_string`  21. `csv.parse`  22. `csv.to_string`
23. `text.trim`  24. `text.lower`  25. `text.upper`  26. `text.splitn`
27. `regex.compile`  28. `regex.replace`  29. `math.abs`  30. `math.min`
31. `math.max`  32. `math.round`  33. `math.sqrt`  34. `time.now`
35. `time.sleep`  36. `net.tcp_listen`  37. `net.tcp_connect`  38. `http.get`
39. `http.post`  40. `http.serve`

Useful module aliases are `term` for `core.term`, `files` for `core.files`,
`process` for `core.process`, `sys` for `core.sys`, `json` for
`core.encoding.json`, `csv` for `core.encoding.csv`, `text` for `core.text`,
`regex` for `core.regex`, `math` for `core.math`, `time` for `core.time`, `net`
for `core.net`, and `http` for `core.http`.

HTTP services use `use core.http.server as server` and `use core.net as net`.
Create a router with `server.mux()`, register `mux.get(path, handler)`, and
return `Ok(server.response(status, body))` from the handler. A request route
parameter is `request.param("name")`. `net.tcp_listen(address)` plus
`server.serve_once_listener(listener, mux)` handles one request; repeat it in
a loop when the service must stay up.

## 6. Ten runnable canonical programs

Each block is a complete source file with a `fn run` entry point. Save one block
as `program.jet` and run it with the normal Jet launcher. Programs that read a
path or listen on a port receive that value as their first argument.

### 1. Hello

```jet
fn run() {
    print("Hello, Jet")
}
```

### 2. Arithmetic

```jet
fn run() {
    answer :: 6 * 7
    print(answer)
}
```

### 3. Function call

```jet
fn triple(n: Int) Int -> n * 3

fn run() {
    print(triple(14))
}
```

### 4. Branching

```jet
fn run() {
    score :: 8
    if score >= 5 -> print("pass") else -> print("retry")
}
```

### 5. Loop and mutation

```jet
fn run() {
    total := 0
    loop i in 1..5 -> total += i
    print(total)
}
```

### 6. List ordering

```jet
fn run() {
    values := [Int]{3, 1, 2}
    values.sort()
    print(values[0])
}
```

### 7. Optional fallback

```jet
fn maybe(flag: Bool) ?Int -> {
    if flag -> return Val(7)
    return None
}

fn run() {
    print(maybe(false) ?? 0)
}
```

### 8. Command-line argument

```jet
use core.process as process

fn run() {
    name :: process.argv().get(1) ?? "world"
    print("hello, {name}")
}
```

### 9. File input

```jet
use core.files as files
use core.process as process

fn run() {
    path :: process.argv().get(1) ?? panic("missing path")
    text :: files.read(path) ?? panic("read failed")
    print(text.trim())
}
```

### 10. HTTP health endpoint

```jet
use core.http.server as server
use core.net as net
use core.process as process

struct State {
    done: Bool
}

fn run() !(HTTPError | NetError | IOError) {
    port :: process.argv().get(1) ?? "18080"
    listener :: net.tcp_listen("127.0.0.1:{port}")
    state :: shared State{done: false}
    mux :: server.mux()
    mux.get("/health", (_req: HTTPRequest) -> Ok(server.response(200, "{{\"status\":\"ok\"}}").header("content-type", "application/json")))
    mux.get("/hello/:name", (req: HTTPRequest) -> {
        name :: req.param("name") ?? ""
        Ok(server.response(200, "Hello, {name}").header("content-type", "text/plain"))
    })
    mux.get("/shutdown", (_req: HTTPRequest) -> {
        state.done = true
        Ok(server.response(200, "{{\"bye\":true}}"))
    })
    loop {
        if state.done -> break
        server.serve_once_listener(listener, mux) ?? panic("serve failed")
    }
}
```

## 6.1 Repository idiom suites

When repository context is available, use these executable suites as the owning
examples instead of inventing a parallel teaching form:

- `examples/suites/dispatch.jet` — ordered dispatch tables and grouped aliases.
- `examples/suites/failure.jet` — implicit failure flow, typed expert contracts,
  and one conversion rail.
- `examples/suites/finite_state.jet` — enums, variant groups, and typestate
  transitions.
- `examples/suites/ownership.jet` — reused views, explicit `~` boundaries, and
  cost visibility.
- `examples/suites/wire_output.jet` — canonical JSON writer bytes and a
  `#Codable` round trip.

## 7. Cold-agent rules

Return one complete Jet source file and nothing else when a task asks for code.
Use only the APIs named in the task and this capsule. Do not assume repository
files, hidden helpers, network access, package downloads, or unstated input.
Compute output from the supplied input; do not hardcode a task's expected
answer. Keep stdout exact: no progress messages or Markdown fences in the
submitted file. The evaluator checks source compilation and then executes the
program against a fixed fixture. A failed compile or wrong observable output is
a failed case, not a partial success.
