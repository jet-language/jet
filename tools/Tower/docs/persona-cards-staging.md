# Persona-run cards — staging (for main agent to merge)

Source: `docs/plans/persona-status/2026-06-20.md`. Built per the tower-sweep
house rules. **Do not paste blindly** — the main agent merges Section A into
`board.json` and Section B into `ballots/decision-ballots.md` (single-writer).

Existing card ids go to c82; benchmark/plugin are c80/c81; regex is c79.
New ids proposed here start at **c83**. All decision ids below were checked
against `syntax-decisions.md` — none collide with a ratified id.

---

## Section A — Proposed board cards

| Suggested id | Title | Stage | Plan slug | Decisions | Note |
|---|---|---|---|---|---|
| **c83** | HTTP routing layer / middleware (`jet.http`) | pre-plan | `http-routing` | D-ROUTE1 | Tariq, Amara. Persona rec #2. |
| **c84** | Detached-task idiom — fix L1101 on servers | pre-plan | `task-detach` | D-DETACH1 | Tariq + any server. Persona rec #3. |
| **c85** | `repr(C)` struct layout control | pre-plan | `repr-c-layout` | D-REPRC1 | Yuki, Marcus. Persona rec #4. Interacts D-SOA1, c82. |
| **c86** | Streaming / line-by-line stdin | pre-plan | `streaming-stdin` | D-STDIN1 | Priya, Elena. Persona rec #7. File `.lines()` already exists; stdin doesn't. |
| **c87** | Terminal raw-mode + single-key input | pre-plan | `terminal-raw-mode` | D-TERM1 | Kofi (only *blocked* persona). Persona rec #8. I6 dep gate. |
| **c88** | `fs.list_dir` full paths + path join | pre-plan | `fs-listdir-paths` | D-LSDIR1 | Priya. From per-persona Push column. |
| **c89** | Typed CSV row structs | pre-plan | `typed-csv-rows` | D-CSVROW1 | Elena. Must not block on S56. |
| **c90** | Typed JSON output / struct serialization | pre-plan | `typed-json-output` | D-JSONOUT1 | Elena. Confirm built-in `#[Serialize]` marker status vs S56. |
| **c91** | Structured flag/argument parsing | pre-plan | `structured-arg-parsing` | D-ARGS1 | Amara, Priya. Distinct from D-CLI1 passthrough (c11). |
| **c92** | Human-readable log format for `jet.log` | pre-plan | `human-log-format` | D-LOGFMT1 | Amara. |
| **c93** | Sized floats `F32`/`F64` impl + precision math | pre-plan | `sized-floats` | D-FLOATW1 | Marcus. Spelling RATIFIED (D-SG9) but UNBUILT; one open precision/math Q. |
| **c94** | Linear algebra + SIMD math story | far-horizon | `math-linalg-simd` | D-MATHLIB1, D-SIMD1 | Marcus. Large; I6 dep + roadmap-slot gates. Surfaced, not urgent. |
| **c95** | C-header binding example (`use c.<lib>`) | pre-plan | `cbind-example` | — (no decision) | Yuki. Pure I5 gap; D-CBIND3/c53 shipped. Implement-only — could go straight to planned/implementation. |
| **c96** | M12.2 registry + `jet publish` UX | pre-plan | `publish-registry-ux` | D-PUBLISH1 | Saoirse, Amara. Rides c50/c56 infra; UX is the open call. |

### ALREADY TRACKED — do NOT create new cards (see Section C for detail)

