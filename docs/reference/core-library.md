# Core library (`core`)

The Jet Core library gives you files, terminal I/O, environment variables,
process control, math, time, random numbers, JSON, tasks, and channels —
enough to write real command-line tools. Every fallible call returns a
`T ? E` value; nothing in Core panics on its own.

**How it works today:** Core modules are built into the compiler. Use them by
name; the compiler type-checks your calls and generates only the helpers you
actually use (see [Using modules](#using-modules) and [Pay for what you call](#pay-for-what-you-call)).

**Canonical name:** `core` (owner, 2026-06-26). Every first-party library — the
built-in modules below and the ring packages — lives under the single `core.*`
namespace. There is no `jet.*` or `std.*` library namespace.

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
use core.encoding.json as json       // a nested submodule
```

`use core.fs` and `use core.encoding.json` each resolve to a
compiler-known module under the `core` root.

**Not allowed:**

```jet
import core.fs as fs        // teaching error E0015 — use `use core.fs`
use "std/fs"               // quoted paths are for .jet files only
```

If you name a local file or folder `core`, `jet`, `http`, `regex`, `csv`, `toml`,
`crypto`, or `archive`, the compiler rejects it — those names are reserved for
first-party packages (**E1002**). An unknown core module is **E1001**;
selective imports (`use core.fs.{read}`) are rejected — keep qualified access
through an alias. An unknown item in a known core module is **E1004**, with a
did-you-mean suggestion when possible.

Fallible core functions return `T ? E` and must be handled with `?`, `??`, or
a pattern test like any other Jet result. File APIs are whole-file only (no
streaming handles); paths are plain `String`; binary APIs use `U8` and `[U8]`.

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

## Optional values (`T?`) — combinators (D-HOLE1)

`T?` is either `value(x)` (present) or `null` (absent) — see S31/S35 for the
core pattern-test and `??` fallback forms. Composing two or more optionals
gets library combinators instead of a general "hole"/absent-propagating value
type (D-HOLE1 rejected that: it would duplicate `T?` and silently bypass
distinct-type arithmetic gating like `@Numeric`).

| Method | Type | What it does |
| --- | --- | --- |
| `.map(f)` | `(T?, fn(T) -> R) -> R?` | Applies `f` to the payload if present; `null` stays `null` |
| `.zip(other)` | `(T?, U?) -> (a: T, b: U)?` | Pairs two optionals: present only when **both** are present |
| `Option.lift2(f, a, b)` | `(fn(T, U) -> R, T?, U?) -> R?` | Applies a two-argument function to `a`/`b` only when both are present |

```jet
price: Float? :: lookup_price(id)
qty: Float? :: lookup_qty(id)

// zip: both present -> present pair; either null -> null
total1 :: price.zip(qty).map((pair) => pair.a * pair.b)

// lift2: same idea, no explicit pair
total2 :: Option.lift2((p, q) => p * q, price, qty)

// total1, total2: Float? — null unless both price and qty were present
```

See `examples/features/types/option_combinators.jet`.

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
    entries :: fs.list_dir("/tmp") ?? return
    print(entries.len())
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
| `list_dir(path)` | `[DirEntry] ? IOError` | One entry per directory member, sorted by name (D-LSDIR1) |
| `create_dir(path)` | `() ? IOError` | Create a directory |
| `is_dir(path)` | `Bool` | Whether the path is a directory |
| `copy(from, to)` | `() ? IOError` | Copy a file |
| `rename(from, to)` | `() ? IOError` | Rename or move a file |

**`IOError`** — `NotFound(path)`, `PermissionDenied(path)`, or `Other(message)`.

**`DirEntry`** (D-LSDIR1) has three readable fields:

| Field    | Type   | Meaning                             |
|----------|--------|--------------------------------------|
| `name`   | String | bare filename (no directory prefix)  |
| `path`   | String | full path (portable, OS-native sep)  |
| `is_dir` | Bool   | true when the entry is a directory   |

Use `entry.path` for a ready-to-use path (don't build `"{dir}/{entry}"` by
hand) and `entry.name` for filename checks (`entry.name.ends_with(".txt")`).
`core.path` provides `path.join(dir, name) -> String` plus `.parent()`,
`.extension()`, and `.normalize()` for composing paths independently of
`DirEntry`. Example: `examples/features/io/dir_entry.jet`.

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

`jet run file.jet -- arg1 arg2` forwards everything after `--` verbatim as
program arguments (`io.args()` sees them, argv[1..]); plain positional words
with no separator also work (`jet run greet.jet Ada`). An unknown `--`-flag
written before the `--` is **E2102**, which teaches the `--` form (D-CLI1).
`jet test` also accepts `--`; `jet build` does not (no running process).

---

### `core.args` — declarative CLI parsing (D-ARGS1)

Build a flag/option/positional spec once and parse `io.args()` against it,
instead of hand-walking `[String]`:

```jet
use core.args as args

fn main() {
    spec :: args.spec()
        .flag("verbose", "print extra detail")
        .option("output", "write result to FILE", "FILE")
        .positional("input", "file to read")
    parsed :: spec.parse(io.args()) ?? panic(spec.help())
    print(parsed.flag("verbose"))
    print(parsed.option("output") ?? "(default)")
}
```

`args.spec()` returns an `ArgsSpec` builder; each method consumes it and
returns a new one:

| Method | Signature | Registers |
|--------|-----------|-----------|
| `.flag(name, help)` | `(String, String) → ArgsSpec` | `--name` boolean flag |
| `.option(name, help, meta)` | `(String, String, String) → ArgsSpec` | `--name VALUE` string option |
| `.positional(name, help)` | `(String, String) → ArgsSpec` | required positional |
| `.help()` | `() → String` | formatted help text |
| `.parse(argv)` | `([String]) → ParsedArgs ? String` | parses `argv` against the spec |

`ParsedArgs` query methods:

| Method | Signature | Returns |
|--------|-----------|---------|
| `.flag(name)` | `(String) → Bool` | true if `--name` was passed |
| `.option(name)` | `(String) → String?` | value of `--name VALUE`, or `null` |
| `.positional(idx)` | `(Int) → String?` | the nth positional (0-based), or `null` |

`--help` is not wired automatically — add a `.flag("help", "…")` and check
`parsed.flag("help")` yourself. `.parse` returns `ParsedArgs ? String`, where
the error string carries the parse message (unknown flag, missing positional,
…). Wrong argument counts on builder/query methods are **E1301**–**E1304**.
Example: `examples/features/io/cli_args.jet`.

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
a: Vec3 :: Vec3(1.0, 2.0, 3.0)
b: Vec3 :: Vec3(4.0, 5.0, 6.0)
sum: Vec3 :: a + b
    print(a.dot(b))                 // 32.0
    print(a.cross(b).to_array())    // [-3.0, 6.0, -3.0]
    print(Vec3(0.0, 3.0, 4.0).length())   // 5.0

scale: Mat3 :: Mat3(2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0)
out: Vec3 :: scale * Vec3(1.0, 2.0, 3.0)
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
v: F32x4 :: F32x4(1.0, 2.0, 3.0, 4.0)
w: F32x4 :: F32x4(10.0, 20.0, 30.0, 40.0)
s: F32x4 :: v + w
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
`@Pure fn` cannot call them (E3403 — they break reproducibility). To use
randomness inside a `@Pure fn`, take a seeded `Rng` **as a parameter** and draw
through it — the seed makes the stream reproducible on every machine:

```jet
@Pure fn roll(rng: ~Rng) -> Int {
    return rng.int(1, 6)            // inclusive; advances the stream (needs ~Rng)
}
fn main() {
    r := random.rng(42)            // same seed → same draws everywhere
    print(roll(~r))
}
```

The injected `Rng` mirrors the full ambient `random.*` set (D-DET-CAPAPI):

| `Rng` method | Returns | What it does |
|--------------|---------|--------------|
| `int(lo, hi)` | `Int` | Draw an Int in `[lo, hi]` (inclusive); advances the stream |
| `float()` | `Float` | Draw a Float in `[0.0, 1.0)`; advances the stream |
| `bool()` | `Bool` | Draw a coin; advances the stream |
| `pick(xs)` | `T?` | Uniform element of `[T]`, or null if empty; advances the stream |
| `shuffle(~xs)` | nothing | Reorder a list in place (Fisher–Yates); advances the stream |

Every draw — including `bool`/`pick`/`shuffle` — needs a `~Rng` receiver, and
`shuffle` needs the list passed with `~` (it edits in place).

---

### `core.time` — clock and delays

Time in Core is **Unix milliseconds** only — no dates, time zones, or
formatting (use `core.time` for calendars).

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
| `sleep(millis)` | nothing | Block for about `millis` milliseconds (runtime E3003 if an ambient `#Context(deadline: …)` budget expires first) |
| `time.start()` | `Stopwatch` | Start a stopwatch |
| `sw.elapsed_millis()` | `Int` | Milliseconds since `time.start()` |
| `clock(seed)` | `Clock` | A **deterministic** clock capability starting at `seed` ms (D-DET1) |
| `ms(n)` | `Duration` | A `Duration` of `n` milliseconds (pure value; D-DET-CAPAPI) |
| `secs(n)` | `Duration` | A `Duration` of `n` seconds (pure value; D-DET-CAPAPI) |

**Test hook:** when the environment variable `LEX_TEST_EPOCH` is set to an
integer, `time.now()` returns that value instead of the real clock. Tests use
this to pin output; normal programs ignore it.

A `@Pure fn` cannot call ambient `time.now()` (E3403 — the wall clock is not
reproducible). To use time inside a `@Pure fn`, take a seeded `Clock` **as a
parameter** and read through it; the clock only moves when you `tick` it, so the
result is reproducible:

```jet
@Pure fn at(clock: Clock) -> Int {
    return clock.now()             // current value in ms; pure read
}
fn main() {
    c :: time.clock(1000)          // a Clock starting at 1000 ms
    print(at(c))                   // 1000, on every machine
}
```

| `Clock` method | Returns | What it does |
|----------------|---------|--------------|
| `now()` | `Int` | The clock's current value in ms (read; no `~` needed) |
| `tick(ms)` | `Int` | Advance the clock by `ms` (relative) and return the new value (needs `~Clock`) |
| `advance(to_ms)` | `Int` | Set the clock to the **absolute** instant `to_ms` and return it (needs `~Clock`; D-DET-CAPAPI) |
| `wait(d)` | `Int` | Advance the clock by a `Duration` `d` and return the new value (needs `~Clock`; D-DET-CAPAPI) |

A `Duration` is a deterministic span of milliseconds minted by `time.ms(n)` /
`time.secs(n)` (pure value constructors). Read it back with `d.millis()`.

| `Duration` method | Returns | What it does |
|-------------------|---------|--------------|
| `millis()` | `Int` | The span in milliseconds (read) |

**Expert escape — `assume_deterministic { … }`.** Inside a `@Pure fn`, a block
written `assume_deterministic { … }` suspends the determinism check (E3401/E3403)
for its body — the "I know this is deterministic" hatch. It is a semantic
footgun: nothing verifies the claim, so use it only when you can guarantee
reproducibility yourself. See `examples/features/effects/determinism.jet`.

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

    if data == .Object(entries) {
        if entries.contains("name") {
            print(entries["name"])
        }
    }
}
```

**One dynamic value, four format faces (D-ENC-DYN1).** Every format's untyped
`parse` returns the same rich dynamic value, internally `DataTree`, user-facing
as **`Data`** — variants `.Null` / `.Bool` / `.Int` / `.Float` / `.Text` /
`.Array` / `.Object`. `Json`, `Toml`, `Yaml`, and `Csv` are type aliases over
`Data` (so `json.parse` reads as `Json`, `toml.parse` as `Toml`, …), but it's
one structure with one walker and one accessor set (`.field(name)`, `.at(i)`,
`.int()`, `.float()`, `.text()`, `.bool()`). Integral numbers decode to `.Int`,
fractional to `.Float`; objects keep field order.

| Function | Returns | What it does |
|----------|---------|--------------|
| `parse(text)` | `Json ? JSONError` | Parse a JSON string |
| `decode(text)` | `Json ? JSONError` | Lenient parse — coerces string→number/bool, logs each coercion (D-JSON3) |
| `to_string(j)` | `String` | Compact JSON text |
| `to_string_pretty(j)` | `String` | Indented JSON text |

**`JSONError`** — `line` and `message` pointing at the parse failure.

**`core.encoding.csv`** — `parse(text) -> [[String]] ? String` (rows of fields),
`to_string(rows) -> String`. **`core.encoding.toml`** / **`core.encoding.yaml`**
— `parse(text) -> Toml ? JSONError` / `Yaml ? JSONError` (full adapters over
`Data`, not a flat map), `to_string(value)`.

Each adapter is a full serde equivalent, not a lossy subset:

- **JSON** — full RFC 8259: exponents and the strict number grammar, every
  escape including `\uXXXX` with surrogate-pair combining; rejects invalid
  escapes, lone surrogates, and raw control characters with a line + message.
- **CSV** — header-mapped typed rows (`decode<T>` maps columns to fields by name).
- **TOML** — full TOML 1.0: `[table]` headers, `[[array-of-tables]]`, dotted keys,
  inline tables, strings (every escape + multi-line), integers in every base,
  floats incl. `inf`/`nan`, booleans, datetimes, arrays.
- **YAML** — full YAML 1.2 core (D-ENC-YAML1): block + flow maps/sequences,
  core-schema typed scalars, single/double-quoted + plain + block scalars
  (`|`/`>` with chomping), comments, `---`/`...` document markers, and
  anchors/aliases (`&a`/`*a`). Explicit/custom tags (`!!str`, `!T`) are deferred.

All four parsers are std-only (I6).

Jet has no general `Any` top type (D-DYNAMIC-TYPE1): use the precise shape for
the job — an enum for a closed set of variants, generics or traits for
abstraction, `T?` for absence, and `Data` for parsed dynamic input. Writing
`Any` in type position is **E0350**.

#### Typed (de)serialization — one derive, every format (D-SERDE1–8)

Mark a type `@[Codable]` and it crosses the wire in any format. `@[Codable]` is
both directions; the one-way markers are `@[Encode]` (write-only) and `@[Decode]`
(read-only). The derive is compiler-owned (like `derive Comparable`) — no macros,
no runtime reflection.

```jet
use core.encoding.csv as csv
use core.encoding.json as json

@[Codable]
struct Order {
    id: Int
    #[Rename("customer")] who: String      // wire key overrides the field name
    items: [String]
    note: String?                          // absent optional is omitted on the wire
}

fn main() {
    o :: Order.{ id: 7, who: "Ada", items: ["pen", "ink"], note: null }
    print(json.to_string(o))               // {"id":7,"customer":"Ada","items":["pen","ink"]}

    raw :: "{{\"id\":9,\"customer\":\"Bo\",\"items\":[\"ink\"],\"note\":\"rush\"}}"
    back :: json.decode<Order>(raw) ?? panic("bad order")   // typed decode
    print(back.who)                        // Bo
}
```

**Encode** — `to_string(v)` / `to_string_pretty(v)` accept any `@[Codable]`/`@[Encode]`
value (the dynamic `JSON` tree and the `[[String]]`/`Map` forms still work too). Field
order is preserved.

**Typed decode** — `decode<T>(text)` (D-SERDE6) returns `T ? DecodeError` for
json/toml/yaml, and `[T] ? DecodeError` for csv (one struct per row, columns mapped
to fields by header name). The target type comes from the `<T>` turbofish or an
cfg: Config :: json.decode(text)`). Bare `json.decode(text)` with no
target stays the lenient dynamic `JSON` (above). `DecodeError` carries a field `path`
and a `reason`; compose it with `??`.

