# M10 — Standard library

**Decisions:** S51 (std import spelling; amended 2026-06-13 for canonical
`jet.std`), S54 (no naming lint), and SL1-SL10 ratified. Depends on M4
(errors), M5 (collections), M8 (closures), M9 (generics for signatures like
`max[T: Comparable]`).
**Error codes:** E1001+.

**Open owner call:** S16 still lacks an import-cycle policy. M10 can build
without changing that rule, but before module work expands further we should
ratify either "cycles are errors" or the alternative explicitly.

## Goal

Enough batteries to rewrite real CLI tools: files, stdin/args/env,
process control, math, time, random, JSON. This is the small frozen core
std ratified by SL1; networking, regex, CSV/TOML, calendar time, crypto,
archives, and sqlite belong to the first-party ring after packages exist.

M10 std is implemented as compiler-known modules backed by Rust std in the
generated prelude (no Jet-source stdlib yet — canonical package identity
`jet.std` exists conceptually, but package delivery waits for M12). Every
fallible operation returns `T ? E` with a small per-domain error enum;
nothing panics except programmer errors. Error enums are designed for the
future SL6 declared-conversion story, but M10 does not implement that
conversion surface yet.

## Surface (ratified S51 + SL2)

```jet
import std.fs as fs;
import jet.std as stdlib;
import std.io as io;
import std.json as json;

fn main() {
    val args = io.args();                       // [String]
    val path = args.get(1) or panic("usage: tool <file>");

    val text = fs.read(path) or return;         // String or IOError
    fs.write("out.txt", text.to_upper()) or panic("can't write");

    val name = io.input("your name? ");         // String or IOError
    val data = json.parse(text) or return;      // JSON or JSONError
}
```

`std` is the reserved short spelling for canonical package `jet.std`.
Both spellings are valid:

```jet
import std;             // short spelling, namespace std
import jet.std as std;  // explicit canonical spelling
import std.fs as fs;
import jet.std.fs as fs;
```

These are **module imports** (S16 — no quotes). Dot paths select
compiler-known submodules. Contrast: `import "./lib"` is a **file import**
(quoted path to a `.jet` file). Unknown std module → E1001 listing the real
ones. A local module named `std`, a local module named `jet`, or a local
module that collides with a reserved first-party short name → E1002. M10
only implements `std`/`jet.std`, but reserves the names SL2 requires before
M12: `std`, `jet`, `http`, `regex`, `csv`, `toml`, `crypto`, `archive`.

Selective imports are rejected by SL3: no `import std.math { clamp }`, no
`from std.math import clamp`. Use aliases and qualified access:
`import std.math as math; math.clamp(x, lo, hi)`.

## Module inventory (exact v1 API — implement all, nothing more)

All paths are `String` in M10 (SL7). There is no `Path` type and no
`std.path` helper module in this milestone.

**std/fs** — `read(path) -> String or IOError` ·
`read_bytes(path) -> [U8] or IOError` · `write(path, text) -> ()
or IOError` · `append(path, text)` · `exists(path) -> Bool` ·
`remove(path)` · `list_dir(path) -> [String] or IOError` ·
`create_dir(path)` · `is_dir(path) -> Bool` · `copy(from, to)` ·
`rename(from, to)`. `enum IOError { NotFound(path: String);
PermissionDenied(path: String); Other(message: String); }`

**std/io** — `args() -> [String]` · `input([prompt]) -> String or
IOError` (reads a line, strips newline) · `read_all_input() -> String or
IOError` (stdin to EOF) · `eprint(value)` (stderr twin of `print`).

**std/env** — `get(name) -> (String?)` · `set(name, value)` ·
`current_dir() -> String ? IOError` · `home_dir() -> (String?)`.

**std/process** — `exit(code)` (no return) · `run(cmd: [String]) ->
ProcessResult or IOError` where
`struct ProcessResult { code: Int; output: String; errors: String; }`.

**std/math** — `sqrt` `pow` `abs` (Int+Float overloads via two names if
needed: `abs`/`fabs` is BANNED — use generic `[T: Numeric]` internal
bound) · `min[T: Comparable](a, b)` · `max[T: Comparable]` · `floor`
`ceil` `round -> Int` · constants `pi`, `e` · `clamp(x, lo, hi)`.

**std/random** — `int(low, high) -> Int` (inclusive, S22) · `float() ->
Float` (0..1) · `pick<T>(xs: [T]) -> (T?)` · `shuffle<T>(mut xs)` ·
`seed(n)`. Backed by a tiny PRNG written in the prelude (xoshiro256++)
— deterministic under `seed`, no external crate (I6).

**std/time** — `now() -> Int` (unix millis) · `sleep(millis)` ·
`Stopwatch` struct (`start()`, `elapsed_millis()`). No dates, calendar
types, timezone conversion, or formatting in M10 (SL8).

**std/json** — `enum JSON { Null; Boolean(b: Bool); Number(n: Float);
Text(s: String); Array(items: [JSON]); Object(entries: [String, JSON]); }`
· `parse(text) -> JSON or JSONError` · `render(j) -> String`
· `render_pretty(j) -> String`. Parser hand-written in the prelude
(recursive descent, ~200 lines) — also the flagship proof that Jet's
own data types model real-world data. `JSONError { line, message }`.
M10 is dynamic JSON only; typed JSON lands later via the S55 derive
direction. When typed JSON lands, unknown fields are errors by default with
an explicit tolerant-parsing opt-out (SL10).

