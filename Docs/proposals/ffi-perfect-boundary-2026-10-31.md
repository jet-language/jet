# Jet FFI, version 2: the best boundary we can build, shown by example

Status: design draft by FFIDesign-1, 2026-10-31, for owner review. It replaces
the 2026-10-30 proposal this replaces. Nothing here is ratified by being written
down. Every new construct is marked **PROPOSED**. Every remaining owner choice is a ballot in
the ballots on Tower card #6366. Every remaining cost is in `ffi-perfect-boundary-downsides-2026-10-31.md`.

## 1. Summary

You should be able to use any C, C++, Rust, Python or JavaScript library from Jet:

- with one line;
- with no glue code and no `#Unsafe`;
- with Jet errors, Jet strings, Jet lists, and handles that close themselves.

You should be able to hand any Jet library to those languages the same way, and run a library
you do not trust in a sandbox by changing one option. When something goes wrong, you get one
backtrace across every language. You also get a trace of every crossing, and a build or load
error that names the changed function instead of a crash.

Four ideas get there.

1. **Facts, not guesses.** Every boundary function has a set of facts:
   - who frees a result;
   - whether a pointer may be null;
   - how long a borrowed pointer lives;
   - which thread may call it;
   - how it fails.

   Jet takes these from the header, from Clang attributes, and from a shared **pack** of facts
   per library version on Jetpack (PROPOSED, ballot D-FFI-PACK1). Missing facts get a safe
   shape, never a guess.
2. **Jet's own rules do the checking.** A borrowed C pointer becomes a Jet view tied to its
   owner, so "use after `sqlite3_step`" is an ordinary borrow error. A handle is a `Close`
   resource, so it closes at scope end. A status code goes through a visible Jet function named
   by `#Check(fn)` (PROPOSED), so it becomes a Jet error. No new safety system is invented.
3. **One ladder for experts.** These rungs show or control the same plan, from easiest to most
   manual:
   - the generated wrapper;
   - `jet inspect bind` to see it;
   - `--shape native` to get the raw shape;
   - `--freeze` to pin it;
   - the same steps written by hand with `core.ffi` (D-FFI-PRIMS1).

   Nothing in a wrapper is private compiler magic.
4. **Placement is one option.** A library runs in the program, contained, in a wasm sandbox, or
   in a child process. The choice is one line in `package.jet`, and call sites never change
   (ballot D-FFI-ISOLATE1).

### How to read the gallery

Each entry has five parts:

- **What you write**: the complete program.
- **What you run**: the command line.
- **What you see**: the program output, and any diagnostics.
- **What Jet generated**: the relevant `jet inspect bind` lines.
- **The best competitor's version**: the strongest other language for that task.

A status line says how much exists today:

- **EXISTS**: today's compiler runs it.
- **PARTIAL**: some rows run today, and the rest is named.
- **PROPOSED**: new design.

**Outputs are design targets.** The output blocks show what the design promises. They are not
captured runs, except where an entry says EXISTS and cites the example it was taken from.
Diagnostic codes from E3224 up are new and PROPOSED. Lower codes are today's codes, with today's
meanings.

---

## 2. Gallery

### G1. One C function from a system header

The simplest case: one header, one call.

```jet
// cosine.jet
package {
    name: "cosine"
    authority: { allow: [FFI, IO] }
}

use m :: "math.h"

fn run() {
    print(m.cos(0.0))
    print(m.hypot(3.0, 4.0))
}
```

```text
$ jet run cosine.jet
1.0
5.0
```

```text
$ jet inspect bind "math.h" --row cos --row hypot
c.m   header /usr/include/math.h (sha256 41c0…)   pack jetpack:c/libm@2.39 (reviewed)
  fn cos(x: Float) -[]> Float     = "cos"     read: double cos(double)  effects: none [pack]
  fn hypot(x: Float, y: Float) -[]> Float = "hypot"  effects: none [pack]
  link: m (pkg-config fallback: libm)
```

Competitor: **Zig `@cImport`**, the best one-line C import today.

```zig
const std = @import("std");
const c = @cImport(@cInclude("math.h"));

pub fn main() void {
    std.debug.print("{d}\n{d}\n", .{ c.cos(0.0), c.hypot(3.0, 4.0) });
}
// zig run cosine.zig -lc  ->  1  5
```

Where Jet is ahead: the call is pure (`-[]>`) because the libm pack says so. That means `cos` can
be called from a pure function, and from comptime (G17). In Zig it is an opaque extern call.

Status: **PARTIAL**. `use m :: "x.h"` and scalar calls exist: `Examples/features/lowlevel/ffi.jet`
and `cbind/`, with the hand-written header parser in
`Jetpack/PackageModel/Source/Foreign/C/HeaderParser.jet`. D-CIMPORT1's pinned Clang reader, the
libm pack, and pure effect facts are PROPOSED or unbuilt. The CINPUT1 ballot recorded E0102 for a
system header today.

---

### G2. SQLite in one line, used idiomatically

The ask: bind sqlite in one line, then use it as Jet code. Errors are Jet errors, handles close
themselves, and text and rows are safe.

```jet
// notes.jet
package {
    name: "notes"
    authority: { allow: [FFI, IO, FS, Mem.Alloc] }
    deps: { sqlite3: c@nixpkgs:sqlite }
}

use sql :: c.sqlite3

struct Note {
    id: Int
    body: String
}

fn load(db: sql.Database) -> [Note] sql.SqliteError! {
    notes := [Note]{}
    stmt := db.prepare("select id, body from notes order by id")?   // `:=`: step() writes it
    loop &stmt.step()? {                                             // `&` marks the written receiver
        &notes.push(Note{id: stmt.column_int(0), body: stmt.column_text(1).copy()})
    }
    Ok(notes)
}

fn run() {
    db :: sql.open("notes.db")?
    db.exec("create table if not exists notes(id integer primary key, body text)")?
    db.exec("insert into notes(body) values ('buy milk'), ('ship ffi v2')")?
    loop note in load(db)? {
        print("{note.id}: {note.body}")
    }
    if db.exec("insert into nope values (1)") == .Err(e) -> print("error: {e}")
}   // stmt closed at the end of load, db closed here
```

```text
$ jet run notes.jet
1: buy milk
2: ship ffi v2
error: SqliteError.Sql(code: 1, message: "no such table: nope")
```

The pack writes these rows once for everyone. It is an ordinary `#Import module` overlay with a
few PROPOSED markers. It is not compiler code.

```jet
// jetpack:c/sqlite3@3.46 pack, file sqlite3.jet (PROPOSED shape)
#Import module c.sqlite3 {
    #Close(sqlite3_close_v2)
    struct Database = "sqlite3"
    #Close(sqlite3_finalize)
    struct Statement = "sqlite3_stmt"

    #Check(status_open)
    fn open(path: String) -> Database = "sqlite3_open"              // int (const char*, sqlite3**)
    #Check(status)
    fn Database.exec(self, sql: String) = "sqlite3_exec"            // NULL callback, NULL errmsg
    #Check(status)
    fn Database.prepare(self, sql: String) -> Statement from self = "sqlite3_prepare_v2"
    #Check(step_status)
    fn Statement.step(&self) -> Bool = "sqlite3_step"               // SQLITE_ROW -> true, DONE -> false
    fn Statement.column_int(self, col: I32) -> Int = "sqlite3_column_int64"
    fn Statement.column_text(self, col: I32) -> View<str> from self = "sqlite3_column_text"
}

#Error
enum SqliteError {
    Sql(code: I32, message: String)
    Busy
}

fn status(code: I32, db: Database) SqliteError! {
    if code == SQLITE_OK -> return Ok()
    if code == SQLITE_BUSY -> return Err(SqliteError.Busy)
    Err(SqliteError.Sql(code: code, message: errmsg(db)))
}
```

What the pack's rows mean, all checked against `sqlite3.h` when the pack is merged:

- `open(path)` drops the trailing `sqlite3**`, which is the proved out-parameter rule
  (D-FFI-AUTO1). The `int` result goes through `status_open`, and the handle closes with
  `sqlite3_close_v2`. A failed open still closes the half-made handle.
- `struct Database = "sqlite3"` names a C opaque type with a Jet name. It is PROPOSED; today the
  binder names it from the C typedef, and `#Close(fn)` sits on the row that returns the handle.
- `fn Database.exec(...)` is an external inherent method, the spelling the spec already allows
  for methods. Here it is PROPOSED for import rows.
- `prepare` returns a `Statement` that is a view of the database (`-> Statement from self`,
  PROPOSED for a non-view handle). It cannot outlive `db`.
- `column_text` returns `View<str> from self`. `step` takes `&self`, so the caller writes
  `&stmt.step()` on a `:=` binding, as for any Jet method that writes its receiver. Holding the
  text across the next `&stmt.step()` is therefore an ordinary Jet borrow error, as shown next.

```jet
// mistake.jet: keep a row's text across step()
fn first_two(db: sql.Database) -> String sql.SqliteError! {
    stmt := db.prepare("select body from notes")?
    _ :: &stmt.step()?
    first :: stmt.column_text(0)
    _ :: &stmt.step()?
    Ok("{first} / {stmt.column_text(0)}")
}
```

```text
$ jet check mistake.jet
Error [jet.UseWhileWriteActive]: `stmt` is written by `&stmt.step()` while `first` still borrows it.
  --> mistake.jet:6:10
    |
  5 |     first :: stmt.column_text(0)
    |              ------------------- `first` is a view of `stmt` (sqlite3_column_text: valid until the next step)
  6 |     _ :: &stmt.step()?
    |          ^^^^^^^^^^^^ this write ends that view
  7 |     Ok("{first} / {stmt.column_text(0)}")
    |          ------- the view is used here
 Why: SQLite frees a row's text when the statement steps; the pack ties the view to `stmt`.
 Fix: Keep a copy before stepping: `first :: stmt.column_text(0).copy()`.
More: jet-lang.dev/e/E0204
```

Competitor: **Rust `rusqlite`**, which is about 15,000 lines of hand-written safe wrapper over
bindgen output.

```rust
use rusqlite::{Connection, Result};
struct Note { id: i64, body: String }

fn main() -> Result<()> {
    let db = Connection::open("notes.db")?;
    db.execute("create table if not exists notes(id integer primary key, body text)", [])?;
    db.execute("insert into notes(body) values ('buy milk'), ('ship ffi v2')", [])?;
    let mut stmt = db.prepare("select id, body from notes order by id")?;
    let notes = stmt.query_map([], |r| Ok(Note { id: r.get(0)?, body: r.get(1)? }))?;
    for n in notes { let n = n?; println!("{}: {}", n.id, n.body); }
    if let Err(e) = db.execute("insert into nope values (1)", []) { println!("error: {e}"); }
    Ok(())
}
```

Where Jet is ahead:

- No library author wrote 15,000 lines. The pack is about 120 lines of facts, plus `#Check`
  functions.
- The same pack serves every Jet program, and `jet inspect bind` shows every rule it applies.
- The borrow error is the same one a Jet beginner already knows.

Status: **PROPOSED**. Pieces that exist today:

- `#Close(fn)` on generated handles (`opaque_handles/.jet/bindings/c/z.jet`);
- `c@nixpkgs:` deps;
- scope-end `Close` (`Examples/features/memory/scope_close_safety_net.jet`);
- view returns with `from` for Jet functions.

Packs, `#Check`, method rows and foreign view returns are new.

---

### G3. C data shapes: struct by value, fixed array, out-parameter, nullable pointer, counted buffer