```jet
raw :: "item,qty\npen,3\nink,5"
sales :: csv.decode<Sale>(raw) ?? panic("bad csv")   // [Sale]
print(json.to_string(sales))   // [{"item":"pen","qty":3},{"item":"ink","qty":5}]
```

**Field attributes** (D-SERDE5):

| Attribute | Effect |
|-----------|--------|
| `#[Rename("k")]` | use `k` as the wire key for this field |
| `#[Skip]` | never serialize; on decode use the field's default |
| `#[Default]` / `#[Default(8080)]` | when the key is absent, use the type's default (or the given literal) |
| `#[Flatten]` | inline a `@[Codable]` struct field's keys into the parent object |

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
required field, runtime), E2411 (type isn't serializable — also fires at the use
site for a non-codable generic argument), E2412 (unknown field, runtime). E2413 is
retired (D-SERDE12).

Generic `@[Codable]` is first-class (D-SERDE9-12): the derive auto-injects
`T: Encode`/`T: Decode` bounds on exactly the type params that reach the wire —
the user never spells them. A phantom or `#[Skip]`-only param carries no serde
bound (only structural `Clone`), so `Id<Kind>` serializes for any `Kind`. A
non-codable type argument fails at the use site (E2411), not the definition.

> The expert hand-impl path (`impl T: Encode { fn encode … }` over the `DataTree`
> tree, D-SERDE2) is a future increment; see
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
| `task.wait()` | `T` | Alias of `.join()` |
| `task.pause()` | nothing | Request paused state on the task control plane (D-COROUTINE1) |
| `task.resume()` | nothing | Clear paused state on the task control plane |
| `task.cancel()` | nothing | Request cancellation on the task control plane |
| `task.trace()` | `String` | Read control-plane state as `paused=...,cancel=...` |
| `tasks.channel<T>()` | `Channel<T>` | Create a typed channel receive half |
| `ch.sender()` | `Sender<T>` | Create a clonable send half |
| `sender.send(value)` | nothing | Move one value into the channel |
| `ch.receive()` | `T ? Closed` | Block for a value, or return `Closed` when senders are gone |

