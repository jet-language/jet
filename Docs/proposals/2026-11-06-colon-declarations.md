# Every declaration is `name: value` (2026-11-06)

This proposal backs ballot **D-DECL-COLON1**. The owner asked on 2026-11-06
for `name: value` in place of `name :: value`, for functions, modules and
constants alike. His reasons: fewer characters and a feel closer to Python;
Jet becomes a natural configuration language, like Lua or Nix; and every
declaration reads the same way, fields included, since a field is a constant
each instance holds.

Counts were measured on dev `b486d6c09a` over every `.jet` file (18,352 files,
1,254,763 code lines) with
`~/.cache/jet-dev/scratch/Main/ballots/census-marks.js`. The example files
show proposed syntax that the compiler does not accept yet. They also use the
two companion proposals in
[`2026-11-06-memory-marks-in-signatures.md`](2026-11-06-memory-marks-in-signatures.md)
only where section 6 says so.

## 1. The rule

> Everything that introduces a name is written `name: value`. A value you
> may change later is written `name := value`.

| Today | Proposed |
|---|---|
| `limit :: 100` | `limit: 100` |
| `count := 0` | `count := 0` (unchanged) |
| `fn area(r: Float) -> Float { … }` | `area: fn(r: Float) -> Float { … }` |
| `struct Point { x: Int, y: Int }` | `Point: struct { x: Int, y: Int }` |
| `enum Mode { Fast, Safe }` | `Mode: enum { Fast, Safe }` |
| `trait Shape { fn area(self) -> Float }` | `Shape: trait { area: fn(self) -> Float }` |
| `module math { … }` | `math: module { … }` |
| `module int_cache :: cache<Int>(32)` | `int_cache: cache<Int>(32)` |
| `Meters :: Int` (alias) | `Meters: Int` |
| `use process :: core.process` | `use process: core.process` |
| `package { name: "x" }` | `package: { name: "x" }` |

Kind parameters go on the kind word, never on the name: `first: fn<T>(xs:
[T]) -> T?`, `Box: struct<T> { value: T }`, `cache: module<K>(capacity:
Int)`. The name stays a plain name everywhere it is declared and used.

Two kinds of body decide how to read the right side:

- **A shape body** describes what each value holds: the braces after
  `struct`, `trait`, `enum`, and a foreign `#Import module`. There `name:
  Type` gives each instance a field of that type, or each implementor a
  method it must provide.
- **Every other body** is a value body: a file, a `module`, a function, an
  `impl`. There `name: thing` makes `name` that thing.

The word before the brace always says which body you are in.

## 2. Why

**One shape for every declaration.** `package.jet` already writes `name:
value` (`name: "compiler"`, `members: [...]`, `test: { quick: … }`), and so do
struct literals (`Point{x: 1}`), named arguments (`reruns: 100`) and field
declarations (`x: Int`). Source files are the one place that writes `::`. The
proposal makes a package file, a config file and a program the same language.

**`::` has no job left.** Jet took `::` from Jai, where `x : Type : value`
gives a name, a type and a value, and `x :: value` is that form with the type
left out. D-BIND-BARE1=A retired typed bindings, so the type slot between the
colons is gone and only the doubled colon remains.

**Fewer characters where it counts.** Bindings are the most common
declaration: 141,892 `::` lines and 49,916 `:=` lines. Each `::` binding
loses two characters. A function or type header gains one (`fn run()` is 8,
`run: fn()` is 9), across 71,818 functions and 11,324 types.

**It removes a second way to write a function.** Today `fn name(…) { … }`
and `name :: fn(…) { … }` both declare a function (891 uses of the second).
Under the proposal the two are one form.

**A configuration language for free.** A file of entries is a valid module,
read by any program with `use`. It needs no new format, and it can compute
values with ordinary functions, as Nix and Lua can (example file 4).

## 3. Downsides, and how each is designed away