- `jet.regex` (persona rec #1) → **ALREADY TRACKED: c79 / D-REGEX1 (ratified, in implementation).**
- External `Struct::method` (Saoirse, Dani; persona rec #6) → **ALREADY TRACKED: S83 (open ballot).**
- Comptime user-defined derives (persona rec #5) → **ALREADY TRACKED: S56 (deferred to Epoch 3).**
- Registry infra / build-from-source / M9 wave-2 → **ALREADY TRACKED: c50, c56, S52.** (c96 is the publish-UX layer on top, not a dup of infra.)
- TLS for HTTP → **ALREADY TRACKED: D-NET1 (`jet.tls` wraps rustls), ratified.**
- Custom allocator trait (Marcus) → **ALREADY TRACKED: D-REF2 (open), noted on c05.**
- Async/await, `select`/non-blocking receive (Kofi, Dani) → **deferred to Epoch 3** (persona doc + roadmap); no new card — flag for owner if he wants it carded.

---

## Section B — Proposed decision cards (house format, ballot-ready)

> Order them under their board card when merging into `decision-ballots.md`.
> All carry user story + tradeoff table + per-option worked example +
> recommendation. No effort/difficulty column anywhere.

---

### D-ROUTE1 — HTTP route registration & dispatch surface (rec A)

**User story.** Tariq is porting a Go `net/http` service to Jet. He has ten
endpoints — `GET /users/:id`, `POST /orders`, a health check. Today `jet.http`
gives him one handler closure and he branches on `request.path` with a growing
`if/match` ladder; `:id` extraction is manual string-splitting. He wants to
register routes and have the right handler called with `:id` already parsed.

| Option | Registration | Param access | Glance-readable route map | Beginner read |
|---|---|---|---|---|
| A — builder chain | `router.get(path, h)` | `req.param("id")` | yes — one place | clear |
| B — route table value | a `[Route]` literal | `req.param("id")` | yes — declarative | clear |
| C — handler attribute | `#route("GET","/u/:id")` on fn | typed handler args | scattered across fns | medium |
| D — match block | `route req { GET "/u/:id" -> … }` | bound in pattern | yes — but new syntax | high (familiar `match`) |

- **Option A — builder method chain.** A `Router` value collects routes; handlers
  read params from the request.

```jet
fn main() {
    router :: http.Router.new()
    router.get("/users/:id", get_user)
    router.post("/orders", create_order)
    router.get("/health", |req| http.ok("ok"))
    http.serve(":8080", router)
}

fn get_user(req: http.Request) -> http.Response {
    id :: req.param("id")              // "42" — extracted from /users/:id
    http.json(lookup(id))
}
```

- **Option B — declarative route table.** Routes are a value, handlers referenced
  by name.

```jet
routes :: [
    http.route(GET,  "/users/:id", get_user),
    http.route(POST, "/orders",    create_order),
    http.route(GET,  "/health",    health),
]
http.serve(":8080", routes)
```

- **Option C — handler attribute.** Each handler declares its own route; the
  framework collects them.

```jet
#route(GET, "/users/:id")
fn get_user(req: http.Request, id: String) -> http.Response {
    http.json(lookup(id))              // :id arrives as a typed arg
}
// routes live next to their handlers, but there is no one place
// to read the whole route map.
```

- **Option D — match-style routing block.** A dedicated routing construct.

```jet
http.serve(":8080", |req| route req {
    GET  "/users/:id" (id) -> http.json(lookup(id)),
    POST "/orders"          -> create_order(req),
    GET  "/health"          -> http.ok("ok"),
    _                       -> http.not_found(),
})
// new grammar; reads like `match`, but adds routing syntax to the language (I8).
```

**Recommendation:** A — a `Router` builder keeps routing a *library* (no grammar
change, honoring I8), gives one readable place for the route map, and the
`req.param` access generalizes to query/header params. B is a fine declarative
peer; D buys familiarity at the cost of new syntax the simplicity ratchet resists.

---

### D-DETACH1 — Marking a task as intentionally detached (silence L1101) (rec A)

**User story.** Tariq spawns his HTTP server on a task so `main` can keep doing
setup. Every server program he writes lights up **L1101** ("Task value dropped
without `.join()`") — including the shipped `57_http_server.jet`. The warning is
right for an accidental drop but wrong here: he *wants* the server task to outlive
the spawn scope. He needs a one-word "I meant this."

| Option | Surface | Capture safety enforced | Reads as intent | One verb |
|---|---|---|---|---|
| A — `task.detach()` | method on handle | yes (owned/`share` only) | yes — explicit verb | yes |
| B — `#detach` marker on spawn | attribute (D-ATTR1) | yes | yes — leads the spawn | yes |
| C — `detach { … }` block | parallel to `spawn { … }` | yes | yes — but two spawn forms | no (two verbs) |
| D — `spawn(detached: true)` | named arg | yes | trailing flag, easy to miss | yes |

- **Option A — `.detach()` on the task handle.** Spawn returns a handle; calling
  `.detach()` consumes it and exempts it from L1101.

```jet
fn main() {
    server :: spawn { http.serve(":8080", router) }
    server.detach()        // "runs on its own; don't warn me"
    log.info("server up")
    // no L1101 — the drop was declared intentional
}
```

- **Option B — `#detach` marker on the spawn.** The intent leads the statement.

```jet
fn main() {
    #detach spawn { http.serve(":8080", router) }
    log.info("server up")
}
```

- **Option C — a dedicated `detach { … }` block.** A second spawn verb whose
  result is never joinable.

```jet
fn main() {
    detach { http.serve(":8080", router) }   // distinct from `spawn { … }`
    log.info("server up")
    // two spawn constructs to teach; which one do I reach for?
}
```

- **Option D — a named arg on spawn.** A flag selects detached mode.

```jet
fn main() {
    spawn(detached: true) { http.serve(":8080", router) }
    // the intent is a trailing boolean; in review it's easy to miss.
}
```

In every option, a detached task that captures a borrowed `view` of the caller's
scope is a compile error (it would outlive the borrow):

```jet
fn run(cfg: view Config) {
    spawn { serve(cfg) }.detach()
    // error[Lxxxx]: a detached task may not capture the borrow `cfg` (view)
    //   it can outlive the scope `cfg` is borrowed from
    //   help: pass an owned copy — `spawn { serve(copy cfg) }` — or `share cfg`
}
```

**Recommendation:** A — `.detach()` is a single explicit verb on the value, reads
as a deliberate choice in review, and is the natural place to quote in the L1101
fix-it ("if intentional, call `.detach()`"). It keeps one spawn verb (unlike C)
and is leading-visible (unlike D).

---

### D-REPRC1 — C-compatible struct layout annotation (rec A)

**User story.** Yuki is writing ARM firmware. She needs a Jet struct that overlays
a memory-mapped peripheral register block — exact field order, C padding, no
reordering — so an `@unsafe` volatile cast onto the MMIO address is sound. Today
struct layout is opaque, so she can't reliably interop with C structs or hardware.

| Option | Spelling | Family it joins | Modes | Beginner sees it's expert |
|---|---|---|---|---|
| A — `#repr(c)` | attribute (D-ATTR1) | markers; near `@unsafe` | `c`, `packed`, `align(N)`, `transparent` | yes — clearly an annotation |
| B — `#layout(c)` | attribute | same family as D-SOA1 `#layout(soa)` | layout kinds | yes |
| C — `c struct Foo` | type modifier keyword | none | only `c` | medium |
| D — `extern(c) struct` | extern modifier | FFI `extern` family | only `c` | yes — ties to FFI |

- **Option A — `#repr(c)` attribute (+ `packed` / `align(N)`).** Pins layout;
  codegen stamps `#[repr(C)]` on the generated Rust struct.

```jet
#repr(c)
struct GpioRegs {
    mode:   U32,
    output: U32,
    input:  U32,
}

fn read_input(base: U64) -> U32 {
    @audit("MMIO read of GPIO input register at a fixed peripheral address")
    @unsafe {
        regs :: mem.cast<GpioRegs>(base)   // sound: layout is pinned
        mem.volatile_read(regs.input)
    }
}

// a growable field breaks the guarantee:
#repr(c)
struct Bad { tag: U32, items: [U32] }
// error[E04xx]: field `items: [U32]` has no stable C layout
//   help: use a fixed-size array `[U32#N]`, or remove `#repr(c)`
```

- **Option B — `#layout(c)`, unifying with SOA.** C-repr and SOA become one
  `#layout(…)` family.

```jet
#layout(c)
struct GpioRegs { mode: U32, output: U32, input: U32 }
// one annotation family also spells #layout(soa) (D-SOA1) and #layout(packed).
```

- **Option C — `c struct` modifier keyword.** Layout is a struct-declaration
  modifier.

```jet
c struct GpioRegs { mode: U32, output: U32, input: U32 }
// terse, but adds a bare keyword in type position and has no room for
// packed/align variants without more keywords.
```

- **Option D — `extern(c) struct`.** Ties layout to the FFI surface.

```jet
extern(c) struct GpioRegs { mode: U32, output: U32, input: U32 }
// reads as "this struct crosses the C boundary"; conflates layout with FFI,
// so a pure-Jet struct that just wants packed layout has nowhere to go.
```

**Recommendation:** A — `#repr(c)` matches the ratified attribute/marker family
(D-ATTR1), sits visually next to the other expert markers (`@unsafe`/`@audit`),
and has obvious room for `packed`/`align(N)` that firmware needs. B is a strong
alternative *if* the owner wants one `#layout(…)` family shared with D-SOA1 —
that cross-cutting choice (repr and SOA together vs separate) is the real fork
and worth deciding alongside D-SOA1.

---

### D-STDIN1 — Streaming line-by-line stdin (rec A)

**User story.** Priya writes a grep-like filter: `cat huge.log | jet run filter.jet`.
Today `io.read_all_input()` reads *all* of stdin into memory, then she splits it
by hand. Files already stream (`reader.lines()` works), but stdin has no such
path. She wants stdin to stream lines the same way files do, constant-memory.

| Option | Spelling | Same type as files? | Convenience | One idiom |
|---|---|---|---|---|
| A — `io.stdin().lines()` | stdin handle mirrors `files.open` | yes — reuses `FileLines` | medium | yes (files+stdin interchangeable) |
| B — bare `io.lines()` | top-level convenience | yes under the hood | high | a second spelling beside files |
| C — `io.read_lines()` | returns an iterator value | maybe | high | a third verb |

- **Option A — `io.stdin()` handle with `.lines()` / `.read_line()`.** Mirrors the
  file reader exactly, so a function can take either source.

```jet
fn main() {
    loop line in io.stdin().lines() {
        if line.contains("ERROR") { print(line) }
    }
}
// same .lines() the file reader uses (CheckerStdlib FileLines); a function
// written against a file reader also accepts stdin.
```

- **Option B — bare `io.lines()`.** A direct convenience for the common case.

```jet
fn main() {
    loop line in io.lines() {           // implicitly stdin
        if line.contains("ERROR") { print(line) }
    }
}
// terse, but "lines of what?" is implicit, and it's a separate spelling
// from the file `reader.lines()` users already learned.
```

- **Option C — `io.read_lines()` returning an iterator.** A new verb alongside
  `read_all_input`.

```jet
fn main() {
    loop line in io.read_lines() {
        print(line)
    }
}
// pairs by name with read_all_input, but adds a third reading verb and
// doesn't reuse the file streaming type.
```

A `pure fn` reading stdin stays rejected (stdin is impure, like `input`):

```jet
pure fn count() -> Int {
    n :: 0
    loop _ in io.stdin().lines() { n += 1 }   // error: pure fn reads stdin (impure)
    n
}
```

**Recommendation:** A — reusing the file reader's `.lines()`/`FileLines` gives
*one* streaming idiom across files and stdin (a function written for one accepts
the other), which is the strongest one-path outcome. `read_all_input` stays as a
small-input convenience.

---

### D-TERM1 — Terminal raw-mode + key input surface (rec A)

**User story.** Kofi is building a terminal puzzle game — the one persona whose
verdict is *blocked*, not just friction. He needs to read an arrow key without
Enter, move the cursor, and print color, all from one file. `core.io` is
line-based, so today he cannot write a game loop at all. He wants a small
terminal API that puts the terminal in raw mode and restores it automatically.

| Option | Surface | Auto-restore | Key model | Scope |
|---|---|---|---|---|
| A — `raw_mode { … }` scoped block | block guarantees restore | yes — on scope exit (incl. panic) | `Key` enum | minimal: raw + key + cursor + color |
| B — `Terminal` handle value | methods on a handle | via scope-guard the user holds | `Key` enum | configurable |
| C — `core.term` free functions | enter/exit + read funcs | manual `term.restore()` | `Key` enum or bytes | minimal |
| D — full TUI module | screen/widget abstraction | yes | rich events | large (alt-screen, mouse, resize) |

- **Option A — `raw_mode { … }` scoped block (rec).** Raw mode is entered for the
  block and *guaranteed* restored on exit (built on the ratified scope-guard,
  D-DEFER1).

```jet
fn main() {
    raw_mode {
        term.clear()
        loop {
            term.move_to(0, 0)
            term.write("press a key (q to quit): ".green())
            match term.read_key() {
                Key.Char('q') -> break,
                Key.Arrow(dir) -> term.write("arrow: {dir}"),
                Key.Char(c)    -> term.write("you pressed {c}"),
                else           -> {},
            }
        }
    }
    // terminal is back in cooked mode here, even if the loop panicked
}
```

- **Option B — a `Terminal` handle with methods.** The user holds the handle and a
  guard.

```jet
fn main() {
    t :: term.enter_raw()           // returns a handle + restores via its guard
    loop {
        match t.read_key() { Key.Char('q') -> break, else -> {} }
    }
}
// flexible, but the restore depends on the handle's guard surviving every path;
// a beginner can drop it on an early return and wedge their terminal.
```

- **Option C — `core.term` free functions.** Explicit enter/exit.

```jet
fn main() {
    term.enter_raw()
    loop { match term.read_key() { Key.Char('q') -> break, else -> {} } }
    term.restore()        // MUST be called on every exit path, by hand
}
// forgetting restore() (or a panic before it) leaves the terminal broken —
// the exact footgun a beginner game author will hit.
```

- **Option D — a full TUI module.** Alt-screen, widgets, mouse, resize events.

```jet
fn main() {
    app :: tui.App.new()
    app.on_key(|k| if k == Key.Char('q') { app.quit() })
    app.run()
}
// powerful, but far past what "a small terminal game" needs; large surface,
// many decisions, slower to give Kofi anything playable.
```

**Recommendation:** A — the scoped `raw_mode { }` block makes auto-restore a
*language guarantee*, not a discipline, which is exactly right for a beginner
games persona who must not be able to wedge their terminal. `Key` as an enum makes
input teachable. (The I6 question — native termios vs a bootstrap crate — is an
implementation choice on top, flagged in the plan, not a user-facing fork.)

---

### D-LSDIR1 — Directory listing: paths, not just names (rec A)

**User story.** Priya writes her first Jet tool: scan a directory and rename
files. `fs.list_dir(dir)` hands her bare names, so she rebuilds each full path
with `"{dir}/{name}"` — fragile, and on the wrong OS the separator is wrong. She
wants the scan to give her something she can act on directly.

| Option | What `list_dir` gives | Path-join help | `is_dir` without re-stat | Behavior change |
|---|---|---|---|---|
| A — `DirEntry` values | `{name, path, is_dir}` | path built for you | yes | yes (return type changes) |
| B — full-path strings | `[String]` full paths | implicit | no | yes (values change) |
| C — names + `path.join` | `[String]` names + helper | explicit `path.join` | no | none (additive) |

- **Option A — `list_dir` returns `[DirEntry]`.** Each entry carries name, full
  path, and type.

```jet
fn main() ? {
    loop entry in fs.list_dir("./logs")? {
        if entry.is_dir { continue }
        fs.rename(entry.path, "{entry.path}.bak")?   // full path, ready to use
    }
}
```

- **Option B — `list_dir` returns full-path strings.**

```jet
fn main() ? {
    loop path in fs.list_dir("./logs")? {            // each is "./logs/app.log"
        fs.rename(path, "{path}.bak")?
    }
}
// no is_dir without a separate fs.is_dir(path) call.
```

- **Option C — keep names, add `path.join`.**

```jet
fn main() ? {
    dir :: "./logs"
    loop name in fs.list_dir(dir)? {                 // bare names, as today
        path :: path.join(dir, name)                 // portable join
        fs.rename(path, "{path}.bak")?
    }
}
// additive (nothing existing changes), but the user still threads dir+name
// by hand on every scan.
```

**Recommendation:** A — `DirEntry` gives a beginner the path *and* `is_dir` in one
step, which is what nearly every scan actually needs (filter dirs, act on files),
and removes a whole class of separator bugs. It is a return-type change to a
shipped function — call that out — but the persona task (scan + act) is the
canonical first tool, so getting it right beats source-compat. A `path.join`
helper (C) is still worth shipping *alongside* for the cases A doesn't cover.

---

### D-CSVROW1 — Typed CSV row decoding (rec A)

**User story.** Elena runs a CSV→JSON ETL. `jet.csv` hands her each row as
`[String]`, so she pulls fields by index (`row[2].to_int()`), guessing at columns
and re-counting when the file changes. She wants to declare a row as a struct and
decode records into it by header name, with a clean per-row error she can skip
with `??`.

| Option | How fields map | Needs S56 derives? | Robust to column reorder | Failure shape |
|---|---|---|---|---|
| A — comptime `decode<Row>` | by field name via comptime reflection | no (uses S57/S60 comptime) | yes (header mapping) | typed row error |
| B — explicit mapping closure | user writes `Row{ id: r[0]… }` | no | no (positional) | user-chosen |
| C — `#[CsvRow]` derive | derive generates decoder | **yes (blocked on S56)** | yes | typed |

- **Option A — `csv.decode<Order>(record)` via comptime field reflection.** The
  compiler walks `Order`'s fields (comptime is shipped, S57/S60) and maps columns
  by header name, coercing types.

```jet
struct Order { id: Int, customer: String, total: Float }

fn main() ? {
    loop record in csv.rows("orders.csv")? {
        order :: csv.decode<Order>(record) ?? continue   // skip malformed rows
        emit(order)
    }
}
// a bad cell:
// row 14, column `total`: cannot read "N/A" as Float  → ?? skips this row
```

- **Option B — explicit mapping closure.** The user writes the field map; no
  reflection.

```jet
fn main() ? {
    loop r in csv.rows("orders.csv")? {
        order :: Order{ id: r[0].to_int()?, customer: r[1], total: r[2].to_float()? } ?? continue
        emit(order)
    }
}
// total control, but indices are back and a column reorder silently corrupts.
```

- **Option C — `#[CsvRow]` derive.** A derive generates the decoder.

```jet
#[CsvRow]
struct Order { id: Int, customer: String, total: Float }
// order :: csv.decode<Order>(record)?
// error: user-defined derives (S56) are not available until Epoch 3
```

**Recommendation:** A — comptime `decode<Row>` gives Elena typed, header-mapped
rows *today* (comptime field walk is already shipped) without waiting on the S56
derive system, and the typed per-row error composes with the ratified `??` skip
idiom. C is the eventual ergonomic spelling once S56 lands; ship A now, and if A's
comptime decode and a future `#[CsvRow]` derive both exist they should produce the
*same* decoder (one path).

---

### D-JSONOUT1 — Serialize a typed struct to JSON (rec A)

**User story.** Elena's ETL ends by emitting JSON. `json.render` takes the dynamic
`JSON` enum, so she hand-builds `Object([("id", Number(o.id)), …])` for every
struct — verbose and drift-prone. She wants `json.render(order)` to just work when
`order: Order`.

| Option | Mechanism | Needs S56? | One annotation for in+out | Field rename |
|---|---|---|---|---|
| A — built-in `#[Serialize]` marker | compiler honors via comptime field walk | no | yes (drives decode too) | via `#json("name")` |
| B — explicit `to_json(self)` method | user writes per type | no | no | manual |
| C — S56 user-derive | user-written derive | **yes (blocked)** | yes | yes |

- **Option A — built-in `#[Serialize]` marker.** A *built-in* marker (distinct from
  S56 user-derives; D-ATTR2 ratified the bare-marker form) tells the compiler to
  generate render (and decode) by walking fields.

```jet
#[Serialize]
struct Order { id: Int, customer: String, total: Float }

fn main() {
    order :: Order{ id: 7, customer: "Mara", total: 19.5 }
    print(json.render(order))      // {"id":7,"customer":"Mara","total":19.5}
}

#[Serialize]
struct Bad { cb: fn(Int) -> Int }
// error: `Bad` is not serializable — field `cb` is a function
```

- **Option B — explicit `to_json` method.**

```jet
impl Order {
    fn to_json(view self) -> JSON {
        JSON.object([("id", JSON.num(self.id)), ("customer", JSON.str(self.customer)),
                     ("total", JSON.num(self.total))])
    }
}
// works with no compiler help, but every struct re-writes the obvious thing
// and it drifts when a field is added.
```

- **Option C — S56 user-derive.**

```jet
derive Order ~~ Serialize     // S83 connector
// error: user-defined derives (S56) are deferred to Epoch 3
```

**Recommendation:** A — a built-in `#[Serialize]` marker (riding the already-shipped
comptime field walk and the ratified bare-marker form) closes the gap now without
S56, and the *same* marker should drive both `json.render` and typed decode so one
annotation covers in and out. **Owner must confirm** whether a built-in Serialize
marker is intended distinct from S56 user-derives before this ratifies — that
boundary is the real question.

---

### D-ARGS1 — Structured command-line argument parsing (rec A)

**User story.** Amara replaces a bash script with Jet. Her tool takes
`--input file`, `--verbose`, and a positional command. `io.args()` gives her a raw
`[String]`; she writes the flag loop by hand and her `--help` is a `print`. She
wants to declare the flags her tool accepts and get typed values plus a generated
`--help` and good errors for free.

| Option | Spec form | Needs S56/comptime? | Auto `--help` | Typed values |
|---|---|---|---|---|
| A — builder spec | `args.flag(...).option(...)` | no | yes | yes |
| B — `#[Args]` struct | fields are flags | yes (comptime/S56) | yes | yes |
| C — declarative table | a `[ArgSpec]` value | no | yes | yes |

- **Option A — builder spec value.** Build a spec, parse `io.args()` against it.

```jet
fn main() ? {
    spec :: args.spec()
        .flag("verbose", short: 'v', help: "noisy output")
        .option("input", String, required: true, help: "input file")
        .positional("command", String)
    cli :: spec.parse(io.args())?       // prints generated --help on `--help`
    if cli.flag("verbose") { log.verbose() }
    run(cli.option("input"), cli.positional("command"))
}
// unknown flag:
//   error: unknown flag `--inpt` (did you mean `--input`?)
//   usage: tool [--verbose] --input <file> <command>
```

- **Option B — `#[Args]` struct.** Declare a struct; fields become flags.

```jet
#[Args]
struct Cli {
    #flag(short: 'v')  verbose: Bool,
    #option(required)  input:   String,
    #positional        command: String,
}
// cli :: args.parse<Cli>(io.args())?
// error: deriving the parser needs user-derives (S56), deferred to Epoch 3
```

- **Option C — declarative table.** A value listing the args.

```jet
cli :: args.parse(io.args(), [
    args.flag("verbose", short: 'v'),
    args.option("input", String, required: true),
    args.positional("command", String),
])?
// equivalent to A's data without the builder chain.
```

**Recommendation:** A — a builder spec gives typed values, auto-generated `--help`,
and teaching errors *today* with no dependency on S56, and the spec value can later
back a `#[Args]` struct form (B) once derives land — same parser underneath. The
generated `--help` and error text are product copy and must be snapshot-tested.

---

### D-LOGFMT1 — Human-readable log output for `jet.log` (rec A)

**User story.** Amara runs her automation script in a terminal and reads the log
live. `jet.log` emits JSON lines, so her console is a wall of `{"level":"info",…}`.
She falls back to building strings by hand. She wants the same `log.info(…)` calls
to print a readable line locally, while still emitting JSON when piped to a log
aggregator.

| Option | Default | Selection | Magic level | Risk |
|---|---|---|---|---|
| A — auto by TTY | text on a TTY, JSON when piped | auto + override | highest | surprise if expectation differs |
| B — text default, JSON opt-in | text | `log.setup(format: json)` | medium | prod forgets to switch to JSON |
| C — JSON default (today), text opt-in | JSON | `log.setup(format: text)` | low | beginner sees JSON first |

- **Option A — auto-detect (text on a TTY, JSON when piped).** The logger picks
  format by whether stderr is a terminal; an explicit setting overrides.

```jet
fn main() {
    log.info("starting", port: 8080)
    // interactive terminal:
    //   12:01:03 INFO  starting  port=8080
    // piped (`tool | jq`):
    //   {"ts":"...","level":"info","msg":"starting","port":8080}
}
```

- **Option B — text by default, JSON opt-in.**

```jet
fn main() {
    log.info("starting", port: 8080)              // 12:01:03 INFO starting port=8080
}
// production:
log.setup(format: json)                            // opt in to JSON lines
```

- **Option C — JSON by default (status quo), text opt-in.**

```jet
fn main() {
    log.setup(format: text)                        // must opt in to readable output
    log.info("starting", port: 8080)
}
// without setup, a beginner running locally sees raw JSON — today's friction.
```

**Recommendation:** A — auto-by-TTY is the modern logger behavior (Rust `tracing`,
Go `slog` setups, Python `rich`) and gives a beginner a readable console *and* a
production pipeline JSON *with no configuration*, which is the strongest
beginner-experience + correctness combination. The explicit override stays for
when detection guesses wrong. The text line layout is product copy — snapshot it.

---

### D-FLOATW1 — Precision-correct math on sized floats (rec A)

> Note: the `F32`/`F64` *type spellings* are already ratified (**D-SG9**); they are
> merely unimplemented. This decision is only the **math/precision policy** on top.

**User story.** Marcus runs a numerical simulation where memory and precision
matter. He wants `F32` arrays for half the memory and wants `core.math.sqrt`,
`sin`, etc. to work on them at `F32` precision — and he wants the compiler to stop
him from silently dropping `F64` precision into an `F32` binding.

| Option | Math over widths | Literal into `F32` | Mixed `f32`+`f64` |
|---|---|---|---|
| A — width-generic math, explicit conversion | `sqrt` works per-width, returns same width | explicit `.to_f32()` or exact-rep literal ok | error: convert explicitly (D-SG9) |
| B — f64-only math, convert at call | `sqrt` always f64; convert in/out | implicit narrowing allowed | implicit widen to f64 |

- **Option A — width-generic math + explicit conversions (rec).** `core.math`
  functions accept and return the float width they're given; precision-losing
  moves are explicit, consistent with D-SG9's "no implicit widening, named-method
  conversions."

```jet
xs :: [F32]{ 1.0, 2.0, 3.0 }
ys :: xs.map(|x| math.sqrt(x))     // sqrt(F32) -> F32, full F32 path

a :: 1.0e40                        // a: Float (f64)
b :: F32 = a                       // error: assigning f64 to F32 may lose precision
                                   //   help: write `a.to_f32()` to convert explicitly
c :: 2.0f32 + 3.0                  // error: cannot mix F32 and Float(f64)
                                   //   help: `2.0f32 + (3.0).to_f32()`
```

- **Option B — f64-only math, convert at the boundary.** Math stays f64; sized
  floats are storage only.

```jet
xs :: [F32]{ 1.0, 2.0, 3.0 }
ys :: xs.map(|x| math.sqrt(x.to_f64()).to_f32())   // round-trip through f64
b :: F32 = 1.0e40                                   // silently narrows
// less ceremony, but the f64 round-trip defeats the F32 precision/perf intent
// and silent narrowing is the footgun numerical code most fears.
```

**Recommendation:** A — width-generic math keeps `F32` a real first-class precision
choice (not just storage), and explicit precision-losing conversions match the
already-ratified D-SG9 stance (no implicit widening, named conversions). B
reintroduces exactly the silent-narrowing footgun D-SG9 rejected for casts.

---

### D-MATHLIB1 — Linear-algebra library home & scope (rec A)

**User story.** Marcus needs vectors and matrices — dot, cross, matmul, and ideally
decompositions/FFT — for a physics simulation. Today `core.math` is scalar `Float`
ops only, so he writes matrices from scratch or drops to Rust FFI. He wants a
numerics library that ships with the language.

| Option | Home | Dimensions | v1 scope |
|---|---|---|---|
| A — `jet.linalg` ring package | a ring package (like csv/toml/regex) | comptime-sized + runtime | small vectors + matrices core, FFT later |
| B — `core.math` extension | built into core | runtime-sized | grows core surface |
| C — expert-only `@unsafe` BLAS binding | FFI overlay | runtime | thin wrapper, expert-tier |

- **Option A — `jet.linalg` ring package (rec).** Numerics ships as a first-party
  ring package, consistent with regex/csv/toml.

```jet
use jet.linalg

fn main() {
    a :: Matrix<2,2>{ {1.0, 2.0}, {3.0, 4.0} }     // comptime-sized (rides S76/c82)
    b :: Matrix<2,2>{ {5.0, 6.0}, {7.0, 8.0} }
    print((a * b).trace())                          // 67.0
    v :: Vec3{ 1.0, 0.0, 0.0 }
    print(v.cross(Vec3{ 0.0, 1.0, 0.0 }))           // Vec3{0,0,1}
}
```

- **Option B — extend `core.math`.** Matrices live in core.

```jet
use core.math
m :: math.Matrix.new(2, 2)        // runtime-sized only
// pulls a large numerics surface into core, which every program carries.
```

- **Option C — expert `@unsafe` BLAS binding.** A thin FFI wrapper, expert-tier.

```jet
use jet.blas
@unsafe { jet.blas.dgemm(a, b, out) }   // raw, fast, expert-only — no beginner path
```

**Recommendation:** A — a `jet.linalg` ring package keeps core small (I8), matches
how regex/csv/toml already ship, and can offer comptime-sized matrices (riding the
fixed-array work, c82/S76) for the cache-friendly, bounds-checked layout numerical
code wants. The I6 question (native vs bootstrap a numerics crate) is an
implementation gate flagged in the plan, decided like regex (c79).

---

### D-SIMD1 — SIMD primitive surface & safety tier (rec A)

**User story.** Marcus has a hot kernel adding two large `F32` arrays. He wants it
vectorized. Today there are no SIMD types — the expert tier gives him raw pointers
(`@unsafe`/`Ptr<T>`) but no lanes, so he can't write portable vector math.

| Option | Surface | Safety | Portability |
|---|---|---|---|
| A — safe portable lane types `F32x4` | explicit lane values + ops | safe by default (`std::simd`) | portable; falls back when ISA absent |
| B — auto-vectorize hint on a loop | `#vectorize loop …` | safe | compiler-best-effort |
| C — target intrinsics behind `@unsafe` | raw arch intrinsics | expert-only `@unsafe` | per-target |

- **Option A — portable lane types (rec).** First-class `F32x4`/`F64x2` with safe
  ops that lower to portable SIMD.

```jet
fn add(xs: [F32], ys: [F32], out: mut [F32]) {
    loop i in (0..xs.len()).step(4) {
        a :: F32x4.load(xs, i)
        b :: F32x4.load(ys, i)
        (a + b).store(out, i)         // safe; lowers to std::simd, scalar fallback
    }
}
```

- **Option B — a vectorize hint.** Annotate a scalar loop; the compiler tries to
  vectorize.

```jet
#vectorize
loop i in 0..xs.len() { out[i] = xs[i] + ys[i] }
// no new types, but "best-effort" — Marcus can't tell if it actually vectorized.
```

- **Option C — target intrinsics behind `@unsafe`.** Raw architecture intrinsics.

```jet
@audit("AVX2 packed add; falls back required on non-AVX targets")
@unsafe { simd.x86.mm256_add_ps(a, b) }
// maximum control, expert-only, non-portable, and unsafe — no beginner/portable path.
```

**Recommendation:** A — portable safe lane types (`std::simd` model) keep SIMD
*memory-safe by default* (I1) and portable across targets, which fits Jet's
safe-by-default-with-expert-opt-in spine; raw target intrinsics (C) remain
available behind `@unsafe`/`@audit` for the last-mile expert case. B is a nice
additive sugar but can't be the primitive (it's unpredictable).

---

## Section C — Dedup notes (found already tracked, skipped)

- **`jet.regex`** (persona rec #1, cited by 4 personas) — **c79 / D-REGEX1**,
  ratified 2026-06-21 (opt B, bootstrap the regex crate, I6 exception), now in
  *implementation*. No new card. (`syntax-decisions.md` has D-REGEX1.)
- **External `Struct::method`** (persona rec #6; Saoirse, Dani) — **S83**, open in
  the ballot. Not re-created.
- **Comptime user-defined derives** (persona rec #5; Saoirse, Dani) — **S56**,
  deferred to Epoch 3. Multiple new plans (CSV/JSON/args) explicitly avoid
  depending on it.
- **M12.2 registry / build-from-source / M9 wave-2** — **c50** (build-from-source +
  wave-2, in implementation, soft-blocked on dep approvals), **c56** (signed
  cache, far-horizon), **S52** (manifest, registry "in M12.2"). New **c96** is only
  the `jet publish`/resolver *UX* layer on top, not the infra — flagged as such.
- **TLS for HTTP** (Tariq's "no `jet.tls` example") — **D-NET1** ratified
  (`jet.tls` wraps rustls; `jet.http`→`jet.tls`). The gap is a *missing example*,
  not a feature; fold an HTTPS example into the c83 routing work or c95-style
  example task rather than a new decision.
- **Custom allocator trait** (Marcus) — **D-REF2** (open), already noted on **c05**.
  Not re-created.
- **C bindgen engine** (Yuki's "no `@bindgen` verification") — the *engine* is
  **D-CBIND3** ratified + **c53** shipped. The gap is purely a missing example →
  **c95** (`cbind-example`, NO decision). Not a feature gap.
- **`F32`/`F64` "no precision split"** (Marcus) — the *spelling* is ratified under
  **D-SG9** (full sized menu, `Float=F64`), just **unimplemented** in `Source/`.
  So **c93** is mostly an *implementation* of a ratified decision; only the
  math/precision policy (**D-FLOATW1**) is a genuinely new owner call.
- **Async/await, `select`/non-blocking channel receive** (Kofi, Dani) — persona doc
  and roadmap mark these **deferred to Epoch 3**. No card created; flag for the
  owner if he wants an explicit Epoch-3 card. (Kofi's *blocking* gap is the
  terminal, c87, not async — async is a friction note.)
- **Implicit-clone L0201 noise on `wordfreq.jet`** (Elena) — **c12 / D-L0201**
  ratified (liveness-gated lint) and **done**. The remaining `path`/`dir` warnings
  should re-evaluate against the shipped liveness gate before any new work; not a
  new decision.

### Decision ids drafted (15)

D-ROUTE1, D-DETACH1, D-REPRC1, D-STDIN1, D-TERM1, D-LSDIR1, D-CSVROW1,
D-JSONOUT1, D-ARGS1, D-LOGFMT1, D-FLOATW1, D-MATHLIB1, D-SIMD1
(+ c95 needs **no** decision; c96 raises **D-PUBLISH1** — drafted as a one-liner
below, expand when reached).

**D-PUBLISH1 (stub — expand when M12.2 is reached).** *User story:* Saoirse cuts a
release of her Jet library and Amara pins a semver range to it. *Decision:* the
`jet publish` command shape, version-immutability/semver-enforcement policy, and
the resolver's default (highest-compatible vs exact pins + explicit update,
lockfile default). *Why a stub:* rides c50 (build-from-source) and c56 (registry
upload) infra; promote to a full card with worked `jet publish` shell examples once
that infra is verified. Rec: `jet publish` infers version from `pkg.jet`, refuses
re-publish + dirty tree, resolver defaults to highest-compatible with a committed
lockfile.