Values crossing `spawn` or `send` must be sendable: no `view` borrows, no
structs containing `ref` fields, no trait values, and no closure values unless
they are handed over with `take`. A `Task` that goes out of scope without
`.join()` emits warning **L1101**.
With `#Context(deadline: <Int epoch_ms>)`, blocking waits (`task.join()` /
`task.wait()` / `ch.receive()`) observe the inherited budget and report runtime
**E3003** on exceed.
Current thread-runtime implementation records pause/cancel requests for tracing;
hard scheduler-level pause/cancel behavior lands with the M:N runtime.

### `core.regex` — linear-time regular expressions

`use core.regex as re`. Matching is **linear-time** — the engine is a DFA/NFA
hybrid with no catastrophic backtracking, so patterns are ReDoS-safe by
construction. Backreferences and lookaround do not exist (the safety property
would be lost), and that is deliberate.

Every call returns a `Result`; the `Err` carries a one-line message when the
pattern itself is malformed (the only failure at the boundary). A `Match` is a
list of capture groups: `group(0)` is the whole match, `group(n)` is the n-th
group as `String?` (`null` if the group did not participate or `n` is out of
range).

```jet
use core.regex as re

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

### `core.reactive` — signals, derived values, effects (D-REACT1)

`use core.reactive as reactive`. Reactivity is an **opt-in library**, not core
language semantics — ordinary bindings stay non-reactive. The library adds three
explicit reactive values:

- **signal** — a mutable reactive source. `reactive.signal(initial)` infers `T`
  from the initial value and returns a `Signal<T>`. Read with `.get()`, update
  with `.set(v)`.
- **derived** / **computed** — a value recomputed from the signals it reads.
  `reactive.derived(() => expr)` returns a `Derived<T>`; `reactive.computed` is
  the D-SIGNAL1 canonical alias (type name `Computed<T>`). `.get()` reflects the
  latest computation.
- **effect** — a side effect. `reactive.effect(() => { … })` runs the body now,
  and again whenever a signal it read changes. **`#Reactive { … }`** (D-REACTCORE1)
  is sugar for the same scope — the compiler lowers it to `jet_reactive_effect`.
  **`#Reactive fn`** wraps the whole function body the same way (unit return only).

