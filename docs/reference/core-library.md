# Core library (`jet.core`)

The Jet Core library gives you files, terminal I/O, environment variables,
process control, math, time, random numbers, JSON, tasks, and channels —
enough to write real command-line tools. Every fallible call returns a
`T ? E` value; nothing in Core panics on its own.

**How it works today:** Core modules are built into the compiler. Use them by
name; the compiler type-checks your calls and generates only the helpers you
actually use (see [Using modules](#using-modules) and [Pay for what you call](#pay-for-what-you-call)).

**Canonical name:** `jet.core`. The short spelling `core` is reserved and means the
same thing.

**Naming (S54):** types and error enums are PascalCase (`String`, `IOError`,
`JSON`); functions and module segments are snake_case (`read`, `core.fs`).
See S66 for acronym capitalization.

---

## Quick start

```jet
use core.fs as fs
use core.io as io
use core.env as env

fn main() {
    args :: io.args()
    if args.len() < 2 {
        io.eprint("usage: greet <name>")
        return
    }
    name :: args.get(1) ?? return
    greeting :: env.get("GREETING") ?? "hello"
    fs.write("/tmp/greet.txt", "{greeting}, {name}!") ?? return
    print(fs.read("/tmp/greet.txt") ?? return)
}
```

Build and run (extra words after the file become program arguments):

```bash
nix develop -c jet run tool.jet World
# or: nix develop -c jet build tool.jet && ./build/tool World
```

---

## Using modules

Core modules use `use` — no quotes, unlike file imports.

```jet
use core.fs as fs                    // one submodule
use jet.core.encoding.json as json   // same module, canonical spelling
```

Both `use core.fs` and `use jet.core.encoding.json` resolve to the same
compiler-known module.

**Not allowed:**

```jet
import core.fs as fs        // teaching error E0015 — use `use core.fs`
use "std/fs"               // quoted paths are for .jet files only
```

If you name a local file or folder `core`, `jet`, `http`, `regex`, `csv`, `toml`,
`crypto`, or `archive`, the compiler rejects it — those names are reserved for
first-party packages.

---

## Errors and results

Fallible Core functions return `T ? E`. Handle them like any other Jet
result — with `?`, `??`, or a pattern test:

```jet
use core.fs as fs

fn main() {
    text :: fs.read("data.txt") ?? return   // stop on error
    upper :: text.to_upper()
    fs.write("out.txt", upper) ?? panic("couldn't save")  // bug if this fails
}
```

Each module has a small error type (`IOError`, `JSONError`, …). There is no
automatic conversion between error types in v1.

---

## Pay for what you call

Using `core.fs` costs nothing in the generated binary until you **call**
something from it. A program that uses every Core module but only calls
`print` stays hello-world sized. Only the helpers your program can reach are
compiled in.

---

## Modules

### `core.fs` — files and folders

Whole-file helpers only (streaming I/O added in E2-M7). Paths are plain
`String`s.

```jet
use core.fs as fs

fn main() {
    path :: "/tmp/notes.txt"
    fs.write(path, "hello\n") ?? return
    fs.append(path, "world\n") ?? return
    print(fs.read(path) ?? return)        // "hello\nworld\n"
    print(fs.exists(path))                // true
    print(fs.is_dir("/tmp"))              // true
    names :: fs.list_dir("/tmp") ?? return
    print(names.len())
}
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `read(path)` | `String ? IOError` | Read entire file as UTF-8 text |
| `read_bytes(path)` | `[U8] ? IOError` | Read entire file as bytes |
| `write(path, text)` | `() ? IOError` | Create or overwrite a text file |
| `append(path, text)` | `() ? IOError` | Append text to a file |
| `exists(path)` | `Bool` | Whether the path exists |
| `remove(path)` | `() ? IOError` | Delete a file |
| `list_dir(path)` | `[String] ? IOError` | Names in a directory |
| `create_dir(path)` | `() ? IOError` | Create a directory |
| `is_dir(path)` | `Bool` | Whether the path is a directory |
| `copy(from, to)` | `() ? IOError` | Copy a file |
| `rename(from, to)` | `() ? IOError` | Rename or move a file |

**`IOError`** — `NotFound(path)`, `PermissionDenied(path)`, or `Other(message)`.

---

### `core.io` — terminal and arguments

```jet
use core.io as io

fn main() {
    args :: io.args()                    // [String]; index 0 is the program name
    name :: io.input("your name? ") ?? return  // reads one line, strips newline
    print("hi, {name}")
    io.eprint("(log) done")                 // like print, but to stderr
}
```

Pipe input for scripts:

```bash
printf "Ada\n" | nix develop -c jet run ask.jet
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `args()` | `[String]` | Command-line arguments |
| `input([prompt])` | `String ? IOError` | Read one line from stdin; optional prompt |
| `read_all_input()` | `String ? IOError` | Read all of stdin to end-of-file |
| `eprint(value)` | nothing | Print to stderr (any printable value) |

`print` stays in the core prelude (no `use` needed). Use `io.eprint` for stderr.

---

### `core.env` — environment and working directory

```jet
use core.env as env

fn main() {
    home :: env.home_dir()               // String? — may be null
    mode :: env.get("MODE") ?? "dev"     // String? from the environment
    env.set("MODE", "prod")              // set for child processes
    here :: env.current_dir() ?? return  // current working directory
    print(home ?? "(no home)")
    print(mode)
    print(here)
}
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `get(name)` | `String?` | Environment variable, or null if unset |
| `set(name, value)` | nothing | Set an environment variable |
| `current_dir()` | `String ? IOError` | Current working directory |
| `home_dir()` | `String?` | User home directory, if known |

---

### `core.process` — exit and subprocesses

```jet
use core.process as process

fn main() {
    result :: process.run(["echo", "hi"]) ?? return
    print(result.code)       // exit code as Int
    print(result.output)     // stdout as String
    print(result.errors)     // stderr as String
    process.exit(0)          // end the program with an exit code (never returns)
}
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `exit(code)` | never | Stop the program with the given exit code |
| `run(cmd)` | `ProcessResult ? IOError` | Run a command; `cmd` is `[String]` |

**`ProcessResult`** — `code: Int`, `output: String`, `errors: String`.

---

### `core.math` — numbers

```jet
use core.math as math

fn main() {
    print(math.sqrt(2.0))
    print(math.pow(2.0, 10.0))
    print(math.abs(-3))                     // works on Int and Float
    print(math.min(3, 7))                   // generic over Comparable types
    print(math.max(3.5, 7.2))
    print(math.floor(3.9))
    print(math.ceil(3.1))
    print(math.round(3.6))                  // returns Int
    print(math.clamp(15, 0, 10))            // 10
    print(math.pi)
    print(math.e)
}
```

| Item | Notes |
|------|-------|
| `sqrt`, `pow`, `floor`, `ceil` | `Float` in, `Float` out |
| `round` | `Float` in, `Int` out |
| `abs` | `Int` or `Float` |
| `min[T]`, `max[T]` | Two values of the same comparable type |
| `clamp(x, lo, hi)` | Keep `x` inside the range |
| `pi`, `e` | Float constants |

---

### Linear algebra — `Vec2`/`Vec3`/`Vec4`, `Mat3`/`Mat4` (D-LINALG1)

Built-in value types — no import. Components are `Float` (F64); matrices are
column-major. Operators `+`/`-` are element-wise, `*` is element-wise on vectors
(Hadamard) / matrix-multiply on matrices, and `Mat * Vec` transforms a vector.

```jet
fn main() {
    a: Vec3 @= Vec3(1.0, 2.0, 3.0)
    b: Vec3 @= Vec3(4.0, 5.0, 6.0)
    sum: Vec3 @= a + b
    print(a.dot(b))                 // 32.0
    print(a.cross(b).to_array())    // [-3.0, 6.0, -3.0]
    print(Vec3(0.0, 3.0, 4.0).length())   // 5.0

    scale: Mat3 @= Mat3(2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0)
    out: Vec3 @= scale * Vec3(1.0, 2.0, 3.0)
    print(out.to_array())           // [2.0, 4.0, 6.0]
}
```

| Item | Notes |
|------|-------|
| `Vec2`/`Vec3`/`Vec4(x, …)` | Positional construction from `Float` components |
| `Mat3`/`Mat4(m0, …)` | N*N components, column-major |
| `T.splat(x)` / `T.from_array(a)` | Fill all components / build from `[Float#N]` |
| `v.dot(w)` | Scalar dot product |
| `v.cross(w)` | Cross product (`Vec3` only) → `Vec3` |
| `v.length()` / `v.normalize()` | Euclidean length / unit vector |
| `m.matmul(n)` / `m.transpose()` | Matrix product / transpose |
| `m.transform(v)` | Same as `m * v` |
| `v.to_array()` | Round-trip out to `[Float#N]` (D-FIXARR1 bridge) |
| `+` `-` `*` | Element-wise (vectors); `*` = matmul (matrices) / transform (`Mat*Vec`) |

---

### SIMD lanes — `F32x4`, `F64x2` (D-SIMD1/D-SIMD2)

Built-in portable lane types — no import. `F32x4` holds four `F32` lanes, `F64x2`
two `F64`. Element-wise `+`/`-`/`*`/`/` run across every lane at once; `v[i]`
reads a lane; reductions fold the lanes. On the pinned stable toolchain these
lower to a safe scalar-array fallback (no intrinsics, no `std::simd` gate) — a
portable-SIMD backend can replace it later behind the same surface.

```jet
fn main() {
    v: F32x4 @= F32x4(1.0, 2.0, 3.0, 4.0)
    w: F32x4 @= F32x4(10.0, 20.0, 30.0, 40.0)
    s: F32x4 @= v + w
    print(s.to_array())             // [11.0, 22.0, 33.0, 44.0]
    print(v[2])                     // 3.0
    print(v.sum())                  // 10.0
    print(v.reduce(#Max))           // 4.0
    print(F32x4.splat(7.0).to_array())   // [7.0, 7.0, 7.0, 7.0]
}
```

| Item | Notes |
|------|-------|
| `F32x4(a, b, c, d)` / `F64x2(a, b)` | Positional lane construction |
| `T.splat(x)` / `T.from_array(a)` | One scalar in every lane / build from `[F32#4]`·`[F64#2]` |
| `v[i]` | Read lane `i` (bounds-checked) |
| `+` `-` `*` `/` | Element-wise across all lanes |
| `v.sum()` `v.product()` `v.min()` `v.max()` | Named reductions → lane scalar |
| `v.reduce(#Add)` `#Mul` `#Min` `#Max` | General reduce by op marker |
| `v.to_array()` | Round-trip out to `[F32#4]` / `[F64#2]` |

---

### `core.random` — random numbers

```jet
use core.random as random

fn main() {
    random.seed(42)                         // make the sequence repeatable
    print(random.int(1, 6))                 // inclusive range (like dice)
    print(random.float())                   // 0.0 .. 1.0
    items :: [10, 20, 30]
    print(random.pick(items))               // one item, or null if list empty
    random.shuffle(mut items)               // shuffle in place
    print(items)
}
```

Without `seed`, the generator starts from the current time — fine for games,
not for tests.

| Function | Returns | What it does |
|----------|---------|--------------|
| `seed(n)` | nothing | Reset the generator (deterministic after this) |
| `int(low, high)` | `Int` | Random integer, both ends inclusive |
| `float()` | `Float` | Random float from 0 up to (but not including) 1 |
| `pick(xs)` | `T?` | Random element, or null if `xs` is empty |
| `shuffle(mut xs)` | nothing | Randomly reorder a list in place |
| `rng(seed)` | `Rng` | A **deterministic** RNG capability seeded by `seed` (D-DET1) |

The ambient calls above (`int`/`float`/…) read a process-global generator, so a
`#Pure fn` cannot call them (E3403 — they break reproducibility). To use
randomness inside a `#Pure fn`, take a seeded `Rng` **as a parameter** and draw
through it — the seed makes the stream reproducible on every machine:

```jet
#Pure fn roll(rng: ~Rng) -> Int {
    return rng.int(1, 6)            // inclusive; advances the stream (needs ~Rng)
}
fn main() {
    r := random.rng(42)            // same seed → same draws everywhere
    print(roll(~r))
}
```

| `Rng` method | Returns | What it does |
|--------------|---------|--------------|
| `int(lo, hi)` | `Int` | Draw an Int in `[lo, hi]` (inclusive); advances the stream |
| `float()` | `Float` | Draw a Float in `[0.0, 1.0)`; advances the stream |

---

### `core.time` — clock and delays

Time in Core is **Unix milliseconds** only — no dates, time zones, or
formatting (use `jet.time` for calendars).

```jet
use core.time as time

fn main() {
    started :: time.now()                // milliseconds since 1970-01-01 UTC
    time.sleep(100)                      // pause ~100 ms (blocking)
    sw :: time.start()                   // Stopwatch
    time.sleep(50)
    print(sw.elapsed_millis())           // at least 50
    print(time.now() - started)
}
```

| Function / type | Returns | What it does |
|-----------------|---------|--------------|
| `now()` | `Int` | Current Unix time in milliseconds |
| `sleep(millis)` | nothing | Block for about `millis` milliseconds |
| `time.start()` | `Stopwatch` | Start a stopwatch |
| `sw.elapsed_millis()` | `Int` | Milliseconds since `time.start()` |
| `clock(seed)` | `Clock` | A **deterministic** clock capability starting at `seed` ms (D-DET1) |

**Test hook:** when the environment variable `LEX_TEST_EPOCH` is set to an
integer, `time.now()` returns that value instead of the real clock. Tests use
this to pin output; normal programs ignore it.

A `#Pure fn` cannot call ambient `time.now()` (E3403 — the wall clock is not
reproducible). To use time inside a `#Pure fn`, take a seeded `Clock` **as a
parameter** and read through it; the clock only moves when you `tick` it, so the
result is reproducible:

```jet
#Pure fn at(clock: Clock) -> Int {
    return clock.now()             // current value in ms; pure read
}
fn main() {
    c @= time.clock(1000)          // a Clock starting at 1000 ms
    print(at(c))                   // 1000, on every machine
}
```

| `Clock` method | Returns | What it does |
|----------------|---------|--------------|
| `now()` | `Int` | The clock's current value in ms (read; no `~` needed) |
| `tick(ms)` | `Int` | Advance the clock by `ms` and return the new value (needs `~Clock`) |

**Expert escape — `assume_deterministic { … }`.** Inside a `#Pure fn`, a block
written `assume_deterministic { … }` suspends the determinism check (E3401/E3403)
for its body — the "I know this is deterministic" hatch. It is a semantic
footgun: nothing verifies the claim, so use it only when you can guarantee
reproducibility yourself. See `examples/features/111_determinism.jet`.

---

### `core.encoding` — unified serialization (json, csv, toml, yaml)

One library, every format a submodule (D-ENC1). Import the whole library and
reach each format by name, or import a single format directly:

```jet
use core.encoding                    // encoding.json.*, encoding.csv.*, …
use core.encoding.json as json       // or just one format
```

Every format speaks the same two verbs: `parse` (text → value) and `to_string`
(value → text, D-JSONVERB1). JSON adds `to_string_pretty` and `decode`.

```jet
use core.encoding

fn main() {
    raw :: "{\"name\":\"jet\",\"ok\":true,\"n\":1.5}"
    data :: encoding.json.parse(raw) ?? return
    print(encoding.json.to_string(data))           // compact one line
    print(encoding.json.to_string_pretty(data))    // indented

    if data == Object(entries) {
        if entries.contains("name") {
            print(entries["name"])
        }
    }
}
```

**`core.encoding.json`** — dynamic JSON; you walk a `JSON` enum by hand.
**`JSON` variants:** `Null`, `Boolean(b)`, `Number(n)`, `Text(s)`,
`Array(items)`, `Object(entries: [String, JSON])`.

| Function | Returns | What it does |
|----------|---------|--------------|
| `parse(text)` | `JSON ? JSONError` | Parse a JSON string |
| `decode(text)` | `JSON ? JSONError` | Lenient parse — coerces string→number/bool, logs each coercion (D-JSON3) |
| `to_string(j)` | `String` | Compact JSON text |
| `to_string_pretty(j)` | `String` | Indented JSON text |

**`JSONError`** — `line` and `message` pointing at the parse failure.

**`core.encoding.csv`** — `parse(text) -> [[String]] ? String` (rows of fields),
`to_string(rows) -> String`. **`core.encoding.toml`** / **`core.encoding.yaml`**
— `parse(text) -> [String, String] ? String` (flat key/value), `to_string(map)`.

#### Typed (de)serialization — one derive, every format (D-SERDE1–8)

Mark a type `#[Codable]` and it crosses the wire in any format. `#[Codable]` is
both directions; the one-way markers are `#[Encode]` (write-only) and `#[Decode]`
(read-only). The derive is compiler-owned (like `derive Comparable`) — no macros,
no runtime reflection.

```jet
use core.encoding.csv as csv
use core.encoding.json as json

#[Codable]
struct Order {
    id: Int
    #[Rename("customer")] who: String      // wire key overrides the field name
    items: [String]
    note: String?                          // absent optional is omitted on the wire
}

fn main() {
    o @= Order { id: 7, who: "Ada", items: ["pen", "ink"], note: null }
    print(json.to_string(o))               // {"id":7,"customer":"Ada","items":["pen","ink"]}

    raw @= "{{\"id\":9,\"customer\":\"Bo\",\"items\":[\"ink\"],\"note\":\"rush\"}}"
    back @= json.decode<Order>(raw) ?? panic("bad order")   // typed decode
    print(back.who)                        // Bo
}
```

**Encode** — `to_string(v)` / `to_string_pretty(v)` accept any `#[Codable]`/`#[Encode]`
value (the dynamic `JSON` tree and the `[[String]]`/`Map` forms still work too). Field
order is preserved.

**Typed decode** — `decode<T>(text)` (D-SERDE6) returns `T ? DecodeError` for
json/toml/yaml, and `[T] ? DecodeError` for csv (one struct per row, columns mapped
to fields by header name). The target type comes from the `<T>` turbofish or an
expected type (`cfg: Config @= json.decode(text)`). Bare `json.decode(text)` with no
target stays the lenient dynamic `JSON` (above). `DecodeError` carries a field `path`
and a `reason`; compose it with `??`.

```jet
raw @= "item,qty\npen,3\nink,5"
sales @= csv.decode<Sale>(raw) ?? panic("bad csv")   // [Sale]
print(json.to_string(sales))   // [{"item":"pen","qty":3},{"item":"ink","qty":5}]
```

**Field attributes** (D-SERDE5):

| Attribute | Effect |
|-----------|--------|
| `#[Rename("k")]` | use `k` as the wire key for this field |
| `#[Skip]` | never serialize; on decode use the field's default |
| `#[Default]` / `#[Default(8080)]` | when the key is absent, use the type's default (or the given literal) |
| `#[Flatten]` | inline a `#[Codable]` struct field's keys into the parent object |

**Container attributes:**

| Attribute | Effect |
|-----------|--------|
| `#[RenameAll(camel)]` | map every field's wire key — `camel`/`snake`/`pascal`/`kebab`/`screaming` (D-SERDE3) |
| `#[DenyUnknownFields]` | a wire key the struct doesn't declare is an error, not ignored (D-SERDE8) |
| `#[Tag("type")]` / `#[Untagged]` | enum wire representation (D-SERDE7); default is externally tagged |

**Enums** serialize externally tagged by default: a unit variant is its bare name
(`"Closed"`), a payload variant is `{"Variant": payload}`. `#[Tag("type")]` switches
to internal tagging (`{"type":"Click", …}`); `#[Untagged]` emits the payload alone.

Unknown wire keys are ignored by default (forward-compatible); opt into strict
checking with `#[DenyUnknownFields]`. Diagnostics: E2407 (`#[Rename]` non-string),
E2408 (`#[Flatten]` non-struct), E2409 (bad `#[RenameAll]` style), E2410 (missing
required field, runtime), E2411 (type isn't serializable), E2412 (unknown field,
runtime), E2413 (generic serde not yet supported).

> The expert hand-impl path (`impl T: Encode { fn encode … }` over the `DataTree`
> tree, D-SERDE2) and generic-type serde are future increments; see
> `tools/Tower/docs/sidequests/serde-model.md`.

---

### `core.tasks` — tasks and channels

Blocking tasks and typed channels are Jet's concurrency model. There is no
`async`/`await` and no mutex API; tasks communicate by sending owned values.

```jet
use core.tasks as tasks

fn sum_range(first: Int, last: Int) -> Int {
    total := 0
    loop n in first..last {
        total += n
    }
    return total
}

fn main() {
    a :: tasks.spawn(() => sum_range(1, 25))
    b :: tasks.spawn(() => sum_range(26, 50))
    c :: tasks.spawn(() => sum_range(51, 75))
    d :: tasks.spawn(() => sum_range(76, 100))
    print(a.join() + b.join() + c.join() + d.join())
}
```

Channels carry one type:

```jet
use core.tasks as tasks

fn main() {
    ch: Channel<Int> :: tasks.channel()
    sender :: ch.sender()
    task :: tasks.spawn(take(sender) () => {
        sender.send(42)
    })
    task.join()
    print(ch.receive() ?? panic("channel closed"))
}
```

| Function / type | Returns | What it does |
|-----------------|---------|--------------|
| `tasks.spawn(lambda)` | `Task<T>` | Run a zero-parameter lambda on a new task |
| `task.join()` | `T` | Wait for the task and consume the task handle |
| `tasks.channel<T>()` | `Channel<T>` | Create a typed channel receive half |
| `ch.sender()` | `Sender<T>` | Create a clonable send half |
| `sender.send(value)` | nothing | Move one value into the channel |
| `ch.receive()` | `T ? Closed` | Block for a value, or return `Closed` when senders are gone |

Values crossing `spawn` or `send` must be sendable: no `view` borrows, no
structs containing `ref` fields, no trait values, and no closure values unless
they are handed over with `take`. A `Task` that goes out of scope without
`.join()` emits warning **L1101**.

### `jet.regex` — linear-time regular expressions

`use jet.regex as re`. Matching is **linear-time** — the engine is a DFA/NFA
hybrid with no catastrophic backtracking, so patterns are ReDoS-safe by
construction. Backreferences and lookaround do not exist (the safety property
would be lost), and that is deliberate.

Every call returns a `Result`; the `Err` carries a one-line message when the
pattern itself is malformed (the only failure at the boundary). A `Match` is a
list of capture groups: `group(0)` is the whole match, `group(n)` is the n-th
group as `String?` (`null` if the group did not participate or `n` is out of
range).

```jet
use jet.regex as re

fn main() {
    text :: "order 42 shipped"
    print(re.is_match("\\d+", text) ?? panic("bad pattern"))   // true

    m :: re.match("(\\d+) shipped", text) ?? panic("bad pattern")
    if m == value(mat) {
        print(mat.group(0) ?? "")   // 42 shipped
        print(mat.group(1) ?? "")   // 42
    }

    print(re.replace_all("\\d+", text, "#") ?? panic("bad pattern"))
}
```

| Call | Returns | Does |
|------|---------|------|
| `re.is_match(pat, text)` | `Bool ? String` | whether `pat` occurs anywhere |
| `re.match(pat, text)` | `Match? ? String` | first match with capture groups, `null` if none |
| `re.find(pat, text)` | `String? ? String` | first matched substring, `null` if none |
| `re.find_all(pat, text)` | `[String] ? String` | every non-overlapping match, left to right |
| `re.replace(pat, text, repl)` | `String ? String` | replace the first match (`$1`, `${name}` allowed in `repl`) |
| `re.replace_all(pat, text, repl)` | `String ? String` | replace every match |
| `re.split(pat, text)` | `[String] ? String` | split `text` on every match |
| `mat.group(n)` | `String?` | capture group `n` of a `Match` |

Note: `{N}` quantifiers must be written `{{N}}` in Jet source — single braces
are string interpolation (S8). Write `\\d{{4}}` for "four digits".

`regex` is the one owner-approved I6 bootstrap dependency (D-REGEX1): it lives
only inside the hidden FFI bridge crate, never in the compiler, and is slated to
be replaced by an in-house RE2-style engine before the end of Epoch 3.

---

### `core.mem` — arenas and regions

Expert-tier explicit allocators, unlocked by `use core.mem` (no `#Unsafe`
needed — arenas are the *safe* fast-allocation primitive). An arena bump-allocates
many values into one buffer and frees them all at once.

```jet
use core.mem

fn main() {
    arena :: mem.Arena.new()             // or .new(capacity: 4096)
    x :: arena.alloc(42)                 // x is a *view* into the arena
    y :: arena.alloc("hi")
    print(x)
    print(y)
    arena.reset()                        // frees everything; buffer reused
    z :: arena.alloc(7)
    print(z)
}
```

`arena.alloc(value)` hands back a **view** into the arena's storage, not an owned
copy. A view is fast and zero-copy, but it lives only inside its **region** — the
scope of the `arena` binding — and only until the arena is `reset`/`free`d. The
checker enforces both:

- returning, storing, or giving away a view → **E0631** (it would outlive the arena);
- using a view after `reset()`/`free()` → **E0632**.

Both are compile errors, so a dangling arena pointer can never run. Copy what you
need out (`x.clone()`) before it leaves the region.

For the cases scope-inference is too coarse — a region spanning two allocators, or
narrower than the function — write an explicit **`region r { … }`** block:

```jet
use core.mem

fn main() {
    region scratch {
        a :: mem.Arena.new()
        b :: mem.Bump.new()
        first :: a.alloc(1)
        second :: b.alloc(2)
        print(first)
        print(second)
    }                                    // both arenas freed here
}
```

| Type / verb | What it does |
|-------------|--------------|
| `mem.Arena.new()` / `.new(capacity: N)` | A general grow-only arena |
| `mem.Bump` / `mem.Pool` / `mem.Fixed` | Bump / fixed-slot / static-backed variants |
| `arena.alloc(value)` | Store `value`, return a scope-bound view |
| `arena.reset()` | Free everything, keep the buffer (reusable) |
| `arena.free()` | Return the buffer to the OS |
| `region r { … }` | An explicit region — views inside may not escape it |

---

## Text parsing

Turn text into values and split it into lines. `to_int` is fallible — it returns
the same `Int ? ParseError` result `Int.parse` does, so handle it with `?`/`??`.

```jet
fn main() {
    n @= "42".to_int() ?? -1                 // 42
    bad @= "oops".to_int() ?? -1             // -1 (parse failed → fallback)
    print(n + bad)

    loop line in "first\nsecond".lines() {   // ["first", "second"]
        print(line)
    }
}
```

| API | Returns | What it does |
|-----|---------|--------------|
| `String.to_int()` | `Int ? ParseError` | Parse the text as an integer (leading/trailing space ignored) |
| `String.lines()` | `[String]` | Split into lines (`\n` and `\r\n`; no trailing empty line) |

---

## Binary data (`U8`)

The `U8` type holds one byte (0–255). Literals outside that range are a compile
error (**E1003**).

```jet
fn main() {
    b: U8 :: 255
    print(b.to_int())                       // 255 as Int
    n :: 42.to_u8() ?? return              // checked conversion
    bytes :: "hi".bytes()                  // [U8]
    text :: String.from_bytes(bytes) ?? return
    print(text)
}
```

| API | Returns | What it does |
|-----|---------|--------------|
| `String.bytes()` | `[U8]` | UTF-8 bytes of a string |
| `String.from_bytes(bs)` | `String ? UTF8Error` | Decode UTF-8 bytes |
| `n.to_u8()` | `U8 ? String` | Checked Int → U8 |
| `b.to_int()` | `Int` | U8 → Int |

Use `fs.read_bytes` / `fs.write` when you need raw file bytes.

---

## Numeric surface (D-NUMOPS1)

Plain integer arithmetic (`+` `-` `*` `/`) **traps on overflow** at every width —
a result outside the type's range stops the program with a Jet panic instead of
silently wrapping. Opt a single op out at the use site:

```jet
fn main() {
    hi: U8 :: 200
    lo: U8 :: 100
    print(wrapping(hi + lo))            // 44   — wraps around (C behaviour)
    print(saturating(hi + lo))          // 255  — clamps to the type's range
    print(checked(hi + lo) ?? 0)        // 0    — checked(…) -> T?, null on overflow
}
```

| Form | Returns | What it does |
|------|---------|--------------|
| `expr` (`a + b`, …) | `T` | Traps on overflow (safe default) |
| `wrapping(a + b)` | `T` | Wraps around the type's range |
| `saturating(a + b)` | `T` | Clamps to `MIN`/`MAX` |
| `checked(a + b)` | `T?` | `null` on overflow |

**Bounds and float constants** — per-type `MIN`/`MAX`, plus float specials:

| Member | On | Value |
|--------|----|-------|
| `U8.MAX` / `I32.MIN` / … | any integer type | the type's range ends |
| `Float.INFINITY` / `.NEG_INFINITY` | floats | ±∞ |
| `Float.NAN` | floats | not-a-number |
| `Float.EPSILON` | floats | smallest representable step |

**Predicates and bit queries:**

| Method | On | Returns |
|--------|----|---------|
| `x.is_nan()` / `.is_infinite()` / `.is_finite()` | floats | `Bool` |
| `n.count_ones()` / `.count_zeros()` | integers | `Int` |
| `n.leading_zeros()` / `.trailing_zeros()` | integers | `Int` |

**Bit operators** — `&` `|` `^` keep the operand width (both sides the same
type); `<<` `>>` take any integer shift-count and keep the left side's type. A
shift count past the type's width traps (no leaked Rust panic).

**Width conversions** are named methods — no implicit narrowing or widening:

| Method | Returns | Direction |
|--------|---------|-----------|
| `.to_i64()` / `.to_u32()` / … (widening) | `T` | infallible |
| `.to_u8()` / `.to_i16()` / … (narrowing) | `T ? String` | fallible (`?`/`??`) |
| `.to_f32()` / `.to_float()` | `F32` / `Float` | infallible |

---

## Common mistakes (and what Jet suggests)

| You wrote | Jet wants |
|-----------|-----------|
| `println(...)` | `print(...)` |
| `eprintln(...)` | `io.eprint(...)` |
| `open("file")` / `File.open` | `fs.read(...)` / `fs.write(...)` |
| `getenv("X")` / `os.environ` | `env.get("X")` |
| `import core.fs` | `use core.fs` (teaching error E0015) |
| `val x = …` / `var x = …` | `x :: …` (immutable) / `x := …` (mutable) |

---

## First-party ring (`jet.*` packages)

Core (`jet.core`) stays at the eight modules above. The first-party ring
ships as versioned `jet.*` packages. These shipped in Epoch 2:

| Package | What it unlocks |
|---------|-----------------|
| `jet.http` | HTTP client + server, blocking networking (plain HTTP; TLS requires `jet.tls`) |
| `jet.regex` | grep-class tools, text validation |
| `jet.log` | Structured logging / tracing / metrics |
| `jet.time` | Calendar dates, time zones, formatted dates |
| `jet.crypto` | Hash, HMAC, vetted random primitives |
| `jet.archive` | zip / tar / gzip (staged — not yet available) |
| `jet.db` | SQLite (FFI-tier) (staged — not yet available) |

---

## Writing Core in Jet (future)

Today, Core lives in the compiler as typed signatures plus Rust prelude templates
(`Source/Prelude/Std.rs`). The **API** is Jet; the **implementation** is Rust until
the package system fully stabilizes.

---

## Examples in this repo

| Example | Shows |
|---------|-------|
| `examples/features/29_files.jet` | Read, transform, write with errors |
| `examples/features/30_json.jet` | Parse, inspect, mutate, re-render JSON |
| `examples/features/31_cli.jet` | Args, environment, exit codes |
| `examples/features/106_serde_derive.jet` | `#[Codable]` encode + typed `decode<T>` with `#[Rename]` |
| `examples/features/107_csv_typed.jet` | `csv.decode<Row>` → struct → JSON (the typed CSV pipeline) |
| `examples/features/108_json_typed.jet` | Nested struct + list + optional round-trip with `#[RenameAll(camel)]` |

Run the full battery: `nix develop -c cargo test --test golden` and `nix develop -c cargo test --test corelib`.