```c
// geom.h (your own library)
typedef struct { float x, y; } Vec2;
typedef struct { Vec2 corners[4]; } Quad;

Vec2  vec2_add(Vec2 a, Vec2 b);                                   // struct by value
Quad  quad_unit(void);                                            // fixed array inside a struct
int   quad_area(const Quad *q, float *out_area);                  // status plus out-param
const char *geom_name(int shape_id);                              // may return NULL
void  geom_scale(float *pts __counted_by(n), size_t n, float k);  // counted buffer
```

```jet
// shapes.jet
package {
    name: "shapes"
    authority: { allow: [FFI, IO] }
    deps: { geom: c@"native/geom" }
}

use g :: c.geom

fn run() {
    v :: g.vec2_add(g.Vec2{x: 1.0, y: 2.0}, g.Vec2{x: 0.5, y: 0.5})
    print("{v.x}, {v.y}")

    q :: g.quad_unit()
    print("corner 2 = {q.corners[2].x}, {q.corners[2].y}")

    area :: g.quad_area(q) ?? -1.0
    print("area {area}")

    print(g.geom_name(3) ?? "unknown shape")

    pts := [F32]{1.0, 2.0, 3.0}
    g.geom_scale(&pts, 10.0)
    print(pts)
}
```

```text
$ jet run shapes
1.5, 2.5
corner 2 = 1.0, 1.0
area 1.0
unknown shape
[10.0, 20.0, 30.0]
```

```text
$ jet inspect bind geom
c.geom   source native/geom/geom.h + geom.c (read by pinned clang 19.1, target x86_64-linux-gnu)
  #Layout(c) struct Vec2 { x: F32, y: F32 }                       size 8 align 4  [header, checked vs DWARF]
  #Layout(c) struct Quad { corners: [Vec2; 4] }                   size 32 align 4 [header, checked vs DWARF]
  fn vec2_add(a: Vec2, b: Vec2) -> Vec2                           by value [header]
  fn quad_unit() -> Quad                                          by value [header]
  fn quad_area(q: Quad) -> F32 GeomStatus!                        rule out-param->return; q passed by address [header]
                                                                  status: nonzero = GeomStatus.Code(n) [default rule]
  fn geom_name(shape_id: I32) -> String?                          NULL -> None (D-FFI-CMAP1); text borrowed, copied [header]
  fn geom_scale(pts: &[F32], k: F32)                              counted_by(n): n = pts.len() [header]
```

Notes:

- `quad_area` takes `const Quad*`. A `const T*` parameter is a read borrow, so the call passes
  `q`'s address with no copy and no sigil.
- The status-plus-out-parameter rule applies with no pack, because the shape is unambiguous: an
  `int` result plus a trailing pointer to a scalar. The binder never invents what a nonzero
  status means beyond a typed code. A pack or a `#Check(fn)` row can name it.
- `geom_name` returns `const char*`, which is borrowed by default. Jet copies it and frees
  nothing. That is correct here, because the strings are static.

Competitor: **C# `LibraryImport`**, the strongest managed-language source generator.

```csharp
[StructLayout(LayoutKind.Sequential)] struct Vec2 { public float X, Y; }
[StructLayout(LayoutKind.Sequential)] unsafe struct Quad { public fixed float Corners[8]; }

static partial class Geom {
    [LibraryImport("geom")] public static partial Vec2 vec2_add(Vec2 a, Vec2 b);
    [LibraryImport("geom")] public static partial Quad quad_unit();
    [LibraryImport("geom")] public static partial int quad_area(in Quad q, out float area);
    [LibraryImport("geom", StringMarshalling = StringMarshalling.Utf8)]
    public static partial string? geom_name(int id);           // frees with CoTaskMemFree: wrong here
    [LibraryImport("geom")] public static partial void geom_scale(Span<float> pts, nuint n, float k);
}
// The user writes every line above by hand; nothing checks it against geom.h.
```

Where Jet is ahead:

- Nothing is written by hand.
- The layouts are checked against the library's debug info.
- The string-ownership mistake C# makes by default (freeing a static string) cannot happen. Jet's
  default for an unannotated `const char*` is copy-and-keep, and W3230 says so.

Status: **PARTIAL**. Today's native lowering in `Compiler/JetIR/Source/BuildForeign.jet` passes:

- scalars;
- `String` (copied);
- `&` scalars;
- `&` structs of machine scalars.

Struct-by-value returns, fixed arrays, `counted_by`, the out-parameter rule and DWARF checks are
PROPOSED. D-FFI-CMAP1 ratified the `T?` and list mapping.

---

### G4. Callbacks with context: `qsort` with a capturing closure, and an event loop on its own thread

```jet
// sort.jet
package {
    name: "sort"
    authority: { allow: [FFI, IO] }
}

use libc :: "stdlib.h"

struct Row {
    name: String
    score: I32
}

fn run() {
    rows := [Row]{Row{name: "ada", score: 7}, Row{name: "bo", score: 9}, Row{name: "cy", score: 5}}
    descending :: true
    compares := 0
    libc.qsort(&rows, (a, b) -> {
        compares += 1
        if descending -> b.score <=> a.score else -> a.score <=> b.score
    })
    loop r in rows -> print("{r.name} {r.score}")
    print("compares {compares}")
}
```

```text
$ jet run sort.jet
bo 9
ada 7
cy 5
compares 3
```

Here `qsort(void *base, size_t n, size_t size, int (*cmp)(const void*, const void*))` becomes
`qsort<T>(items: &[T], cmp: fn(T, T) -> Ordering)`:

- `base`, `n` and `size` collapse into one list, which is the proved pointer, count and size
  rule;
- the libc pack types `cmp`'s `int` result as `Ordering` (negative, zero, positive), so the
  closure uses Jet's `<=>` and cannot overflow the way `b - a` can;
- the libc pack marks `cmp` as **call-scoped**: used only during the call.

So the closure may capture and write `compares`, exactly as a non-escaping local lambda may today
(spec, "Captures and escaping lambdas"). `qsort` has no context pointer, so the trampoline keeps
the closure in a per-thread slot, saved and restored around the call. Nested sorts work. This is
PROPOSED in ballot D-FFI-CALLBACK3, which amends D-CABI-CALLBACK1.

Today the same program is rejected, from `tests/ui/cffi_callback_capture.stderr`:

```text
Error [jet.NonCAbiType]: `fn(I32) I32 -[]>` is not a C-compatible type for a foreign function parameter or return.
 Why: `#Import` / `#Export` functions must use types with a stable C ABI at the edge.
 Fix: Use scalars, `String`, or a struct with C layout; pointers only through the gated tier.
```

The second half of the entry is an event loop that calls back from its own thread. libuv-style
`loop_run()` runs on a thread the C library starts, and calls `on_tick` there.

```jet
// ticker.jet
package {
    name: "ticker"
    authority: { allow: [FFI, IO, Time] }
    deps: { ticker: c@"native/ticker" }
}

use tk :: c.ticker