Dependency tracking is **explicit-by-read**: any `.get()` evaluated inside a
derived or effect body subscribes that derived/effect to the signal. A `.set(v)`
re-runs every subscriber.

```jet
use core.reactive as reactive

fn main() {
    price :: reactive.signal(100)
    qty :: reactive.signal(2)
    total :: reactive.derived(() => (price.get() * qty.get()))
    print(total.get())                       // 200

    reactive.effect(() => print(total.get()))  // prints 200 now
    price.set(150)                             // effect re-runs → 300
    qty.set(3)                                 // effect re-runs → 450
    print(total.get())                         // 450
}
```

| Call | Returns | Does |
|------|---------|------|
| `reactive.signal(initial)` | `Signal<T>` | a mutable reactive source holding `T` |
| `reactive.derived(() => expr)` | `Derived<T>` / `Computed<T>` | a value recomputed from the signals it reads |
| `reactive.computed(() => expr)` | `Computed<T>` | canonical alias for `derived` (D-SIGNAL1) |
| `reactive.effect(() => { … })` | — | a side effect re-run when a read signal changes |
| `#Reactive { … }` | — | explicit reactive effect scope (lowers like `reactive.effect`) |
| `sig.get()` / `der.get()` | `T` | read the current value (and subscribe, inside a derived/effect) |
| `sig.set(v)` | — | write a new value and re-run subscribers |