**Binary data** (S42): `U8` is the 8-bit unsigned sized type; std binary
APIs use `[U8]` with range checks at literals (E1003 "a U8 holds
0..255"); `b.to_int()`, `n.to_u8()` checked at runtime;
`String.bytes() -> [U8]` and `String.from_bytes([U8]) -> String or
UTF8Error` land here.

## Rules & sema notes

1. Std modules are namespaces in sema with fixed signatures (declared in
   a Rust table, like today's builtins) — calls typecheck exactly like
   user functions; did-you-mean works across a module's items (E1004).
2. Tiny prelude (SL4): M10 adds no bare `input`, `min`, `max`, `read`,
   `abs`, etc. `print` and core types/methods remain the zero-import story;
   std library functions require imports.
3. Method/function review rule (SL4): libraries may choose dot methods or
   module functions based on the operation. Methods fit one obvious receiver;
   module functions fit domain services, side effects, no single receiver,
   or symmetric arguments. M10 core std should avoid duplicate
   method+function pairs unless a specific API has a strong reason.
4. Std API style sheet (SL5):
   - argument order is subject first, then what is done to it;
   - names are full words unless an already-ratified prelude/core name says
     otherwise;
   - every fallible call returns `T ? E`; no panicking twins in core;
   - no abbreviated module names (`random`, not `rand`);
   - verbs are actions, nouns are values, bool functions ask questions;
   - one canonical spelling per task in M10 core std by default.
5. No naming lint (S54): Jet does not prescribe snake_case or other
   naming conventions in v1 — `jet fmt` handles layout only (S44).
6. No global state: `std/random`'s default generator is a thread-local
   seeded from time; document determinism story honestly.
7. All blocking calls (`input`, `sleep`, `run`) are fine in v1 (no async
   — non-goal).

## Codegen

Each module's functions become prelude helpers over Rust std
(`std::fs::read_to_string` etc.), mapping errors into the Jet enums.
JSON/PRNG are pure-Rust code in the prelude template. The prelude
becomes a separate generated module; keep it under `src/prelude/` as
`.rs` template files included with `include_str!` so it's reviewable
Rust, not string soup in codegen.rs.

Pay-for-what-you-call is an M10 invariant (SL9): codegen emits a std helper
only if sema proves the program can call it. Importing a module is free;
only calls cost generated Rust/binary size. This should become architecture
rule R10 in docs/03.

## Diagnostics to register

E1001 unknown std module (lists all) · E1002 local module shadows reserved
first-party root/name (`std`, `jet`, later ring short names) · E1003 U8
literal out of range · E1004 unknown item in module (suggestion) ·
selective import syntax teaching error pointing back to qualified imports.
Teaching: E0037 `println!`/`eprintln!` → `print`/`io.eprint` · E0038
`open(`/`File::open` → `fs.read` · E0039 `os.environ`/`getenv` →
`env.get`.

## Examples & tests

- `examples/29_files.jet` — read/transform/write with error handling.
- `examples/30_json.jet` — parse, walk, mutate, re-render JSON.
- `examples/31_cli.jet` — args + env + exit codes (a real mini-tool).
- Golden tests use tempdirs; `std/time`/`std/random` examples pin output
  via `seed` and injected clock (the prelude reads `LEX_TEST_EPOCH` env
  var when set — test hook, documented as such).
- ui fixtures for E10xx + teaching errors.
- Import tests cover both `import std.fs as fs` and
  `import jet.std.fs as fs`; both resolve to the same compiler-known module.
- Size-regression tests enforce SL9: `01_hello.jet --small` stays under a
  pinned byte budget, and a fixture importing all M10 std modules but
  calling nothing stays within noise of hello-world.
- User-facing std reference: **docs/stdlib.md** (example-first tour of every module).

## First-party ring (post-M10, SL1/SL2 ratified)

Core std stays the M10 eight modules. The ring ships as versioned
`jet.*` packages with reserved short import names. Build order
(adoption priority):

| Order | Package | Unlocks |
|---|---|---|
| 1 | `jet.http` (client) | API calls — blocking on streaming I/O + SL6 error conversion |
| 2 | `jet.regex` | grep-class tools, validation |
| 3 | `jet.csv` + `jet.toml` | data files, configs |
| 4 | `jet.http` (server) | small services — after tasks (v2) |
| 5 | `jet.time` (calendar) | dates/timezones (SL8 constraints) |
| 6 | `jet.crypto` | hash/random/hmac — vetted primitives only |
| 7 | `jet.archive` | zip/tar/gzip |
| 8 | `jet.db` (sqlite) | FFI-tier (M7 machinery) |

Package delivery and `import http#0.8.1 as http` resolution land in M12.2
(see docs/plans/epoch-1/m12-packages.md, SL2). Everything below this line is
community-package territory.

## Out of scope

Networking, regex, CSV/TOML, calendar/timezone library, crypto, archives,
sqlite, package version syntax at import sites, package overrides, custom
std providers, and lockfile resolution are M12/ring or post-v1 work. Paths
stay `String`; `std.path` string helpers are post-M10. Date formatting/
timezones are post-v1 ring work; M10 stays unix millis only. File handles/
streaming are out of scope (whole-file reads only — `read_bytes` covers
big-ish files). Threads are M11 (deferred v2 per S53).