fn run() {
    seen := 0
    clock :: tk.start(interval_ms: 10)
    sub :: clock.on_tick((event) -> {
        seen += 1
        if seen == 3 -> event.stop()
    })
    clock.wait_stopped()
    print("ticks {seen}")
}   // sub closes here: stop is requested, Jet waits for C to acknowledge, then frees the closure
```

```text
$ jet check ticker.jet
Error [E3233]: the callback for `on_tick` runs on the library's own thread, but it writes `seen`, which lives in this task.
  --> ticker.jet:10:28
    |
 10 |     sub :: clock.on_tick((event) -> {
    |                          ^^^^^^^^^^ called on ticker's thread [pack: thread .Library]
 11 |         seen += 1
    |         ---- written here
 Why: Two threads would write `seen` at once. Jet never moves a callback to another thread on its own.
 Fix: Deliver the callback to this task: `clock.on_tick((event) -> { … }, deliver: .Owner)`.
 Fix: Or share the counter: `seen :: shared Count{n: 0}`; the callback's `seen.n += 1` holds one lock.
More: jet-lang.dev/e/E3233
```

With `deliver: .Owner`, ticks queue to the task that registered them, and run at its next wait
point (`clock.wait_stopped()`):

```text
$ jet run ticker.jet
ticks 3
```

Competitors:

- **Rust**: `extern "C"` plus a `void*` context, written by hand per API.
- **Swift**: closures with captures cannot be passed as C function pointers at all.

```rust
extern "C" fn cmp(a: *const c_void, b: *const c_void) -> c_int { /* no context available: */
    /* qsort has no user-data slot, so a Rust closure cannot be passed; use a thread_local */ 0 }
// For callbacks with user data:
extern "C" fn on_tick(user: *mut c_void) {
    let seen = unsafe { &*(user as *const AtomicI32) };   // the user guarantees lifetime and threads
    seen.fetch_add(1, Ordering::SeqCst);
}
unsafe { ticker_on_tick(clock, on_tick, &seen as *const _ as *mut c_void) };
```

Where Jet is ahead:

- Closures just work for call-scoped callbacks, with no context pointer or `unsafe`.
- A foreign-thread race is a compile error that offers two fixes, where Rust leaves it as an
  `unsafe` promise.

Status: **PROPOSED**. D-FFI-CALLBACK2=A ratified managed registrations, and
`Examples/ffi/callbacks/run.jet` is its fixture. Call-scoped capturing closures, `deliver:` and
scope-end stop are ballot D-FFI-CALLBACK3.

---

### G5. Ownership: a malloc'd return, a borrowed view with a lifetime, a library-owned handle

Three C ownership facts, shown side by side.

```c
char *strdup(const char *s);                          // caller frees with free()
const char *getenv(const char *name);                 // borrowed from the environment, may be NULL
const unsigned char *png_get_rows_data(png_structp p, png_infop i);   // valid while p lives
png_structp png_create_read_struct(...);              // library-owned; destroy with png_destroy_read_struct
```

```jet
// owners.jet
package {
    name: "owners"
    authority: { allow: [FFI, IO, Env, FS, Mem.Alloc] }
    deps: { png: c@nixpkgs:libpng }
}

use {
    cstr :: "string.h"
    png :: c.png
}

fn run() {
    // 1. malloc'd return: Jet copies the text, then calls free() once.
    copy :: cstr.strdup("hello")
    print(copy)

    // 2. borrowed view: valid while the decoder lives; cannot outlive it.
    image :: png.Image.open("logo.png")?
    pixels :: image.rows()           // View<U8> from image, no copy
    print("{image.width}x{image.height}, first byte {pixels[0]}")

    // 3. library-owned handle: closed by the library's own destroy function at scope end.
}   // image closed here with png_destroy_read_struct
```

```text
$ jet run owners
hello
64x64, first byte 255
```

```text
$ jet inspect bind "string.h" --row strdup
  #Close(free) fn strdup(s: String) -> String = "strdup"   result owned by caller [header: __attribute__((malloc(free)))] -> copy, free once
```

If you return the borrowed view from the function that owns the image, Jet rejects it:

```jet
fn leak_rows() -> View<U8> {
    image :: png.Image.open("logo.png") ?? panic("bad file")
    image.rows()
}
```

```text
Error [jet.ReadOnlyValueNotOwned]: `image.rows()` borrows `image`, which closes when `leak_rows` returns.
  --> leak.jet:3:5
 Why: the rows belong to libpng's decoder [pack c/libpng@1.6: rows valid while decoder lives].
 Fix: Return an owned copy, `image.rows().copy()`, or return the `Image` itself.
More: jet-lang.dev/e/E0120
```

With no fact, the safe default for an unannotated `char*` return is: copy it, never free it,
and warn.

```text
Warning [W3230]: `legacy_name` returns `char*` and nothing says who frees it; Jet copies it and frees nothing.
 Why: freeing memory the library still owns would crash; never freeing memory you own leaks.
 Fix: If the caller frees it, add `#Close(free)` to the row (or to the pack); if it is borrowed, add `#Borrowed`.
More: jet-lang.dev/e/W3230
```

Competitor: **Rust, bindgen plus a hand wrapper.** All three facts are written by the user, in
`unsafe`.

```rust
let p = unsafe { libc::strdup(c"hello".as_ptr()) };
let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
unsafe { libc::free(p as *mut c_void) };                 // forget this: leak; do it twice: crash
// libpng: a hand-written struct with impl Drop calling png_destroy_read_struct,
// and a lifetime parameter on the rows slice that the wrapper author must get right.
```

Where Jet is ahead: the facts come from the header (`malloc(free)`) and the pack, and Jet's
existing `Close` and view rules enforce them. No user code is `unsafe`.

Status: **PROPOSED**. `#Close(fn)` on handles exists. `#Close(free)` on text, `#Borrowed`, view
returns from foreign rows and W3230 are new. Today a `String` return "is copied and never freed"
with no warning (spec).

---

### G6. C++: a class, `std::string`, `std::vector`, an exception, a template

```cpp
// native/inventory.hpp
#include <string>
#include <vector>
#include <stdexcept>

class Inventory {
public:
    explicit Inventory(std::string owner);
    void add(const std::string& item, int count);      // throws std::invalid_argument if count < 0
    std::vector<std::string> names() const;
    const std::string& owner() const;
};

template <typename T> T clamp_to(T v, T lo, T hi);
```

```jet
// shop.jet
package {
    name: "shop"
    authority: { allow: [FFI, IO, Mem.Alloc] }
    deps: { inventory: cpp@"native" }
}

use inv :: cpp.inventory

fn run() {
    shelf := inv.Inventory.new(owner: "nate")?       // `:=`: add() writes the shelf
    &shelf.add(item: "bolt", count: 3)?
    &shelf.add(item: "nut", count: 5)?
    print("{shelf.owner()}: {shelf.names()}")

    if &shelf.add(item: "gear", count: -1) == .Err(e) -> print("refused: {e}")

    print(inv.clamp_to<I32>(15, 0, 10))
    print(inv.clamp_to<F64>(-0.5, 0.0, 1.0))
}   // ~Inventory runs here
```

```text
$ jet run shop
nate: ["bolt", "nut"]
refused: CppError(type: "std::invalid_argument", what: "count must be >= 0")
10
0.0
```

```text
$ jet inspect bind inventory
cpp.inventory   source native/inventory.hpp (pinned clang 19.1, -std=c++20), shim 6.1 KB (cached)
  struct Inventory                      opaque, owned; Close -> ~Inventory()        [header]
  fn Inventory.new(owner: String) -> Inventory CppError!                            [header; may throw: not noexcept]
  fn Inventory.add(&self, item: String, count: I32) CppError!                       [header]
  fn Inventory.names(self) -> [String]                                              std::vector<std::string> -> [String], copied
  fn Inventory.owner(self) -> View<str> from self                                   const std::string& -> view
  fn clamp_to<T: I32 | I64 | F32 | F64 | …>(v: T, lo: T, hi: T) -> T                instances used: <int32_t>, <double>
```

Notes:

- Template instances are found from the calls in the program, as D-FFI-CPP1 ratified, so there
  is no `--instantiate` flag. The pinned clang comes from Jetpack (D-ADOPT-CC1=B), so there is
  no absolute clang path. Today's implementation still needs both flags.
- `add` takes `&self`, so it is called as `&shelf.add(...)` on a `:=` binding, like any Jet
  method that writes its receiver.
- `names()` copies, because a `std::vector<std::string>` holds C++ strings whose layout is the
  C++ library's.
- `owner()` is a view, because the method returns a `const&`.
- A method marked `noexcept` has no `CppError!`.

Competitor: **Rust `cxx`**. The user writes a bridge module that repeats every signature.

```rust
#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("inventory.hpp");
        type Inventory;
        fn new_inventory(owner: &CxxString) -> UniquePtr<Inventory>;   // needs a C++ helper
        fn add(self: Pin<&mut Inventory>, item: &CxxString, count: i32) -> Result<()>;
        fn names(self: &Inventory) -> UniquePtr<CxxVector<CxxString>>;
    }
}
// Templates: not supported; write a non-template C++ wrapper per instance.
```

Where Jet is ahead:

- Nothing is repeated.
- Constructors, templates and exceptions come from the header.
- `std::string` and `std::vector` arrive as Jet types.

Status: **PARTIAL**. A clang-based C++ binder exists:
`Jetpack/Integrations/Source/Foreign/Binders/Cpp.jet`, `CppClangSurface.jet` and
`CppShimRender.jet`. The spec requires `--target --clang --ar` with absolute paths and explicit
`--instantiate`. Automatic instances and pinned-toolchain provisioning are PROPOSED.

---

### G7. A Rust crate, on every target

```jet
// grep.jet
package {
    name: "grep"
    authority: { allow: [FFI, IO, FS] }
    deps: { regex: rust@"regex@1.11" }
}

use re :: rust.regex

fn run() {
    pattern :: re.Regex.new("(\\w+)@(\\w+)\\.com")?
    text :: "write to ada@example.com or bo@test.com"
    loop m in pattern.captures_iter(text) {
        user :: m.get(1) ?? next                     // a missing group skips this match
        host :: m.get(2) ?? next
        print("{user.as_str()} at {host.as_str()}")
    }
    if re.Regex.new("(unclosed") == .Err(e) -> print("bad pattern: {e}")
}
```

```text
$ jet run grep
ada at example
bo at test
bad pattern: RegexError.Syntax("regex parse error: (unclosed\n ^\nerror: unclosed group")

$ jet build grep --target aarch64-linux-gnu
built .jet/build/aarch64-linux-gnu/grep (2.1 MB, regex bridge cached)
```

```text
$ jet inspect bind regex --row Regex.new --row captures_iter
rust.regex 1.11.1   rustdoc-json (pinned rustc 1.83), bridge crate 14 items (cached per target)
  fn Regex.new(re: String) -> Regex RegexError!            Result<Regex, Error> -> RegexError!
  fn Regex.captures_iter(self, haystack: String) -> CaptureMatches from self, haystack
                                                           iterator -> Jet iterator; lifetimes 'r, 'h -> from self, haystack
```

Rust lifetimes `'r` and `'h` become Jet view provenance, `from self, haystack`, because both are
real Rust facts.

rustdoc JSON is still an unstable format: `rustdoc --output-format json` needs `-Z unstable-options`,
so a nightly toolchain or `RUSTC_BOOTSTRAP=1`, and its schema version changes between releases. Jet runs it only with
the pinned toolchain and a reader for that schema version, and the bridge crate itself builds on
stable. A pin bump that changes the schema updates the reader in the same Jet release.

Competitor: **C++ calling Rust through `cxx`**, the strongest existing Rust-from-another-language
path. It needs a hand-written bridge, and Cargo plus CMake glue.

```rust
#[cxx::bridge] mod ffi { extern "Rust" { type Re; fn re_new(p: &str) -> Result<Box<Re>>; fn re_find_all(r: &Re, t: &str) -> Vec<String>; } }
// plus a Rust impl of each function, plus cmake + corrosion to build it for each target.
```

Where Jet is ahead: no bridge is written, Cargo is never installed by hand, and every target
works.

Status: **PARTIAL**. A Rust bridge exists: `Compiler/JetDriver/Source/Driver/RustForeign.jet` and
`Jetpack/Integrations/Source/Foreign/RustBridge/`. It takes by-value types, plus `&`, `^` and
`#Close`. D-FFI-RUST1=A limits user Rust to static musl x86_64 Linux and keeps Cargo external.
Ballot D-FFI-RUST2 proposes the pinned toolchain on every target, rustdoc-based reading,
iterators and `dyn` traits.

---

### G8. Python and numpy

```jet
// stats.jet
package {
    name: "stats"
    authority: { allow: [FFI, IO] }
    deps: { numpy: py@">=1.26" }
}

use np :: py.numpy

fn run() {
    samples :: [F64]{2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0}
    arr :: np.asarray(samples)                    // shared memory, no copy
    print("mean {np.mean(arr)} std {np.std(arr)}")

    scaled :: (arr * 10.0).to_list<F64>()         // result comes back as a Jet list
    print(scaled[0..3])

    if np.linalg.inv(np.zeros((2, 2))) == .Err(e) -> print("python said: {e}")
}
```

```text
$ jet run stats
mean 5.0 std 2.0
[20.0, 40.0, 40.0]
python said: PyError(type: "numpy.linalg.LinAlgError", message: "Singular matrix", at: "numpy/linalg/_linalg.py:104")
```

```text
$ jet inspect bind numpy --row asarray --row mean
py.numpy 2.1.3   stubs: numpy/__init__.pyi (typed), placement .Process (default for py)
  fn asarray(a: [F64]) -> NDArray<F64>     buffer protocol -> shared memory pages, no copy
  fn mean(a: NDArray<F64>) -> F64 PyError!  stub: def mean(a: ArrayLike, ...) -> floating[Any]
```

Placement is the one option from G12. The default for Python is a crash-isolated child process
(D-FFI-PY1). Arrays cross through shared memory, so the sidecar copies nothing. For hot loops of
tiny calls, choose in-process with `isolate: { numpy: .InProcess }`.

Competitors:

- **Julia `PythonCall`**: very good, and in-process only.
- **Mojo**: Python modules import directly, in-process, untyped.

```julia
using PythonCall
np = pyimport("numpy")
a = np.asarray([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0])
println("mean ", pyconvert(Float64, np.mean(a)), " std ", pyconvert(Float64, np.std(a)))
try np.linalg.inv(np.zeros((2, 2))) catch e; println("python said: ", e) end
```

Where Jet is ahead: the call is typed from numpy's own `.pyi` stubs, and a numpy crash cannot
take the program down. Arrays still cross with no copy.

Status: **PARTIAL**. The Python binder exists, scalar-only: `Jetpack/PackageModel/Source/Foreign/Binders/Python.jet`.
The sidecar supervisor exists too: `WorkerSupervisor.jet`. Arrays, stubs, shared memory and
`PyError` details are PROPOSED.

---

### G9. JavaScript in the web tier

```jet
// chart.jet  (web target)
package {
    name: "chart"
    authority: { allow: [Browser, Net, FFI] }
    deps: { chartjs: js@"chart.js@4.4" }
}

use {
    charts :: js.chartjs
    web :: js.browser
}

fn run() {
    canvas :: web.document.get_element_by_id<web.HTMLCanvasElement>("sales") ?? panic("no #sales canvas")
    response :: web.fetch("/api/sales")?                 // a Promise: this is a wait point
    totals :: response.json<[Int]>()?                    // checked against [Int] at the crossing
    chart :: charts.Chart.new(canvas, {
        type: "bar",
        data: {labels: ["Q1", "Q2", "Q3", "Q4"], datasets: [{label: "sales", data: totals}]},
    })
    print("drew {totals.len()} bars")
}
```

```text
$ jet build chart --target web && jet serve chart
serving http://localhost:8080  (chart.wasm 182 KB, chart.js loaded from the hangar copy)
[browser console] drew 4 bars
```

If the server sends `["12", 30]`:

```text
[browser console] Stop [E3234]: `response.json<[Int]>()` got a string at index 0 where `Int` was declared.
 Why: values from JavaScript are checked at the crossing; Jet never lets an untyped value in.
 Fix: Declare the field as `String` and parse it, or fix the server to send numbers.
```

Competitor: **Rust `wasm-bindgen` and `web-sys`**.

```rust
#[wasm_bindgen(start)]
pub async fn run() -> Result<(), JsValue> {
    let window = web_sys::window().unwrap();
    let resp: Response = JsFuture::from(window.fetch_with_str("/api/sales")).await?.dyn_into()?;
    let json = JsFuture::from(resp.json()?).await?;
    let totals: Vec<i64> = serde_wasm_bindgen::from_value(json)?;
    // Chart.js: no typings; write #[wasm_bindgen] extern blocks by hand for every method used.
    Ok(())
}
```

Where Jet is ahead:

- Types come from the package's `.d.ts` (D-FFI-JS1).
- There is no `async`/`.await` colouring, because the Promise is a wait point.
- The same `use js.chartjs` line runs on a native target inside the JS-on-wasm host.

Status: **PARTIAL**. The JS binder exists, scalar-only, with a Node sidecar for bind time:
`Jetpack/PackageModel/Source/Foreign/Binders/JavaScript.jet`. `.d.ts` stubs, objects and
promise wait points are PROPOSED. D-FFI-JS1 ratified the two hosts.

---

### G10. Exporting a Jet library to C: records, lists, errors, callbacks, a long-running call

```jet
// tally/library.jet
#Codable
struct Item {
    name: String
    qty: I32
}

#Codable
struct Report {
    total: I64
    names: [String]
}

#Error
enum TallyError {
    Empty
    Negative(index: Int)
}

#Export(c) fn summarize(items: [Item]) -> Report TallyError! {
    if items.len() == 0 -> return Err(TallyError.Empty)
    total := I64{0}
    loop i := 0, i < items.len(), i + 1 {
        qty :: items[i].qty
        if qty < 0 -> return Err(TallyError.Negative(index: i))
        total += qty                                  // I32 widens to I64 implicitly
    }
    Ok(Report{total: total, names: items.map((it) -> it.name)})
}

#Export(c) fn each_over(items: [Item], limit: I32, visit: fn(Item)) {
    loop it in items { if it.qty > limit -> visit.call(it) }
}

#Export(c) fn slow_count(path: String) -[FS]> Int IOError! {   // waits on I/O: exported as async
    text :: core.files.read(path)?
    Ok(text.lines().len())
}
```

```jet
// tally/package.jet
name: "tally"
version: "0.1.0"
outputs: {
    core: .Library{ name: "tally", native: true, bindings: [c, python] }
}
```

```text
$ jet build tally
built .jet/build/libtally.so, libtally.a
wrote .jet/build/bindings/c/tally.h (3 exports, 2 records, 1 error enum)
wrote .jet/build/bindings/python/tally/ (wheel tally-0.1.0-cp312-abi3-linux_x86_64.whl)
```

The generated header, an excerpt. It is plain C, and every ownership rule is written in it.

```c
/* tally.h, generated by jet 2026.10 from tally 0.1.0. Contract fingerprint 7f3a…  */
typedef struct { const char *ptr; size_t len; } tally_str;            /* borrowed UTF-8 */
typedef struct { tally_str name; int32_t qty; } tally_Item;
typedef struct { int64_t total; tally_str *names; size_t names_len; } tally_Report;   /* owned: free with tally_Report_free */
typedef enum { TALLY_OK = 0, TALLY_ERR_EMPTY = 1, TALLY_ERR_NEGATIVE = 2 } tally_status;
typedef struct { tally_status kind; int64_t index; /* Negative */ } tally_TallyError;

tally_status tally_summarize(const tally_Item *items, size_t items_len,
                             tally_Report *out, tally_TallyError *err);
void tally_Report_free(tally_Report *r);

typedef void (*tally_Item_visit)(void *ctx, const tally_Item *item);
void tally_each_over(const tally_Item *items, size_t items_len, int32_t limit,
                     tally_Item_visit visit, void *ctx);         /* visit is called only during this call */

typedef struct tally_task tally_task;                             /* a running Jet task */
typedef struct { int32_t kind; tally_str message; } tally_IOError;  /* borrowed: valid during done() */
typedef void (*tally_slow_count_done)(void *ctx, tally_status st, int64_t value, const tally_IOError *err);
tally_task *tally_slow_count(tally_str path, tally_slow_count_done done, void *ctx);
void tally_task_cancel(tally_task *t);                             /* done() still runs exactly once */
```

A C host using it:

```c
#include "tally.h"
#include <stdio.h>

static void show(void *ctx, const tally_Item *it) { printf("big: %.*s\n", (int)it->name.len, it->name.ptr); }

int main(void) {
    tally_Item items[] = {{{"bolt", 4}, 3}, {{"nut", 3}, 12}};
    tally_Report r; tally_TallyError e;
    if (tally_summarize(items, 2, &r, &e) == TALLY_OK) {
        printf("total %lld, %zu names\n", (long long)r.total, r.names_len);
        tally_Report_free(&r);
    }
    tally_each_over(items, 2, 10, show, NULL);
    tally_Item bad[] = {{{"x", 1}, -2}};
    if (tally_summarize(bad, 1, &r, &e) == TALLY_ERR_NEGATIVE) printf("negative at %lld\n", (long long)e.index);
}
```

```text
$ cc host.c -Itally/.jet/build/bindings/c -Ltally/.jet/build -ltally -o host && ./host
total 15, 2 names
big: nut
negative at 0
```

Competitors:

- **Rust with `cbindgen`**: the user writes `#[repr(C)]` mirror structs, `extern "C"` wrappers,
  and free functions by hand.
- **Zig `export`**: no lists, errors or closures cross; the user writes every C-shaped function.

```rust
#[repr(C)] pub struct Item { name: *const c_char, qty: i32 }
#[repr(C)] pub struct Report { total: i64, names: *mut *mut c_char, names_len: usize }
#[no_mangle] pub extern "C" fn tally_summarize(items: *const Item, n: usize, out: *mut Report) -> i32 {
    let items = unsafe { std::slice::from_raw_parts(items, n) };        // hand-written, unsafe
    /* … convert, Box::into_raw the names, write *out … */ 0 }
#[no_mangle] pub extern "C" fn tally_report_free(r: *mut Report) { /* hand-written */ }
```

Where Jet is ahead: the Jet code stays ordinary Jet. Every C-side shape is generated, and its
ownership rule is in the header. An async Jet function gets a completion callback plus cancel,
not a blocked thread.

Status: **PARTIAL**. `#Export(c)` with scalars exists (`Compiler/JetSema/Source/Sema/Guest.jet`,
with `jet_text_free`). The projections in `Examples/ffi/projections/` carry records as JSON text
(`JetComponent`). Typed records, lists, errors, callbacks and async are ballot D-FFI-EXPORT2.

---

### G11. The same library from Python

```python
# host.py, after: pip install .jet/build/bindings/python/tally-0.1.0-*.whl
import asyncio
import tally

report = tally.summarize([tally.Item("bolt", 3), tally.Item("nut", 12)])
print(report)                                   # a frozen dataclass

try:
    tally.summarize([])
except tally.TallyError.Empty:
    print("empty refused")

tally.each_over([tally.Item("nut", 12)], 10, lambda it: print("big:", it.name))

async def main():
    n = await tally.slow_count("notes.txt")     # a Jet task; awaiting does not block the loop
    print("lines", n)
asyncio.run(main())
```

```text
$ python host.py
Report(total=15, names=['bolt', 'nut'])
empty refused
big: nut
lines 42
```

The generated `tally/__init__.pyi`, an excerpt:

```python
@dataclass(frozen=True)
class Item:
    name: str
    qty: int            # checked: -2**31 <= qty < 2**31, else ValueError before Jet runs
@dataclass(frozen=True)
class Report:
    total: int
    names: list[str]
class TallyError(Exception):
    Empty: ClassVar[type[TallyErrorEmpty]]          # so `except tally.TallyError.Empty` works
    Negative: ClassVar[type[TallyErrorNegative]]
class TallyErrorEmpty(TallyError): ...
class TallyErrorNegative(TallyError):
    index: int
def summarize(items: list[Item]) -> Report: ...
def each_over(items: list[Item], limit: int, visit: Callable[[Item], None]) -> None: ...
async def slow_count(path: str) -> int: ...
```

Competitor: **Rust `PyO3` plus maturin**, the best "native library for Python" experience
today.

```rust
#[pyclass(frozen)] #[derive(Clone)] struct Item { #[pyo3(get)] name: String, #[pyo3(get)] qty: i32 }
#[pymethods] impl Item { #[new] fn new(name: String, qty: i32) -> Self { Self { name, qty } } }
create_exception!(tally, TallyError, PyException);
#[pyfunction] fn summarize(items: Vec<Item>) -> PyResult<Report> { /* … */ }
#[pyfunction] fn slow_count(py: Python<'_>, path: String) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move { /* … */ })
}
#[pymodule] fn tally(m: &Bound<'_, PyModule>) -> PyResult<()> { /* register each item by hand */ }
```

Where Jet is ahead:

- No attribute appears on any Jet item.
- One `bindings: [c, python]` line writes both hosts from the same export table.
- Errors get one Python exception class per variant.
- `async def` comes from Jet's ordinary wait points.

Status: **PROPOSED**. Today's Python projection (`Examples/ffi/projections/python/host.py`)
returns status codes and passes records as dicts. Ballot D-FFI-EXPORT2.

---

### G12. Run an untrusted library isolated by changing one option

`fastimg` is a third-party image decoder written in C. It has a history of buffer overflows.

```jet
// thumbs/package.jet
name: "thumbs"
version: "0.1.0"
authority: { allow: [FFI, FS, IO, Mem.Alloc] }
deps: { fastimg: c@"vendor/fastimg" }
isolate: { fastimg: .Wasm }          // PROPOSED: the only line that changed
```

```jet
// thumbs/run.jet  (identical with or without the isolate line)
use fi :: c.fastimg

fn run() {
    loop path in ["ok.png", "evil.png", "ok2.png"] {
        if fi.decode_file(path) == {
            .Ok(img) -> print("{path}: {img.width}x{img.height}")
            .Err(e) -> print("{path}: {e}")
        }
    }
}
```

Without the line, `evil.png` overflows a heap buffer inside `fastimg`, and the whole program
dies:

```text
$ jet run thumbs
ok.png: 640x480
Stop [E3236]: native code crashed: SIGSEGV in fastimg (decode.c:211 fi_inflate_row) during `fi.decode_file`.
  last crossing: thumbs/run.jet:5:12  fi.decode_file("evil.png")  thread main
 Why: in-process C shares this program's memory; a fault there ends the program.
 Fix: Run fastimg isolated: add `isolate: { fastimg: .Wasm }` to package.jet (its C source is available).
More: jet-lang.dev/e/E3236
```

With the line:

```text
$ jet run thumbs
ok.png: 640x480
evil.png: IsolateError.Trapped(library: "fastimg", reason: "out-of-bounds memory access", at: "decode.c:211 fi_inflate_row")
ok2.png: 800x600
```

`decode_file` gained an `IsolateError` failure, which Jet adds to the row's error set. The
`if … == { .Ok … .Err … }` match already handles it. The sandbox is rebuilt fresh after a trap,
so `ok2.png` decodes normally.

For a closed binary with no source, use `.Process`:

```jet
isolate: { fastimg: .Process }
```

```text
evil.png: IsolateError.Crashed(library: "fastimg", signal: "SIGSEGV", restarted: true)
```

```text
$ jet inspect bind fastimg --placement
c.fastimg   placement .Wasm (package.jet:5)   source vendor/fastimg/*.c -> wasm32 (pinned clang), translated to native at link
  imports granted: fd_read (FS.Read: inputs passed by the caller only), memory 64 MiB max
  calls: 2 rows; lists cross through the sandbox's memory, no Jet pointer enters
  cost: ~1.3x decode time measured on the corpus; 0 bytes of Jet heap reachable
```

Competitors:

- **Firefox's RLBox with wasm2c**: the same idea, with the C library compiled to wasm and then
  to native. Every call site must be rewritten to use `tainted<T>` values and `sandbox.invoke`.
- **Python**: `subprocess` plus hand-written serialization.

```cpp
rlbox::rlbox_sandbox<rlbox_wasm2c_sandbox> sandbox;
sandbox.create_sandbox();
auto buf = sandbox.malloc_in_sandbox<char>(len);
std::memcpy(buf.unverified_safe_pointer_because(len, "copy in"), data, len);
auto img = sandbox.invoke_sandbox_function(fi_decode, buf, len);
int w = img->width.copy_and_verify([](int w) { return w > 0 && w < 65536 ? w : -1; });   // every value verified by hand
```

Where Jet is ahead: one line in `package.jet`, no call-site change, and the checks RLBox makes
you write by hand come from the row types.

Status: **PROPOSED** (ballot D-FFI-ISOLATE1). The wasm-component plugin host exists:
`Compiler/JetRuntime/Source/PluginHost.jet` loads a component "translated into this program's
own code". So does the process supervisor: `WorkerSupervisor.jet`, used by the sidecar
languages.

---

### G13. Inline C and inline assembly

```jet
// popcount.jet
package {
    name: "popcount"
    authority: { allow: [Exec, FFI, IO, Mem.Alloc] }
}

use core.mem

#[Unsafe("the scalar ABI contract matches the C definition"), FFI(c)]
fn bits_c(x: U64) -> I32 {
    """
    #include <stdint.h>
    int32_t bits_c(uint64_t x) { return __builtin_popcountll(x); }
    """
}

#[Unsafe("popcnt writes only its destination register; no clobbers"), FFI(asm)]
fn bits_asm(x: Int) -> Int {
    """
    popcnt {x}, {x} ; -> return
    """
}

fn run() {
    print(bits_c(U64{255}))          // PROPOSED: the declaration's #Unsafe reason is the one audit site
    print(bits_asm(4096 + 3))        // today both calls also need an #Unsafe("…") { } block
}
```

```text
$ jet run popcount.jet
8
3
```

If the signature and the C disagree, the error names both sides:

```text
Error [jet.InlineForeignBuildFailed]: `#FFI(c)` body defines `int32_t bits_c(uint32_t)` but the Jet signature says `U64`.
  --> popcount.jet:8:4
 Why: inline foreign contracts are ABI boundaries; the binder read the body with the pinned clang.
 Fix: Change the parameter to `U32`, or the C parameter to `uint64_t`.
More: jet-lang.dev/e/E3222
```

Competitors:

- **Zig**: inline `asm volatile` with constraint strings.
- **Rust**: `asm!` with named operands; inline C is not possible.

```rust
fn bits_asm(x: u64) -> u64 { let r: u64; unsafe { std::arch::asm!("popcnt {r}, {x}", x = in(reg) x, r = out(reg) r) }; r }
// Inline C: impossible in Rust; write a .c file plus build.rs plus the cc crate.
```

Where Jet is ahead: one shape, `#FFI(lang)`, for C, C++ and asm, all checked against the Jet
signature.

Status: **EXISTS** for C and x86_64 asm: `Examples/features/lowlevel/inline_c.jet` and
`inline_asm.jet`, and `BuildForeign.jet`. The asm operand spelling here follows those examples.
D-FFI-INLINE1 already ratified a mismatch diagnostic "naming both sides", but it is not built;
today E3222 says the signature "can't be represented". Dropping the call-site `#Unsafe` block is
PROPOSED in D-FFI-VOUCH2, because D-FFI-INLINE1 requires "the enclosing `#Unsafe` gate".

---

### G14. Dynamic loading: a native plugin by path, and a wasm plugin

```jet
// host.jet
package {
    name: "host"
    authority: { allow: [FFI, FS, IO] }
}

use {
    core.ffi
    plugin :: core.plugin
}

// A native plugin: the expected signature is a Jet type, checked against the plugin's contract note.
fn native_version(path: String) -> I32 ffi.FFIError! {
    lib :: ffi.Library.open(path)?                          // PROPOSED: safe when the .so carries a Jet contract note
    version :: lib.symbol<fn() -> I32>("demo_version")?
    Ok(version.call())
}

fn run() {
    print(native_version("./plugins/libdemo.so") ?? -1)
    if native_version("./plugins/libold.so") == .Err(e) -> print(e)

    policy :: Authority.from_rights(["FS.Read:assets"])
    math :: plugin.load("plugins/mathkit.wasm", policy)     // typed by the plugin's frozen exports
    print(math.square(12) ?? panic("square failed"))
}
```

```text
$ jet run host.jet
3
FFIError.Signature(symbol: "demo_version", expected: "fn() -> I32", found: "fn() -> I64", library: "libold.so 1.0")
144
```

A shared object built by Jet, or one with a pack, carries a contract note: a small ELF section
listing each export's Jet signature. `lib.symbol<Sig>` checks `Sig` against it, so a wrong type
is a typed error, not undefined behavior. For a `.so` with no note, the same two calls need
`#Unsafe("reason")`, exactly as ratified in D-FFI-PRIMS1:

```text
Error [E3237]: `libdemo.so` has no contract note, so `lib.symbol<fn() -> I32>` cannot be checked.
 Fix: Wrap the lookup in `#Unsafe("libdemo exports int32_t demo_version(void)")`, or rebuild the plugin with Jet.
```

Competitor: **Rust `libloading`**.

```rust
let lib = unsafe { libloading::Library::new("./plugins/libdemo.so")? };
let version: libloading::Symbol<unsafe extern "C" fn() -> i32> = unsafe { lib.get(b"demo_version")? };
println!("{}", unsafe { version() });     // a wrong type here is silent undefined behaviour
```

Where Jet is ahead: the signature is checked when it can be. The wasm plugin path has typed
exports, and version checks at load (E1257, D-PLUGIN-VERSION1).

Status: **PARTIAL**. `core.plugin` with wasm components exists: `Core/Plugin/Plugin.jet` and
`PluginHost.jet`. `core.ffi` (D-FFI-PRIMS1=A) is ratified but not built. The contract note is
PROPOSED.

---

### G15. A library upgrade that breaks the contract

The project froze its bindings (`jet bind --freeze`, D-FFI-AUTO1). Then `libgeom` 2.0 changes
`geom_name` to return a malloc'd string, and widens `quad_area`'s out-parameter to `double`.

At build time:

```text
$ jet build shapes
Error [E3225]: the contract for `c.geom` changed since it was frozen (.jet/lock, accepted 2026-10-02).
  geom.h 1.4 -> 2.0 (sha256 41c0… -> 9e7d…)
  row quad_area:  out-parameter `float *out_area`  ->  `double *out_area`      result F32 -> F64
  row geom_name:  result ownership  borrowed (static)  ->  owned (free)        [header: __attribute__((malloc(free)))]
  2 call sites affected: shapes.jet:12, shapes.jet:14
 Why: a frozen binding plan never changes silently.
 Fix: Review and accept: `jet bind geom --update --preview`, then `jet bind geom --update --accept 9e7d…`.
More: jet-lang.dev/e/E3225
```

At load time: the program was built against 1.4, and the machine has a 2.0 `.so` with the same
soname:

```text
$ ./shapes
Stop [E3226]: libgeom.so.1 at /usr/lib/libgeom.so.1 does not match the contract this program was built with.
  missing symbol version GEOM_1.4 (library offers GEOM_2.0)
  row quad_area: library's debug info says `double *out_area`, program expects `float *`
 Why: calling it would write 8 bytes into a 4-byte slot.
 Fix: Install libgeom 1.4, or rebuild shapes against 2.0 (`jet build shapes` will show the changes).
More: jet-lang.dev/e/E3226
```

The check runs before `run` starts, so no call into a mismatched library ever happens.

Competitors:

- **Go cgo**: rebuilds against the new header, and compiles cleanly if C's implicit conversions
  allow it.
- **Rust bindgen**: regenerates silently. Both load a mismatched `.so` and fail at the first bad
  call, often as memory corruption.

```text
$ ./shapes-rust        # built against 1.4, run with 2.0
area 0.0               # wrong value, silently; the double write corrupted the next stack slot
Segmentation fault (core dumped)
```

Status: **PROPOSED**. The binding stamp and digest exist
(`jet-ffi-descriptor-v1;… digest=…` in generated bindings). D-FFI-AUTO1=A ratified freeze and
`--policy frozen`. The semantic diff and the load-time check are new.

---

### G16. A mixed-language crash, and the `ffi` lens of `jet perf`

A Jet callback passed to a C++ physics engine indexes past the end of a list. The engine was
called from Jet, and C++ called back into Jet:

```text
$ jet run sim
Stop [E3001]: index 12 is out of range for a list of length 12
  in Jet      sim/run.jet:31:17          on_contact(a, b)               [callback, call-scoped]
  in C++      physics/world.cpp:208      World::dispatch_contacts()     [libphysics.a]
  in C++      physics/world.cpp:144      World::step(float)             [libphysics.a]
  ── crossing: cpp.physics World.step (shim physics_World_step) ──
  in Jet      sim/run.jet:19:9           run()
 Why: …  Fix: …  More: jet-lang.dev/e/E3001
 note: the stop did not unwind through C++. `on_contact` has no failure value for C++, so Jet
       printed this report at the crossing and ended the program.
```

Every frame is symbolized: Jet frames from Jet's maps, C++ frames from DWARF. The crossing is
marked with the row and the shim.

Tracing every crossing uses the existing `jet perf` recorder (D-PERFSESSION1) with one new lens,
`ffi`. `jet run --trace` is already the compiler-event trace, so it is not reused. `--lens=ffi`
keeps per-row counts and times; `--lens=ffi:calls` also keeps each call's arguments:

```text
$ jet perf run sim --lens=ffi:calls
wrote .jet/perf/20261031T1014-7c2e.jettrace (18,240 crossings with call records, 3 threads)

$ jet perf view .jet/perf/20261031T1014-7c2e.jettrace --lens=ffi --top 4
 count   total     mean    row                                   placement   copied
 6,000   41.2 ms   6.9 µs  cpp.physics World.step                in-process  0 B
 6,000    3.1 ms   0.5 µs  c.raylib DrawCircleV                  in-process  0 B
 6,000    1.4 ms   0.2 µs  callback on_contact (from C++)        attach once 0 B
   240   12.8 ms  53.3 µs  py.numpy asarray                      process     0 B (shared)
first 3 crossings:
   0.000 ms  main   -> cpp.physics World.new(gravity: -9.8)                         ok 4.1 µs
   0.012 ms  main   -> cpp.physics World.add_body(&world, Body{mass: 1.0, …})        ok 0.3 µs
   0.020 ms  main   -> cpp.physics World.step(&world, 0.016)                         ok 7.0 µs
```

Competitor: **Go cgo**. Go prints Go frames, and C frames only if the user registers a
traceback function, `runtime.SetCgoTraceback`, which almost nobody does. There is no crossing
trace beyond `perf`.

```text
panic: runtime error: index out of range [12] with length 12
goroutine 1 [running]:
main.onContact(...)        /sim/main.go:31
main._cgoexpwrap_onContact(...)
runtime.cgocallbackg1(...)
<C frames not shown>
```

Status: **PROPOSED** (ballot D-FFI-TRACE1). `jet perf run|view` and `.jettrace` are ratified
(D-PERFSESSION1=D, D-ARTIFACT-EXT1=A); the `ffi` lens is new. D-RT-DEFECT-REPORT1 and
D-RELEASE-DEBUG1 are open.

---

### G17. The expert path: one wrapper expanded down to `core.ffi`

This is the `column_text` row from G2, written by hand with the building blocks. It shows that
nothing in a wrapper is compiler magic.

```text
$ jet inspect bind sqlite3 --row Statement.column_text --expand
```

```jet
// what the wrapper is, as ordinary Jet (output of --expand)
fn Statement.column_text(self, col: I32) -> View<str> from self {
    raw :: ffi.call<fn(mem.Ptr<ffi.COpaque>?, I32) -> mem.Ptr<U8>?>(SYM_sqlite3_column_text, self.handle, col)
    len :: ffi.call<fn(mem.Ptr<ffi.COpaque>?, I32) -> I32>(SYM_sqlite3_column_bytes, self.handle, col)
    if raw == {
        .None -> ffi.view_text_empty(self)                         // SQL NULL reads as ""
        .Val(p) -> ffi.view_text(p, len, from: self, check: .Utf8)   // I32 widens to Int; borrow tied to self; validated once
    }
}
```

The same row written by hand in a project that has no pack:

```jet
use {
    sql3 :: "sqlite3.h"
    core.ffi
    core.mem
}

fn column_text(stmt: sql3.Statement, col: I32) -> View<str> from stmt {
    #Unsafe("sqlite3_column_text is valid until the next step/reset/finalize on stmt; stmt is borrowed for the view's life") {
        raw :: sql3.__native.sqlite3_column_text(stmt, col)        // native shape (--shape native): mem.Ptr<U8>?
        len :: sql3.__native.sqlite3_column_bytes(stmt, col)
        if raw == {
            .None -> ffi.view_text_empty(stmt)
            .Val(p) -> ffi.view_text(p, len, from: stmt, check: .Utf8)
        }
    }
}
```

At comptime, a pure row runs in a wasm sandbox inside the compiler's comptime child (ballot
D-FFI-COMPTIME1). `XXH64` takes `const void*, size_t, uint64_t`; the pointer and count collapse
to one `[U8]`, so the Jet row has only fixed-width types:

```jet
use xx :: c.xxhash // xxhash's C source comes from Jetpack

KEYWORD_HASHES :: comptime {
    loop k in ["fn", "loop", "use", "struct"] -> xx.XXH64(k.bytes(), seed: 0)
}

fn run() { print(KEYWORD_HASHES[0]) }
```

```text
$ jet run hashes.jet
11371907208256377266
$ jet test
comptime replay: xx.XXH64, 4 recorded calls, native linux-x86_64 build matches
```

Competitors:

- **LuaJIT `ffi.cdef`**, the fastest dynamic FFI: no safety and no lifetimes.
- **Zig**: no runtime wrapper to expand, because every call is raw.

```lua
local ffi = require("ffi")
ffi.cdef[[ const unsigned char *sqlite3_column_text(void*, int); int sqlite3_column_bytes(void*, int); ]]
local sq = ffi.load("sqlite3")
local s = ffi.string(sq.sqlite3_column_text(stmt, 1), sq.sqlite3_column_bytes(stmt, 1))   -- copies; nothing checks stmt is live
```

Status: **PROPOSED**. D-FFI-PRIMS1=A ratified `core.ffi`. The `--expand` output, `ffi.call`,
`ffi.view_text` and comptime execution are new.

---

### G18. A real app: a raylib game window with input, textures and sound

```jet
// dodge/package.jet
name: "dodge"
version: "0.1.0"
authority: { allow: [FFI, GPU, IO, FS, Time, Rand, Mem.Alloc] }
deps: { raylib: c@nixpkgs:raylib }
```

```jet
// dodge/run.jet
use rl :: c.raylib

struct Rock {
    pos: rl.Vector2
    speed: F32
}

fn run() {
    window :: rl.InitWindow(800, 450, "dodge")?      // Close: CloseWindow at scope end [pack]; main thread [pack]
    rl.SetTargetFPS(60)
    ship :: rl.LoadTexture("assets/ship.png")?       // Close: UnloadTexture [pack]
    boom :: rl.LoadSound("assets/boom.wav")?         // Close: UnloadSound [pack]
    player := rl.Vector2{x: 400.0, y: 400.0}
    rocks := [Rock]{}
    score := 0

    loop !rl.WindowShouldClose() {
        dt :: rl.GetFrameTime()
        if rl.IsKeyDown(.KEY_LEFT) -> player.x -= 300.0 * dt
        if rl.IsKeyDown(.KEY_RIGHT) -> player.x += 300.0 * dt
        if rl.GetRandomValue(0, 20) == 0 {
            x :: F32.from_i32(rl.GetRandomValue(0, 800))          // destination-owned conversion
            &rocks.push(Rock{pos: rl.Vector2{x: x, y: -20.0}, speed: 200.0})
        }
        loop r in &rocks -> r.pos.y += r.speed * dt
        if rocks.any((r) -> rl.CheckCollisionCircles(r.pos, 12.0, player, 16.0)) {
            rl.PlaySound(boom)
            rocks = [Rock]{}
            score = 0
        }
        score += 1

        frame :: rl.BeginDrawing()                    // Close: EndDrawing at scope end [pack]
        rl.ClearBackground(rl.RAYWHITE)
        rl.DrawTextureV(ship, player, rl.WHITE)
        loop r in rocks -> rl.DrawCircleV(r.pos, 12.0, rl.DARKGRAY)
        rl.DrawText("score {score}", 10, 10, 20, rl.MAROON)
    }
}
```

```text
$ jet run dodge
[window "dodge" opens at 800x450; arrow keys move the ship; rocks fall; score counts up]
[closing the window ends the loop; textures, sound and window close in reverse order]

$ jet build dodge --release
built .jet/build/release/dodge (1.9 MB static, raylib 5.5 linked statically; startup 38 ms to first frame)
```

Mistakes the design catches:

```text
Error [E3231]: `rl.DrawText` must run on the main thread [pack c/raylib@5.5: thread .Main], but this call is inside `task`.
  --> dodge/run.jet:44:9
 Fix: Draw from `run()`, or send the text to the main loop through a channel.

Error [jet.UseAfterMove]: `ship` is used after it was moved by `rl.UnloadTexture(^ship)`.
 Fix: Let `ship` close at scope end, or stop using it after unloading.
```

Notes:

- The names are raylib's own (D-SHAPE-CASE2). A search of raylib's documentation finds every
  call.
- Interpolation in `DrawText` works, because Jet passes its own NUL-terminated text with no
  copy.
- `frame :: rl.BeginDrawing()` is a `Close` resource, so `EndDrawing` cannot be forgotten.

Competitors:

- **Zig with raylib** through `@cImport`: excellent, but nothing is checked.
- **C# Raylib-cs**: a hand-maintained binding package.

```zig
const rl = @cImport(@cInclude("raylib.h"));
pub fn main() void {
    rl.InitWindow(800, 450, "dodge");
    defer rl.CloseWindow();
    const ship = rl.LoadTexture("assets/ship.png");   // missing file: a zero texture, no error
    defer rl.UnloadTexture(ship);
    while (!rl.WindowShouldClose()) {
        rl.BeginDrawing();
        defer rl.EndDrawing();
        // DrawText needs a [*c]const u8: formatting the score needs a buffer and bufPrintZ
    }
}
```

Status: **PARTIAL**. `Examples/features/game/raylib_window.jet` runs today through the
first-party `core.game.raylib` bridge, a hand-written facade. Binding raylib's own header with a
pack, so that any raylib function works, is PROPOSED. The raylib pack's `.Main` thread fact and
`BeginDrawing` as a `Close` resource are pack content.

---

## 3. The design in one page

```text
           your Jet code:  use c.sqlite3 as sql   ·   #Export(c) fn …   ·   isolate: { lib: .Wasm }
                                   │                         │                         │
 L3 wrappers   generated Jet functions + packs (facts, #Check, #Close, view returns, names)
                                   │                         │
 L2 readers / writers   ordinary packages: c, cpp, rust, py, js, …  (D-FFI-BINDER1)
                         readers turn an interface into rows; writers turn exports into host APIs
                                   │                         │
 L1 contract   one descriptor per library: every row's facts with their source and evidence
               (D-FFI-CONTRACT1)  → fingerprint for --freeze and for the load check
                                   │
 L0 primitives   core.ffi: layout, abi, Library/symbol, call, thunk, view_text, completion,
                 capture errno, placement  (D-FFI-PRIMS1). Unsafe steps need #Unsafe("reason").
```

Rules that hold at every layer:

- A layer is built only from the layer below it, and `jet inspect bind --expand` shows the steps.
- Nothing above L0 has a private compiler hook. This is the "the language is a library" pillar.
- Missing facts produce a safe shape. Wrong facts can only come from a vouch (`#Unsafe`) or a
  pack, and `jet audit foreign` names both.

## 4. Layer 0: primitives, by example

`core.ffi` (D-FFI-PRIMS1=A, ratified, not built). Each building block is one line here; G14 and
G17 show them in use.

```jet
use {
    png :: "png.h"
    libc :: "fcntl.h"
    stdlib :: "stdlib.h"
    libjob :: "job.h"
    core.ffi
    core.mem
}

comptime {
    shape :: ffi.layout<png.png_color>(target: $build.target)     // safe, comptime: size, align, field offsets
    assert(shape.size == 3)
}

fn by_hand() -> I32 ffi.FFIError! {
    #Unsafe("libdemo exports int32_t demo_version(void)") {
        lib :: ffi.Library.open("./libdemo.so")?                   // dlopen / LoadLibrary
        f :: lib.symbol<fn() -> I32>("demo_version")?               // FnPtr<Sig>
        return Ok(f.call())
    }
}

fn errno_example(path: String) -> Int ffi.FFIError! {
    fd :: libc.open(path, libc.O_RDONLY, capture: .Errno)          // (I32, errno) read on the same thread
    if fd.value < 0 -> return Err(ffi.FFIError.Os(fd.errno))
    Ok(fd.value)                                                    // I32 widens to Int
}

fn sort_by_key(items: &[Row], key: Int) {
    cb :: ffi.thunk((a: Row, b: Row) -> I32 { compare_rows(a, b, key) })   // (function, user data)
    #Unsafe("qsort_r keeps cb only for this call") {
        stdlib.qsort_r(&items, cb)
    }
}   // cb drops here and is unregistered

fn wait_for_c(job: libjob.Job) -> [U8] {
    done :: ffi.completion<[U8]>()                                  // a wait point fed by a C callback
    libjob.start(job, done.callback())                              // callback + context, called once
    done.wait()                                                     // this task waits; others run
}
```

## 5. Layer 1: the contract and its facts

### 5.1 What a fact is

| Fact | Values | Default when unknown |
|---|---|---|
| nullability | non-null, nullable | nullable, so `T?` (D-FFI-CMAP1) |
| extent | `counted_by(n)`, `sized_by(n)`, NUL-terminated, fixed `[T; N]` | refuse the list shape, keep the native pointer under `--shape native` |
| result ownership | owned by caller (`#Close(fn)`), borrowed (`#Borrowed`), view of owner (`from x`) | text copied, nothing freed, W3230 |
| handle release | `#Close(fn)` | no auto-close; the handle is still opaque and safe to pass |
| invalidation | which calls end a view (they take `&owner`) | generation check on access |
| failure | `#Check(fn)`, errno, `GetLastError`, exceptions | the raw status is returned as `I32` and must be used |
| callback retention | call-scoped, retained until `fn`, forever | retained: managed registration (D-FFI-CALLBACK2) |
| thread | `.Any`, `.Caller`, `.Main`, `.Library` | `.Caller`: the handle cannot cross tasks |
| effects | none (pure), named roots, all | all |
| blocking | `#Blocking` | inline, watchdog report after 50 ms |

### 5.2 Where facts come from, strongest first

1. **Checked:** Jet source, a Jet-built library's contract note, or an isolated placement that
   enforces the fact.
2. **Read** from the header with the library's own flags: Clang nullability,
   `__counted_by`, `noescape`, `malloc(free)`, `ownership_returns`/`takes`, `const`/`pure`,
   `warn_unused_result`, and `// jet-ffi-*` comment lines in headers you own (the format in
   `Examples/ffi/adaptations/*/*.h`).
3. **Pack:** a published overlay for a library version range (§5.3).
4. **The project's own `#Import module` overlay.** This exists today.
5. **Vouched:** rows inside `#Unsafe("reason") { }` in an overlay (D-FFI-VOUCH1).

`jet bind --suggest geom` drafts facts from names (`*_free`, `*_destroy`, `*_new`), doc comments,
and an instrumented run of the project's tests. It writes `geom.suggested.jet` for review. It
never applies them.

### 5.3 Packs (PROPOSED, ballot D-FFI-PACK1)

A pack is an ordinary Jet package, `c-sqlite3-pack`, holding one `#Import module c.sqlite3`
overlay, as shown in G2. It is published on Jetpack with:

- a version range: `sqlite >=3.40 <4`;
- a reviewer list and the header hashes it was checked against;
- its `#Check` and helper functions, as ordinary Jet.

When the project depends on `c@nixpkgs:sqlite`, Jetpack resolves the matching pack the way
TypeScript resolves `@types/*`. It is pinned in `.jet/lock`, and `jet inspect bind` names it.
`packs: { sqlite3: none }` in `package.jet` turns a pack off.

### 5.4 Views and invalidation

A row returning `View<T> from owner` reuses Jet's returned-view provenance (spec, "Ownership and
views"). The pack lists the invalidating calls. The binder checks that each of them takes
`&owner` (or `^owner`). If one does not, the pack is rejected (E3228, PROPOSED), because the
borrow rule would not cover it. A view that cannot be tied to one owner is generation-checked
and returns `ExpiredView` on stale use, as D-FFI-GUEST1 ratified for exports.

### 5.5 Failure: `#Check(fn)`

`#Check(fn)` names a Jet function. It takes the raw result and, optionally, the row's handle
argument, and returns `E!` (unit-fallible) or `T E!`. The row's Jet signature becomes fallible with `E`. This
is D-CABI-RESULT1's "write an ordinary Jet wrapper", written once in the pack instead of once
per call site. The compiler still invents no translation.

### 5.6 `#Unsafe` only where Jet has nothing to check (ballot D-FFI-VOUCH2)

| Call | Today | With VOUCH2 |
|---|---|---|
| `lib.fill(&values)` on a row read from a header or pack | yes (D-FFI-CAP1 gate) | no |
| `zlib.gzclose(^gzip)` on a generated row | yes (D-FFI-CAP1 gate) | no |
| a call to an inline `#[Unsafe("…"), FFI(c)]` function | yes, an `#Unsafe` block at the call | no; the declaration's reason is the one site |
| a row inside an overlay's `#Unsafe("…") { }` vouch block | scalar rows: no; `&`/`^` rows: yes | no at the call; the vouch is the one site |
| `ffi.Library.open`, `lib.symbol` on a `.so` with no contract note | yes | yes |
| `mem.Ptr` arithmetic, `p.*` | yes (D-RAWPTR2) | yes |
| a JS package with no `.d.ts` (D-FFI-JS1) | yes | yes |

### 5.7 Callbacks (ballot D-FFI-CALLBACK3)

| Retention fact | Jet value you pass | Captures allowed | Ends |
|---|---|---|---|
| call-scoped | any closure | anything, including `:=` writes | when the call returns |
| retained until `fn` | closure; returns a registration (`Close`) | owned or sendable captures; `:=` locals with `deliver: .Owner` | scope end requests stop and waits for acknowledgment; `source.unsubscribe(^sub)` gives the stop task |
| forever | closure | owned or sendable captures | never; `jet audit foreign` lists it |

How a closure reaches C, with no code generated at run time:

- **The C function takes a context pointer** (`void *user_data`): one shared entry stub per
  signature, and the closure travels in the context pointer. This is D-FFI-PRIMS1's `ffi.thunk`.
- **Call-scoped, no context pointer** (`qsort`): the same stub reads the closure from a
  per-thread slot that the wrapper saves before the call and restores after it, so nested and
  reentrant calls work.
- **Retained, no context pointer** (rare: old signal-style APIs): a fixed table of stubs per
  signature, built ahead of time. A free slot is taken at registration and returned at close.
  When the table is full, registration returns `CallbackError.TableFull`; it never generates
  code, so W^X and iOS rules hold.
- A panic inside a callback never unwinds through C. If the row declares a failure value for
  the callback, the stub returns it to C and the panic resumes at the Jet frame that made the
  outer call. Otherwise Jet prints the report at the crossing and ends the program (G16).

### 5.8 Effects

A row's effects come from facts. A pure fact gives `-[]>`. Unknown gives every root, which is
today's rule. Under `.Wasm` placement each import the sandbox can reach carries its own effect,
so the row's effect set is enforced rather than promised.

## 6. Layer 2: readers and writers

Readers and writers are ordinary packages (D-FFI-BINDER1=A). The rules every reader follows:

### 6.1 Toolchains arrive by themselves

The pinned clang (D-ADOPT-CC1=B), the Rust toolchain (D-FFI-RUST2), CPython (D-DEP-PY1), Node,
the JVM and Go all come from the Jetpack store, hash-checked, on first use. `jet fetch` warms the
store for offline machines.

### 6.2 Read with the library's flags

Flags come from pkg-config, `compile_commands.json`, or the Jetpack recipe. Layouts are compared
with DWARF or BTF when present (E3227 on mismatch).

### 6.3 Caches

Every generated artifact is content-addressed, and the key covers inputs, flags, target, reader
version and pack. Jetpack serves prebuilt caches. Rows are generated lazily, per use.

### 6.4 Size

Wrappers inline, unused rows are dropped, shims hold only used items, and `c@nixpkgs:` links
statically by default.

### 6.5 Speed

- Text crosses with no copy, because Jet text keeps a trailing NUL.
- Lists cross as pointer plus length.
- Proven-unneeded checks are removed.
- A foreign thread attaches once.
- Isolated calls batch.

### 6.6 Versions

The contract fingerprint is checked at build, under freeze (E3225), and at load, before `run`
(E3226).

### 6.7 Targets

Target sysroots come from Jetpack, and nixpkgs cross builds provide the libraries. Apple SDKs
are supplied by the user.

### 6.8 Distribution

Packages publish their dependency line, their pack and their wrapper. `.jet/lock` holds the
accepted plan. `jet audit licenses` lists obligations.

### Writers

Writers turn the export table into each host's idiom (G10, G11). Each writer has a fixed mapping
table:

| Jet | C | Python | JS | Rust | C++ |
|---|---|---|---|---|---|
| `#Codable` struct | `typedef struct` + `_free` | frozen dataclass | class | struct | struct |
| `[T]` | pointer + len | `list[T]` | `Array` | `Vec<T>` | `std::vector<T>` |
| `String` | `{ptr, len}` UTF-8 | `str` | `string` | `String` | `std::string` |
| `T E!` | status + out + error out | exception per variant | thrown subclass | `Result<T, E>` | `std::expected<T,E>` |
| `fn(T)` param | fn pointer + `void*` ctx | `Callable` | function | `impl FnMut` | `std::function` |
| waits (async) | completion callback + cancel | `async def` | `Promise` | `Future` | `std::future` + cancel |
| `Close` resource | opaque + `_close` | context manager | `Symbol.dispose` | `Drop` | RAII class |
| `View<T> from x` | generation-checked view | memoryview, checked | checked view | `&'a T` (true borrow) | checked span |

## 7. Placement: one option (ballot D-FFI-ISOLATE1)

```jet
// package.jet (PROPOSED key)
isolate: {
    fastimg: .Wasm         // C/C++/Rust source compiled to wasm, translated to native at link
    oldcodec: .Process     // any library, including closed binaries, in a supervised child
    zlib: .Contained       // in-process, tracked pointers (today's policy contain: list)
    numpy: .InProcess      // Python embedded (today's py@embed)
}
```

| Placement | Who can use it | A memory bug in the library | Cost per call | Effects |
|---|---|---|---|---|
| `.InProcess` (default for C, C++, Rust) | anything | can corrupt the program | none | promised |
| `.Contained` | source libraries | stops with a report | checked pointers in that library | promised |
| `.Wasm` | source libraries | traps; `IsolateError.Trapped`; fresh instance | ~1.1–1.5x inside the library | enforced |
| `.Process` (default for Python, Ruby, Perl, PHP, R) | anything | kills the child; `IsolateError.Crashed`; restart | ~2–10 µs (messages; shared-memory lists) | enforced by the OS sandbox |

Call sites never change. A row that can already fail gains `IsolateError` in its error set when
its library is `.Wasm` or `.Process`, so an existing `?` or `.Err` arm handles it. A row that
cannot fail keeps its signature: a trap there is a clean Stop of the calling task, with no Jet
memory touched, and a task boundary or `#Check` row turns it into a value. Handles become
generation-checked IDs into the sandbox; a handle from an instance that trapped returns
`IsolateError.Stale` instead of reaching the fresh instance. Callbacks cross back as messages.
A row that cannot cross is a build error that names it: for example, a function returning a raw
pointer into its own memory for the caller to keep. `policy: { harden: true }` and
`--profile=hardened` raise every `.InProcess` foreign library to at least `.Contained`, as
ratified.

## 8. Languages, threads, async, exports, web and comptime

### 8.1 Python

Python runs in a supervised worker by default (D-FFI-PY1), now spelled through `isolate:`
(`numpy: .InProcess` replaces `py@embed`). The reader produces typed rows from `.pyi` stubs and
type hints. Without hints, values cross as `py.Object` with checked conversions. Large numeric
lists cross in shared memory, with zero copies, under `.Process` (G8). A Python exception becomes
a Jet error carrying the Python traceback.

### 8.2 Rust

The pinned Rust toolchain for every target, with the API read from rustdoc JSON (G7, ballot
D-FFI-RUST2). Generics are instantiated for the types the program uses. `Result` becomes a Jet
error, `Option` becomes `T?`, and returned references become views tied to their owner.

### 8.3 Threads

Thread facts are `.Any`, `.Caller`, `.Main` and `.Library`. A handle whose fact is `.Caller` or
unknown cannot cross tasks. A `.Main` row called inside `task` is E3231 (G18). A callback from a
foreign thread needs sendable captures, unless the registration says `deliver: .Owner`, which
runs it on the registering task (E3233, G4). errno and `GetLastError` are captured on the calling
thread by the wrapper.

### 8.4 Async

Jet has no async/await; tasks wait at wait points. A foreign completion, whether a C callback,
a JS Promise, a Python coroutine or a Rust Future, is a wait point (`ffi.completion<T>`, §4), so
other tasks keep running. `#Blocking` rows run on the blocking pool. Cancelling the Jet task
cancels the foreign operation where the library supports it, and otherwise waits for it.

### 8.5 Exports

Writers turn the export table into each host's idiom (G10, G11, the table in §6, ballot
D-FFI-EXPORT2). A Jet function that waits is exported as a C completion callback with cancel,
a Python `async def`, a JS `Promise`, a Rust `Future`, or a C++ future.

### 8.6 Web

On the web target, C, C++ and Rust libraries with source compile to wasm and link into the
program. JS libraries are read from `.d.ts` files (D-FFI-JS1, G9). Closed binaries are refused
with an error that names the library.

### 8.7 Comptime

Comptime calls a foreign row only when the row is pure, its Jet row has fixed-width types (after
a pointer and count collapse to a list), its source is available, and it builds for wasm32. The
call runs in a fresh wasm sandbox inside the compiler's comptime child process (I9), so the
result does not depend on the build machine. The same C can still differ between wasm and a
native target: float contraction, `long` width and undefined behaviour compile differently. So
comptime-eligible sources build with strict float flags (`-ffp-contract=off`), and `jet test`
replays each recorded comptime call natively on each target and fails, naming the row, on any
difference (G17, ballot D-FFI-COMPTIME1). Every other foreign call at comptime is refused with
the reason. This amends the spec rule "compile-time code links no foreign functions".

## 9. Debugging

- One backtrace shows Jet and foreign frames, with each crossing marked by row and shim (G16).
- `jet perf run <app> --lens=ffi` records crossings into the existing `.jettrace`;
  `jet perf view <file> --lens=ffi` summarizes counts, time, placement and bytes copied, and
  `--lens=ffi:calls` keeps each call (ballot D-FFI-TRACE1).
- `jet inspect bind <lib> --row <name> --expand` shows a wrapper as ordinary Jet (G17).
- `jet inspect bind <lib> --explain` shows every fact with its source (D-FFI-AUTO1).
- Native frames are symbolized from DWARF; Jetpack builds keep debug info in a side file.
- A stop inside a foreign library names the last crossing and its thread (E3236, G12).

## 10. Cross-cutting rules

1. **Names stay foreign.** Rows use the library's spelling (D-SHAPE-CASE2), so docs and search
   work. Packs may add Jet method rows (`fn Database.exec`), and the C name is always shown in
   hover and `inspect bind`.
2. **One `use` form per library per file** (E3204), the same `<lang>.<lib>` structure for every
   language (D-FFI-UNIFY1), and no FFI-only import grammar.
3. **Every diagnostic is a Jet diagnostic** with a code, what, why, fix and a snapshot (I2, I4).
   No foreign toolchain message reaches the user unlaundered.
4. **Threads.** Thread facts are checked. Callbacks never move threads silently. errno is
   captured on the calling thread.
5. **Async.** Every foreign completion is a wait point. Cancellation crosses where the foreign
   side supports it, and otherwise waits.
6. **Panics** never unwind through foreign frames. They resume at the Jet frame that made the
   outer call when the callback row has a failure value to return; otherwise Jet reports at the
   crossing and ends the program (§5.7).
7. **One meaning on every tier (I9).** Comptime (native O0 in a child of the compiler), O1 and
   O2 call the same wrapper. Comptime runs only pure fixed-width rows, in a wasm sandbox, and
   `jet test` checks them against native runs. The web target compiles source libraries to wasm
   and refuses closed binaries with a named error.
8. **Audit.** `jet audit foreign` lists every row whose facts are vouched, packed or unknown,
   with its source. `--profile=hardened` (D-HARDENED1) places every in-process foreign library
   at least `.Contained`, as `policy: { harden: true }` already does (D-MEM-GUARANTEE1).

## 11. What exists today and what is proposed

Code facts come from the dev worktree at df84747616.

| Capability | Today (code) | v2 |
|---|---|---|
| `use "x.h"` header binding | hand-written prototype parser (`HeaderParser.jet`): scalars, `char*`↔`String`, C-layout records, opaque handles, `#define` constants | pinned Clang reader (D-CIMPORT1, ratified, unbuilt), library flags, DWARF check |
| native C call lowering | direct scalar calls, `String` copied, `&` scalar and `&` struct out-slots (`BuildForeign.jet`); callbacks, close adapters and variadics refused | all shapes in G3–G5 |
| `use "src/x.c"` | link closure compiles `.c` inputs (`ForeignLink.jet`) | Clang reads the source (D-FFI-CINPUT1, ratified) |
| `#FFI(c)`, `#FFI(asm)` | built (`inline_c.jet`, `inline_asm.jet`) | unchanged; E3222 names both sides |
| `#Close(fn)` on handles | built in bindgen output; scope-end `Close` built | also on text and list results |
| facts in header comments | fixture format in `Examples/ffi/adaptations/*.h` | one of five fact sources |
| packs, `#Check`, foreign view returns, method rows | none | PROPOSED (D-FFI-PACK1) |
| capturing callbacks | E3203 (`tests/ui/cffi_callback_capture`) | call-scoped closures (D-FFI-CALLBACK3) |
| retained callbacks | fixture `Examples/ffi/callbacks/` (D-FFI-CALLBACK2) | scope-end stop |
| C++ | clang binder and shims (`Binders/Cpp*.jet`); flags and `--instantiate` by hand | automatic instances, pinned toolchain |
| Rust | bridge (`RustBridge/`, `RustForeign.jet`); musl x86_64; cargo external | every target, rustdoc reader (D-FFI-RUST2) |
| Python, JS, others | binders, scalar-only; sidecar supervisor | stubs, objects, arrays, shared memory |
| `#Export(c)` | scalars (`Guest.jet`, `JetLib.jet`); projections pass records as JSON | typed per-host writers (D-FFI-EXPORT2) |
| plugins | wasm component host (`PluginHost.jet`), `core.plugin` | plus native plugins with contract notes |
| `core.ffi` | not built | D-FFI-PRIMS1 (ratified) |
| placement | none for C; sidecar for Python and others | one `isolate:` key (D-FFI-ISOLATE1) |
| `jet audit foreign` | built (`AuditForeign.jet`) | lists pack and unknown facts too |
| contract freeze, load check, trace | descriptor stamp and digest only; `jet perf` and `.jettrace` ratified (D-PERFSESSION1) | E3225, E3226, an `ffi` lens on `jet perf` (D-FFI-TRACE1) |
| comptime foreign calls | refused | pure rows in wasm (D-FFI-COMPTIME1) |

## 12. Adoption, mapped onto decisions

| Decision | State | v2 position |
|---|---|---|
| D-FFI-UNIFY1, D-FFI-ONE1, D-FFI-BINDER1, D-FFI-RUSTIMPORT1, D-FFI-CINPUT1, D-CIMPORT1 | ratified | kept as written |
| D-FFI-CMAP1 (`T?`, counted lists) | ratified | kept; G3 |
| D-FFI-CAP1 (`&`, `^`, `#Close`) | ratified | kept; `#Close` extended to text and lists (D-FFI-PACK1) |
| D-FFI-CONTRACT1, D-FFI-AUTO1, D-FFI-GUEST1, D-FFI-CALLBACK2 | ratified | kept; packs are a fact source under CONTRACT1; G15 uses AUTO1's freeze |
| D-FFI-PRIMS1 (`core.ffi`) | ratified | kept; G14, G17 |
| D-FFI-CPP1, D-FFI-JS1, D-EMBED2 | ratified | kept; G6, G9, G10 |
| D-FFI-VOUCH1, D-FFI-INLINE1 (inline `#FFI` call-site `#Unsafe` gate) | ratified | **amend**: checked `&`/`^` calls and calls to inline `#FFI` functions need no call-site `#Unsafe` (D-FFI-VOUCH2). The ratified note is the weaker choice. |
| D-FFI-RUST1 (musl x86_64, external Cargo) | ratified | **amend**: pinned toolchain on every target (D-FFI-RUST2). The ratified option is the weaker choice. |
| D-CABI-CALLBACK1 (capture-free; no heap, mutable static or thread-local state; no panic path; program-lifetime pointer; no hidden context) | ratified | **amend**: call-scoped closures that capture, a per-thread slot or a prebuilt stub table instead of "no hidden context", a recorded panic instead of "no panic path", and scope-end stop instead of "program lifetime" (D-FFI-CALLBACK3). The ratified rule is the weaker choice. |
| spec "Inline native functions": compile-time code links no foreign functions | spec rule | **amend**: pure fixed-width rows with source run at comptime in a wasm sandbox, checked against native runs by `jet test` (D-FFI-COMPTIME1) |
| D-PERFSESSION1 (`jet perf`, `.jettrace`) | ratified | kept; D-FFI-TRACE1 adds an `ffi` lens |
| D-FFI-PY1 (sidecar default, `py@embed`) | ratified | **amend spelling only**: placement through the one `isolate:` key; the default is unchanged (inside D-FFI-ISOLATE1) |
| D-MEM-GUARANTEE1 (`contain: [deps]`) | ratified | **amend spelling only**: `.Contained` placement (inside D-FFI-ISOLATE1) |
| D-FFI-ISOLATE1 | open | reworked ballot |
| D-FFI-EXPORT2 | open | reworked ballot |
| D-FFI-PACK1, D-FFI-COMPTIME1, D-FFI-TRACE1 | new | new ballots |

Implementation order once ratified, each step usable on its own:

1. Clang reader, flags, and DWARF check.
2. `#Check`, `#Close` on text, view returns, and packs (with sqlite, libpng, raylib, libc and
   libm packs).
3. Call-scoped closures.
4. VOUCH2.
5. Rich exports for C and Python.
6. The `isolate:` key, with `.Wasm` on the existing plugin translator.
7. Load check and trace.
8. Rust on every target.
9. Comptime.

## 13. Sources

- Zig `@cImport`: https://ziglang.org/documentation/master/#cImport
- Rust bindgen: https://rust-lang.github.io/rust-bindgen/
- Rust cxx: https://cxx.rs/
- Rust PyO3: https://pyo3.rs/
- rusqlite: https://github.com/rusqlite/rusqlite
- libloading: https://docs.rs/libloading
- Swift C interop and API notes: https://www.swift.org/documentation/cxx-interop/ and
  https://clang.llvm.org/docs/APINotes.html
- C# `LibraryImport`: https://learn.microsoft.com/dotnet/standard/native-interop/pinvoke-source-generation
- Python cffi: https://cffi.readthedocs.io/
- LuaJIT FFI: https://luajit.org/ext_ffi.html
- Julia PythonCall: https://juliapy.github.io/PythonCall.jl/
- Go cgo: https://pkg.go.dev/cmd/cgo
- Mojo Python interop: https://docs.modular.com/mojo/manual/python/
- RLBox: https://rlbox.dev/
- Clang bounds-safety `counted_by`: https://clang.llvm.org/docs/BoundsSafety.html
- GCC `malloc (deallocator)`: https://gcc.gnu.org/onlinedocs/gcc/Common-Function-Attributes.html
- DefinitelyTyped: https://github.com/DefinitelyTyped/DefinitelyTyped
- Java FFM API (JEP 454): https://openjdk.org/jeps/454