`Signal`/`Derived` are cheap shared handles — copying one (e.g. capturing it in a
lambda) shares the same reactive cell, so a derived/effect reads the live signal
while outer code keeps `.set`ting it. The runtime is pure std (no external crate);
the compiler-side dataflow graph for tooling/IDEs is a separate, future tooling
feature.

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
    n :: "42".to_int() ?? -1                 // 42
    bad :: "oops".to_int() ?? -1             // -1 (parse failed → fallback)
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

`.to_int()` / `.lines()` and `Int.parse(s)` / `Float.parse(s)` are fully
evaluated at comptime — `ok(v)` / `err(e)` construct `Result` values, and
`?` / `??` propagate or unwrap them in pure comptime expressions
(`examples/features/comptime/comptime_parse.jet`).

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

`Int` and `Float` are the beginner defaults (64-bit: `Int` = `I64`, `Float` =
`F64`). The explicit-width menu — `I8 I16 I32 I64 U8 U16 U32 U64 F32 F64` — is
available for expert and FFI/binary work; `I64`/`F64` interchange with
`Int`/`Float` freely, every other width is its own distinct type. A bare
integer literal adopts the width of the slot it lands in (a binding/parameter/
return annotation, or sized arithmetic) and is range-checked at compile time —
a literal that doesn't fit is **E1003**. Widths never mix implicitly:
arithmetic, comparison, and assignment require the same width on both sides
(**E0109**/**E0112**/**E0108**), with no silent narrowing or widening. The
sized types erase to their Rust equivalents (`u8`…`i64`, `f32`) at codegen, so
they cross the C ABI by value (S59). Width conversions are always named
methods (below), never implicit.

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