1. **Readers from TypeScript, Python, Rust or Kotlin read `x: T` as "x has
   type T".** In Jet, a declaration never carries a separate type, so `name:
   thing` always declares. Where `thing` is a type (a field, an alias), the
   familiar reading is already right. Where it is a value (`limit: 100`), the
   familiar reading is impossible, since `100` is not a type, so nobody can
   misread it silently. JavaScript objects, JSON, YAML, CUE and Jsonnet all
   read `name: value` as Jet would.
2. **`{ a: 1 }` could be a block or a record.** A brace group whose entries
   are all `name: value` is a record literal. A block needs at least one
   statement that is not a binding, and every useful block has one: a block
   that only binds names produces nothing, so no meaningful program changes.
3. **`x: Int` at file level makes an alias, not a variable of type Int.**
   That is already true of `x :: Int` today. Using the alias as a number gets
   a registered error with the fix `x: Int{0}`.
4. **A foreign function has no body, like a function type.** Inside a shape
   body (`#Import module c.z { … }`) every entry is a signature, and its
   symbol follows as today: `gzclose: fn(file: ^GzFile) -> Int = "gzclose"`.
   At file level a bodiless `fn(…)` is a function type, as it is today on the
   right of `::`.
5. **"Find the definition of `area`" changes from `fn area` to `area:`.**
   Both are one search. Editor outlines and go-to-definition read the
   parser, not the text, so they do not change.
6. **A function or type header is one character longer.** The two-character
   saving on 141,892 bindings outweighs one extra character on 83,142
   headers, and every header lines up its names at the left margin, where
   the eye looks first.
7. **A large cutover.** About 225,000 declaration lines change. `jet fix`
   rewrites them from the parse tree in one commit, with the grammars,
   syntax registry, spec, examples, snapshots and editor highlighting, as the
   greenfield rule requires. Nothing else changes meaning, so the run-time
   tests prove the rewrite.

## 4. Options on the ballot

- **A. Every declaration** (recommended): the table above.
- **B. Bindings only:** `limit: 100` and `int_cache: cache<Int>(32)`, but
  `fn`, `struct`, `enum`, `trait` and `module` keep their keyword-first form.
  Smaller cutover, but functions and types still read differently from
  everything else, and the two function forms stay.
- **C. Keep `::`.**

## 5. Example files

Each file is shown first as it is in the repository today, then in full under
option A. These use today's call marks, so only the declaration form changes.

### Example 1: `Examples/features/functions/never_return.jet`

Today:

```jet
package {
    name: "functions_never_return"
    version: "0.1.0"
    authority: { allow: [Exec, IO, Mem.Alloc] }
}

use process :: core.process

#Error
enum StopError {
    Closed
}

fn usage() -> Never {
    print("usage: tool <count>")
    process.exit(2)
}

fn count(arg: String) -> Int { return Int.parse(arg) ?? usage() }

fn serve(addr: String) -> Never NetError! {
    loop { panic("serve stopped") }
}

fn propagate(error: NetError) -> Never NetError! { return Err(error) }

fn stop() -> Never { panic("stop") }

fn run_then_stop(stop: fn() -> Never) -> Never { stop() }

fn fail() -> Never StopError! { return Err(StopError.Closed) }

fn halt(code: Int) -> Never Never! { process.exit(code) }

fn halt_usage() -> Never { halt(3) }

trait Terminator {
    fn stop(self) -> Never
}

struct Worker {

}

impl Worker.Terminator {
    fn stop(self) -> Never { panic("worker stopped") }
}

fn run() {
    print(count("41"))
    print(fail() ?? 42)
}
```

Proposed:

