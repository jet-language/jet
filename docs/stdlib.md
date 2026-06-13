# Standard library (`jet.std`)

The Jet standard library gives you files, terminal I/O, environment variables,
process control, math, time, random numbers, and JSON — enough to write real
command-line tools. Every fallible call returns a `Result`; nothing in core std
panics on its own.

**How it works today:** std modules are built into the compiler. Import them by
name; the compiler type-checks your calls and generates only the helpers you
actually use (see [Imports](#imports) and [Pay for what you call](#pay-for-what-you-call)).

**Canonical name:** `jet.std`. The short spelling `std` is reserved and means the
same thing.

---

## Quick start

```jet
import std.fs as fs;
import std.io as io;
import std.env as env;

fn main() {
    val args = io.args();
    if args.len() < 2 {
        io.eprint("usage: greet <name>");
        return;
    }
    val name = args.get(1) or return;
    val greeting = env.get("GREETING") or "hello";
    fs.write("/tmp/greet.txt", "{greeting}, {name}!") or return;
    print(fs.read("/tmp/greet.txt") or return);
}
```

Build and run (extra words after the file become program arguments):

```bash
jet run tool.jet World
# or: jet build tool.jet && ./build/tool World
```

---

## Imports

Std uses **module imports** — no quotes, unlike file imports.

```jet
import std.fs as fs;           // one submodule
import jet.std.json as json;   // same module, canonical spelling
```

Both `import std.fs` and `import jet.std.fs` resolve to the same compiler-known
module.

**Not allowed:**

```jet
import std.math { clamp };     // selective imports — use an alias instead
import "std/fs";               // quoted paths are for .jet files only
```

If you name a local file or folder `std`, `jet`, `http`, `regex`, `csv`, `toml`,
`crypto`, or `archive`, the compiler rejects it — those names are reserved for
first-party packages.

---

## Errors and results

Fallible std functions return `Result<T, E>`. Handle them like any other Jet
result — with `?`, `or`, or a pattern test:

```jet
import std.fs as fs;

fn main() {
    val text = fs.read("data.txt") or return;   // stop on error
    val upper = text.to_upper();
    fs.write("out.txt", upper) or panic("couldn't save");  // bug if this fails
}
```

Each module has a small error type (`IoError`, `JsonError`, …). There is no
automatic conversion between error types in v1.

---

## Pay for what you call

Importing `std.fs` costs nothing in the generated binary until you **call**
something from it. A program that imports every std module but only calls
`print` stays hello-world sized. Only the helpers your program can reach are
compiled in.

---

## Modules

### `std.fs` — files and folders

Whole-file helpers only (no open file handles in v1). Paths are plain
`String`s.

```jet
import std.fs as fs;

fn main() {
    val path = "/tmp/notes.txt";
    fs.write(path, "hello\n") or return;
    fs.append(path, "world\n") or return;
    print(fs.read(path) or return);        // "hello\nworld\n"
    print(fs.exists(path));                // true
    print(fs.is_dir("/tmp"));              // true
    val names = fs.list_dir("/tmp") or return;
    print(names.len());
}
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `read(path)` | `String or IoError` | Read entire file as UTF-8 text |
| `read_bytes(path)` | `List<U8> or IoError` | Read entire file as bytes |
| `write(path, text)` | `() or IoError` | Create or overwrite a text file |
| `append(path, text)` | `() or IoError` | Append text to a file |
| `exists(path)` | `Bool` | Whether the path exists |
| `remove(path)` | `() or IoError` | Delete a file |
| `list_dir(path)` | `List<String> or IoError` | Names in a directory |
| `create_dir(path)` | `() or IoError` | Create a directory |
| `is_dir(path)` | `Bool` | Whether the path is a directory |
| `copy(from, to)` | `() or IoError` | Copy a file |
| `rename(from, to)` | `() or IoError` | Rename or move a file |

**`IoError`** — `NotFound(path)`, `PermissionDenied(path)`, or `Other(message)`.

---

### `std.io` — terminal and arguments

```jet
import std.io as io;

fn main() {
    val args = io.args();                    // List<String>; index 0 is the program name
    val name = io.input("your name? ") or return;  // reads one line, strips newline
    print("hi, {name}");
    io.eprint("(log) done");                 // like print, but to stderr
}
```

Pipe input for scripts:

```bash
printf "Ada\n" | jet run ask.jet
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `args()` | `List<String>` | Command-line arguments |
| `input([prompt])` | `String or IoError` | Read one line from stdin; optional prompt |
| `read_all_input()` | `String or IoError` | Read all of stdin to end-of-file |
| `eprint(value)` | nothing | Print to stderr (any printable value) |

`print` stays in the core prelude (no import). Use `io.eprint` for stderr.

---

### `std.env` — environment and working directory

```jet
import std.env as env;

fn main() {
    val home = env.home_dir();               // String? — may be null
    val mode = env.get("MODE") or "dev";     // String? from the environment
    env.set("MODE", "prod");                 // set for child processes
    val here = env.current_dir() or return;  // current working directory
    print(home or "(no home)");
    print(mode);
    print(here);
}
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `get(name)` | `String?` | Environment variable, or null if unset |
| `set(name, value)` | nothing | Set an environment variable |
| `current_dir()` | `String or IoError` | Current working directory |
| `home_dir()` | `String?` | User home directory, if known |

---

### `std.process` — exit and subprocesses

```jet
import std.process as process;

fn main() {
    val result = process.run(["echo", "hi"]) or return;
    print(result.code);       // exit code as Int
    print(result.output);     // stdout as String
    print(result.errors);     // stderr as String
    process.exit(0);          // end the program with an exit code (never returns)
}
```

| Function | Returns | What it does |
|----------|---------|--------------|
| `exit(code)` | never | Stop the program with the given exit code |
| `run(cmd)` | `ProcessResult or IoError` | Run a command; `cmd` is `List<String>` |

**`ProcessResult`** — `code: Int`, `output: String`, `errors: String`.

---

### `std.math` — numbers

```jet
import std.math as math;

fn main() {
    print(math.sqrt(2.0));
    print(math.pow(2.0, 10.0));
    print(math.abs(-3));                     // works on Int and Float
    print(math.min(3, 7));                   // generic over Comparable types
    print(math.max(3.5, 7.2));
    print(math.floor(3.9));
    print(math.ceil(3.1));
    print(math.round(3.6));                  // returns Int
    print(math.clamp(15, 0, 10));            // 10
    print(math.pi);
    print(math.e);
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

### `std.random` — random numbers

```jet
import std.random as random;

fn main() {
    random.seed(42);                         // make the sequence repeatable
    print(random.int(1, 6));                 // inclusive range (like dice)
    print(random.float());                   // 0.0 .. 1.0
    val items = [10, 20, 30];
    print(random.pick(items));               // one item, or null if list empty
    random.shuffle(mut items);               // shuffle in place
    print(items);
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

---

### `std.time` — clock and delays

Time in v1 is **Unix milliseconds** only — no dates, time zones, or formatting.

```jet
import std.time as time;

fn main() {
    val started = time.now();                // milliseconds since 1970-01-01 UTC
    time.sleep(100);                         // pause ~100 ms (blocking)
    val sw = time.start();                   // Stopwatch
    time.sleep(50);
    print(sw.elapsed_millis());              // at least 50
    print(time.now() - started);
}
```

| Function / type | Returns | What it does |
|-----------------|---------|--------------|
| `now()` | `Int` | Current Unix time in milliseconds |
| `sleep(millis)` | nothing | Block for about `millis` milliseconds |
| `time.start()` | `Stopwatch` | Start a stopwatch |
| `sw.elapsed_millis()` | `Int` | Milliseconds since `time.start()` |

**Test hook:** when the environment variable `LEX_TEST_EPOCH` is set to an
integer, `time.now()` returns that value instead of the real clock. Tests use
this to pin output; normal programs ignore it.

---

### `std.json` — parse and print JSON

Dynamic JSON — you walk a `Json` enum by hand. Typed JSON structs come later.

```jet
import std.json as json;

fn main() {
    val raw = "{{\"name\":\"jet\",\"ok\":true,\"n\":1.5}}";
    val data = json.parse(raw) or return;
    print(json.render(data));                 // compact one line
    print(json.render_pretty(data));           // indented

    if data == Object(entries) {
        if entries.contains("name") {
            print(entries["name"]);
        }
    }
}
```

**`Json` variants:** `Null`, `Boolean(b)`, `Number(n)`, `Text(s)`,
`Array(items)`, `Object(entries: Map<String, Json>)`.

| Function | Returns | What it does |
|----------|---------|--------------|
| `parse(text)` | `Json or JsonError` | Parse a JSON string |
| `render(j)` | `String` | Compact JSON text |
| `render_pretty(j)` | `String` | Indented JSON text |

**`JsonError`** — `line` and `message` pointing at the parse failure.

---

## Binary data (`U8`)

The `U8` type holds one byte (0–255). Literals outside that range are a compile
error (**E1003**).

```jet
fn main() {
    val b: U8 = 255;
    print(b.to_int());                       // 255 as Int
    val n = 42.to_u8() or return;            // checked conversion
    val bytes = "hi".bytes();                // List<U8>
    val text = String.from_bytes(bytes) or return;
    print(text);
}
```

| API | Returns | What it does |
|-----|---------|--------------|
| `String.bytes()` | `List<U8>` | UTF-8 bytes of a string |
| `String.from_bytes(bs)` | `String or Utf8Error` | Decode UTF-8 bytes |
| `n.to_u8()` | `U8 or String` | Checked Int → U8 |
| `b.to_int()` | `Int` | U8 → Int |

Use `fs.read_bytes` / `fs.write` when you need raw file bytes.

---

## Common mistakes (and what Jet suggests)

| You wrote | Jet wants |
|-----------|-----------|
| `println(...)` | `print(...)` |
| `eprintln(...)` | `io.eprint(...)` |
| `open("file")` / `File.open` | `fs.read(...)` / `fs.write(...)` |
| `getenv("X")` / `os.environ` | `env.get("X")` |

---

## What's not in v1 std

These are intentionally out of scope for the frozen core; they may appear as
first-party packages later:

- Networking and HTTP
- Regular expressions, CSV, TOML
- Calendar dates, time zones, formatted dates
- Crypto, archives, SQLite
- Open file handles and streaming I/O (whole-file reads only)
- Async / threads (concurrency is v2)

See `docs/plans/m10-stdlib.md` for the exact frozen API and `docs/stdlib-decisions.md`
for design rationale.

---

## Writing std in Jet (future)

Today, std lives in the compiler as typed signatures plus Rust prelude templates
(`src/prelude/std.rs`). The **API** is Jet; the **implementation** is Rust until
packages land.

To ship a Jet-source standard library, these prerequisites are still open:

| Prerequisite | Milestone | Why it's needed |
|--------------|-----------|-----------------|
| Package manager + store | **M12** | `jet.std` must install as a real package (`jet.toml`, lockfile, content-addressed store) |
| Toolchain-selected `jet.std` | **M12 + SL2** | Compiler picks one std version; imports resolve through the package graph |
| OS boundary story | **M7 (partial)** | Files, processes, and env ultimately need the OS — either keep thin `extern rust` shims or expand FFI |
| Error conversion for `?` | post-v1 | Multi-module std wants propagating across error enums without boilerplate |
| Streaming I/O | post-v1 | Jet-source std can't rely on whole-file helpers forever |

**What Jet can already express:** JSON parsing logic, PRNG algorithms, math
helpers, and data structures — the language has structs, enums, generics,
traits, closures, and `Result`. **What still needs a native bridge:** syscalls
(read/write files, spawn processes, sleep, environment).

The likely migration path: M12 delivers `jet.std` as a bundled first-party
package; pure Jet modules replace Rust where possible; a small audited native
layer stays for I/O and process control until Jet has its own OS interface.

---

## Examples in this repo

| Example | Shows |
|---------|-------|
| `examples/29_files.jet` | Read, transform, write with errors |
| `examples/30_json.jet` | Parse, inspect, mutate, re-render JSON |
| `examples/31_cli.jet` | Args, environment, exit codes |

Run the full battery: `cargo test --test golden` and `cargo test --test stdlib`.
