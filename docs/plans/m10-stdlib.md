# M10 — Standard library

**Blocked on decisions:** S51 (std import spelling), S42 (numeric types —
the `Byte` half), S54 (naming convention lint). Depends on M4 (errors),
M5 (collections), M8 (closures), M9 (generics for signatures like
`max[T: Comparable]`).
**Error codes:** E1001+.

## Goal

Enough batteries to rewrite real CLI tools: files, stdin/args/env,
process control, math, time, random, JSON. Implemented as compiler-known
modules backed by Rust std in the generated prelude (no Jet-source
stdlib yet — that wants packages, M12). Every fallible operation returns
`T or E` with a small set of well-named error enums; nothing panics
except programmer errors.

## Surface (uses ballot recommendations — substitute ratified choices)

```jet
import "std/fs" as fs;
import "std/io" as io;
import "std/json" as json;

fn main() {
    val args = io.args();                       // List[String]
    val path = args.get(1) or panic("usage: tool <file>");

    val text = fs.read(path) or return;         // String or IoError
    fs.write("out.txt", text.to_upper()) or panic("can't write");

    val name = io.input("your name? ");         // String or IoError
    val data = json.parse(text) or return;      // Json or JsonError
}
```

`import "std/<module>" as <alias>;` reuses M6's import machinery with
the reserved `std/` prefix (S51). Unknown std module → E1001 listing
the real ones. Shadowing `std/` with a local file → E1002.

## Module inventory (exact v1 API — implement all, nothing more)

**std/fs** — `read(path) -> String or IoError` ·
`read_bytes(path) -> List[Byte] or IoError` · `write(path, text) -> ()
or IoError` · `append(path, text)` · `exists(path) -> Bool` ·
`remove(path)` · `list_dir(path) -> List[String] or IoError` ·
`create_dir(path)` · `is_dir(path) -> Bool` · `copy(from, to)` ·
`rename(from, to)`. `enum IoError { NotFound(path: String);
PermissionDenied(path: String); Other(message: String); }`

**std/io** — `args() -> List[String]` · `input([prompt]) -> String or
IoError` (reads a line, strips newline) · `read_all_input() -> String or
IoError` (stdin to EOF) · `eprint(value)` (stderr twin of `print`).

**std/env** — `get(name) -> String?` · `set(name, value)` ·
`current_dir() -> String or IoError` · `home_dir() -> String?`.

**std/process** — `exit(code)` (no return) · `run(cmd: List[String]) ->
ProcessResult or IoError` where
`struct ProcessResult { code: Int; output: String; errors: String; }`.

**std/math** — `sqrt` `pow` `abs` (Int+Float overloads via two names if
needed: `abs`/`fabs` is BANNED — use generic `[T: Numeric]` internal
bound) · `min[T: Comparable](a, b)` · `max[T: Comparable]` · `floor`
`ceil` `round -> Int` · constants `pi`, `e` · `clamp(x, lo, hi)`.

**std/random** — `int(low, high) -> Int` (inclusive, S22) · `float() ->
Float` (0..1) · `pick[T](xs: List[T]) -> T?` · `shuffle[T](mut xs)` ·
`seed(n)`. Backed by a tiny PRNG written in the prelude (xoshiro256++)
— deterministic under `seed`, no external crate (I6).

**std/time** — `now() -> Int` (unix millis) · `sleep(millis)` ·
`Stopwatch` struct (`start()`, `elapsed_millis()`).

**std/json** — `enum Json { Null; Boolean(b: Bool); Number(n: Float);
Text(s: String); Array(items: List[Json]); Object(entries: Map[String,
Json]); }` · `parse(text) -> Json or JsonError` · `render(j) -> String`
· `render_pretty(j) -> String`. Parser hand-written in the prelude
(recursive descent, ~200 lines) — also the flagship proof that Jet's
own data types model real-world data. `JsonError { line, message }`.

**Byte** (S42): new scalar type = u8; arithmetic like Int with range
checks at literals (E1003 "a Byte holds 0..255"); `b.to_int()`,
`Int.to_byte()` checked at runtime; `String.bytes() -> List[Byte]` and
`String.from_bytes(List[Byte]) -> String or Utf8Error` land here.

## Rules & sema notes

1. Std modules are namespaces in sema with fixed signatures (declared in
   a Rust table, like today's builtins) — calls typecheck exactly like
   user functions; did-you-mean works across a module's items (E1004).
2. Naming lint (S54): identifiers should be snake_case; L1001 fires on
   camelCase/PascalCase names with the rename — warning, not error, and
   `jet fmt` does NOT auto-rename (behavior changes are never silent).
3. No global state: `std/random`'s default generator is a thread-local
   seeded from time; document determinism story honestly.
4. All blocking calls (`input`, `sleep`, `run`) are fine in v1 (no async
   — non-goal).

## Codegen

Each module's functions become prelude helpers over Rust std
(`std::fs::read_to_string` etc.), mapping errors into the Jet enums.
JSON/PRNG are pure-Rust code in the prelude template. The prelude
becomes a separate generated module; keep it under `src/prelude/` as
`.rs` template files included with `include_str!` so it's reviewable
Rust, not string soup in codegen.rs.

## Diagnostics to register

E1001 unknown std module (lists all) · E1002 local file shadows `std/` ·
E1003 Byte literal out of range · E1004 unknown item in module
(suggestion) · L1001 non-snake_case name.
Teaching: E0037 `println!`/`eprintln!` → `print`/`io.eprint` · E0038
`open(`/`File::open` → `fs.read` · E0039 `os.environ`/`getenv` →
`env.get`.

## Examples & tests

- `examples/23_files.jet` — read/transform/write with error handling.
- `examples/24_json.jet` — parse, walk, mutate, re-render JSON.
- `examples/25_cli.jet` — args + env + exit codes (a real mini-tool).
- Golden tests use tempdirs; `std/time`/`std/random` examples pin output
  via `seed` and injected clock (the prelude reads `LEX_TEST_EPOCH` env
  var when set — test hook, documented as such).
- ui fixtures for E10xx/L1001 + teaching errors.

## Out of scope

Networking (post-v1; needs an async/blocking decision), regex, paths as
a distinct type (strings for v1), date formatting/timezones (millis
only), file handles/streaming (whole-file reads only — `read_bytes`
covers big-ish files), TOML/CSV (JSON proves the pattern; others can be
packages after M12), threads (M11).