```jet
package: {
    name: "functions_never_return"
    version: "0.1.0"
    authority: { allow: [Exec, IO, Mem.Alloc] }
}

use process: core.process

#Error
StopError: enum {
    Closed
}

usage: fn() -> Never {
    print("usage: tool <count>")
    process.exit(2)
}

count: fn(arg: String) -> Int { return Int.parse(arg) ?? usage() }

serve: fn(addr: String) -> Never NetError! {
    loop { panic("serve stopped") }
}

propagate: fn(error: NetError) -> Never NetError! { return Err(error) }

stop: fn() -> Never { panic("stop") }

run_then_stop: fn(stop: fn() -> Never) -> Never { stop() }

fail: fn() -> Never StopError! { return Err(StopError.Closed) }

halt: fn(code: Int) -> Never Never! { process.exit(code) }

halt_usage: fn() -> Never { halt(3) }

Terminator: trait {
    stop: fn(self) -> Never
}

Worker: struct {}

impl Worker.Terminator {
    stop: fn(self) -> Never { panic("worker stopped") }
}

run: fn() {
    print(count("41"))
    print(fail() ?? 42)
}
```

### Example 2: `Examples/features/modules/generic_modules.jet`

Proposed (today's file is in the repository):

```jet
cache: module<K>(capacity: Int) {
    BASE: comptime { capacity }

    COMPUTED_SIZE: comptime { BASE + 1 }

    Entry: struct {
        priv key: K
        priv limit: Int -> capacity
    }

    Slot: enum {
        Empty
        Value(K)
    }

    entry: fn(k: K) -> Entry { return Entry{key: k} }

    slot: fn(k: K) -> Slot { Slot.Value(k) }

    key_of: fn(k: K) -> String { return "key" }

    describe: fn() -> String { "generic cache" }

    size: fn() -> Int { COMPUTED_SIZE }
}

// Forward alias chain: declaration order does not affect resolution.
forward_cache: int_cache

int_cache: cache<Int>(32)

Mode: enum {
    Fast
    Safe
}

tier_zero: module(flag: Bool, count: Int, letter: Char, label: String, mode: Mode) {
    #Meta(category: "tier zero")
    show: fn() {
        print(flag)
        print(count)
        print(letter)
        print(label)
        print(mode == Mode.Fast)
    }
}

values: tier_zero(true, 7, 'J', "jet", Mode.Fast)

Token: trait {}

GoodToken: struct {}

impl GoodToken.Token {}

annotation_kinds: module<Item: Token>(count: Int) {
    count: fn() -> Int { count }
}

kinds: annotation_kinds<GoodToken>(8)

fixed_buffer: module<T>(capacity: Int) {
    Buffer: struct {
        priv items: [T#capacity]
    }

    echo: fn(values: [T#capacity]) -> [T#capacity] { return values.copy() }
}

three_ints: fixed_buffer<Int>(3)

local_laws: module<T> {
    Reveal: trait {
        Output: type
        reveal: fn(self) -> T
    }

    Wrapped: struct {
        priv value: T
    }

    impl Wrapped.Reveal {
        Output: T

        reveal: fn(self) -> T { self.value }
    }

    SourceErr: enum {
        Bad(T)
    }

    TargetErr: enum {
        Wrapped(SourceErr)
    }

    impl SourceErr -> TargetErr { return TargetErr.Wrapped(self) }
}

int_laws: local_laws<Int>

run: fn() {
    k: int_cache.key_of(42)
    d: int_cache.describe()
    n: int_cache.size()
    e: int_cache.entry(42)
    s: int_cache.slot(7)
    print(k)
    print(d)
    print(n)
    print(e.key)
    print(e.limit)
    forwarded: forward_cache.entry(9)
    print(forward_cache.size())
    print(forwarded.key)
    if s == {
        .Value(v) -> { print(v) }
        .Empty -> { print(0) }
    }

    values.show()
    print(kinds.count())
    input: [Int#3]{1, 2, 3}
    fixed: three_ints.echo(input)
    buffer: three_ints.Buffer{items: fixed}
    print(buffer.items[2])
    wrapped: int_laws.Wrapped{value: 91}
    print(wrapped.reveal())
}
```

The associated type reads the same way: a trait lists `Output: type`, and
the impl gives it, `Output: T`, where today they write `type Output` and
`type Output = T`.

### Example 3: a small program, new in this proposal

```jet
use files: core.files

MAX_ITEMS: 100
DEFAULT_NAME: "untitled"

Priority: enum {
    Low
    High
}

Task: struct {
    title: String
    priority: Priority
    done: Bool
}

Inbox: struct {
    tasks: [Task]
}

impl Inbox {
    add: fn(&self, title: String, priority: Priority) {
        if self.tasks.len() < MAX_ITEMS {
            &self.tasks.push(Task{title: title, priority: priority, done: false})
        }
    }

    open: fn(self) -> Int {
        left := 0
        loop task in self.tasks {
            if !task.done -> left += 1
        }
        left
    }
}

summary: fn(inbox: Inbox) -> String {
    "{inbox.open()} open of {inbox.tasks.len()}"
}

run: fn() {
    inbox := Inbox{tasks: [Task]{}}
    &inbox.add("write proposal", Priority.High)
    &inbox.add(DEFAULT_NAME, Priority.Low)
    report: summary(inbox)
    print(report)
}
```

### Example 4: a configuration file

`deploy.jet` is plain entries, which the deploy tool reads with an ordinary
`use`. Unlike JSON or YAML it is checked against real types, and
unlike a template language it computes with ordinary functions.

```jet
use shop: shop.deploy

version: "2.4.1"
region: "eu-west-1"
replicas: 3
image: "registry.example.com/shop:{version}"

limits: shop.Limits{ memory_mb: 512, cpu: 0.5 }

services: [
    shop.Service{ name: "web", port: 8080, public: true },
    shop.Service{ name: "worker", port: 9090, public: false },
]

regions: ["eu-west-1", "us-east-1"]

endpoint: fn(region: String) -> String { "https://{region}.shop.example.com" }

endpoints: regions.map(endpoint)
```

The same file in Nix and Lua, for comparison:

```nix
{
  version = "2.4.1";
  replicas = 3;
  image = "registry.example.com/shop:${version}";
  endpoint = region: "https://${region}.shop.example.com";
}
```

```lua
version = "2.4.1"
replicas = 3
image = "registry.example.com/shop:" .. version
function endpoint(region) return "https://" .. region .. ".shop.example.com" end
```

### Example 5: package files do not change

`Compiler/package.jet` is already written this way, so the proposal turns it
from a special dialect into ordinary Jet:

```jet
name: "compiler"
version: "0.0.1"
edition: "2026"
members: ["JetFoundation", "JetLexer", "JetParser", "JetSema"]
test: {
    quick: TestProfile{ suites: [tests] },
    ui: TestProfile{ suites: [ui, ui_lint], baseline: "../Tools/testkit/known-failing.tsv" },
}
```

## 6. All three proposals together

With D-CALLSITE-MARK2=A and D-MOVE-SIGIL1=A as well, Example 3's methods and
calls lose their marks, and `Examples/features/memory/receiver_marks.jet`
reads:

```jet
Buffer: struct { text: String }

impl Buffer {
    append: fn(&self, chunk: String) {
        self.text = "{self.text}{chunk}"
    }
    grow: fn(&self) -> Buffer {
        self.text = "{self.text}!"
        return Buffer{text: self.text.copy()}
    }
    clear: fn(&self) {
        self = Buffer{text: "cleared"}
    }
    len: fn(self) -> Int {
        return self.text.len()
    }
    seal: fn(&&self) {
        print(self.text)
    }
}

make_buffer: fn() -> Buffer {
    return Buffer{text: "fresh"}
}

inspect: fn(buffer: Buffer) {
    print("inspect {buffer.text}")
}

edit: fn(buffer: &Buffer) {
    buffer.append(" edited")
}

run: fn() {
    buf := Buffer{text: "hello"}
    buf.append(" world")
    print(buf.len())
    inspect(buf.grow())
    edit(buf)
    items := [Buffer{text: "a"}, Buffer{text: "b"}]
    items[1].append("!")
    print(items[1].text)
    items[0].clear()
    print(items[0].text)
    print(make_buffer().len())
    values := [3, 1, 2]
    values.push(values.len())
    print(values)
    buf.seal()
}
```