Each wrapper takes exactly one integer `+`/`-`/`*`/`/`; anything else is **E1005**.

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

## First-party ring (`core.*` packages)

The eight modules above are built into the compiler. The first-party ring
ships as versioned packages under the same `core.*` namespace (owner,
2026-06-26 — a ring package is a `core.<name>` library, not a `jet.*` one).
These shipped in Epoch 2:

| Package | What it unlocks |
|---------|-----------------|
| `core.http` | HTTP client + server, blocking networking (plain HTTP; TLS requires `core.tls`) |
| `core.regex` | grep-class tools, text validation |
| `core.log` | Structured logging / tracing / metrics |
| `core.time` | Calendar dates, time zones, formatted dates |
| `core.crypto` | Hash, HMAC, vetted random primitives |
| `core.reactive` | Signals, derived values, effects (opt-in reactivity, D-REACT1) |
| `core.archive` | gzip compress/decompress, zip read/write, tar add/get/list (D-DEP-ARCHIVE1) |
| `core.compress.gzip` | standalone gzip compress/decompress, no archive container (D-CODECS1) |
| `core.compress.zstd` | standalone zstd compress/decompress, no archive container (D-CODECS1) |
| `core.db` | SQLite — parameterized `DbConnection.query`/`.query_one`/`.execute`/`.begin`/`.commit`/`.rollback`/`.close` via rusqlite bundled (D-DBDRIVER1) |

---

## Writing Core in Jet (future)

Today, Core lives in the compiler as typed signatures plus Rust prelude templates
(`Source/Prelude/Std.rs`). The **API** is Jet; the **implementation** is Rust until
the package system fully stabilizes.

---

## Examples in this repo

| Example | Shows |
|---------|-------|
| `examples/features/io/files.jet` | Read, transform, write with errors |
| `examples/features/serde/json.jet` | Parse, inspect, mutate, re-render JSON |
| `examples/features/io/cli.jet` | Args, environment, exit codes |
| `examples/features/io/cli_args.jet` | `core.args` — flag/option/positional spec + parse |
| `examples/features/io/dir_entry.jet` | `fs.list_dir` → `[DirEntry]` |
| `examples/features/serde/serde_derive.jet` | `@[Codable]` encode + typed `decode<T>` with `#[Rename]` |
| `examples/features/serde/csv_typed.jet` | `csv.decode<Row>` → struct → JSON (the typed CSV pipeline) |
| `examples/features/serde/json_typed.jet` | Nested struct + list + optional round-trip with `#[RenameAll(camel)]` |

Run the full battery: `nix develop -c cargo test --test golden` and `nix develop -c cargo test --test corelib`.
