# Capstone — `logbook`: a markdown knowledge-base manager

**Status:** plan, ready to implement. Hand this file to an implementing agent;
follow the [protocol](README.md) and the invariants in `CLAUDE.md`. Every API and
syntax form below was verified against `src/prelude/*.rs` and the
`examples/features/*` sources on 2026-06-18 — see the verified appendix (§9).

**One-line:** index a directory of markdown notes that carry YAML-ish frontmatter
and `[[wikilinks]]` (the same shape as the agent-memory system in `memory/`),
build the link graph, search by tag/type/text, report dead links and malformed
notes, and serve a read-only web view over HTTP. Ships as one CLI with offline
subcommands (golden-tested) plus a live `serve` mode.

It is the single largest example in the repo and exists to **exercise as much of
jet as one coherent program can** while being a tool the owner could actually run
against his own `memory/` and `docs/` trees — so the output is reviewable by
inspection. It needs **no new syntax**; it is built entirely from
Ratified/shipped capabilities. If a step seems to need syntax that isn't shipped,
stop and follow the syntax decision protocol; do not invent.

---

## 1. Why this project (and why this shape)

The `examples/showcase/*` programs are small, single-purpose, size-budgeted
proofs of features in isolation. They don't show how the pieces compose into
something real, and they don't put the dual-facet thesis (beginner-magic
high-level app + expert opt-in tier) on display in one artifact.

`logbook` is the opposite: one realistic application, multi-module, that pulls in
nearly every shipped feature **because the app needs it**, not as a checklist.
It's deliberately a domain the owner knows cold — the note/frontmatter/wikilink
format is the one in `memory/MEMORY.md` — so a reviewer can read the golden output
and immediately judge correctness.

Three things it is designed to make obvious:

1. **"No regex" is not a hole.** Frontmatter, `[[wikilink]]`, and `#tag`
   extraction are hand-written scanners over jet's string API. The capstone shows
   this is clean, not painful, and documents it as the deliberate boundary it is.
2. **The concurrency model fits ordinary work.** Parsing N note files is
   embarrassingly parallel: `logbook` fans the files out across `tasks.spawn`
   workers and gathers results over a channel, then sorts for determinism. Tasks
   and channels earning their keep on a mundane batch job, not just a server.
3. **The dual facet is real.** The whole app is safe, high-level jet. Exactly one
   small module (`hashid.jet`) opts into the expert low-level tier
   (`use core.mem` + `@audit` + `@unsafe`) for a fast content-change key, clearly
   labeled "you don't need this — `crypto.sha256` covers real needs." A reviewer
   sees the seam.

**Theme/name.** `logbook` = a pilot's logbook of entries; on-theme with the jet/
aviation canon and apt for a notes tool. Alternates: `flightlog`, `manifest`,
`chartroom`. Pick at review; the plan uses `logbook`.

---

## 2. Where it lives & how it's run

```
examples/capstone/logbook/
  README.md            overview, architecture, run instructions, BOUNDARIES, feature matrix
  logbook.jet          entry: CLI dispatch + main + comptime version banner
  note.jet             Note struct, NoteType enum, frontmatter+body parse, link/tag scan, ParseError
  index.jet            Index: name->Note map, forward/back links, parallel build, dead-link report
  search.jet           Query enum + query parse, Searchable trait, pure ranking, find
  render.jet           Render trait + markdown-lite -> HTML for note/list/search
  server.jet           http.serve routing: list / note / search / graph.json
  config.jet           load/merge config (defaults <- config.toml <- env vars)
  hashid.jet           EXPERT TIER (opt-in): @unsafe FNV-1a content-change key over bytes
  ffi.jet              one minimal `extern rust "std"` binding (process id, zero-dep)
  config.toml          sample config (fixture + doc)
  fixtures/notes/      ~9 markdown notes incl. deliberate lint issues
  expected/
    index.out          pinned output of `logbook index fixtures/notes`
    lint.out           pinned output of `logbook lint fixtures/notes` (exit 1)
    find_tag.out       pinned output of `logbook find fixtures/notes "#feedback"`
    find_text.out      pinned output of `logbook find fixtures/notes "owner"`
    links.out          pinned output of `logbook links fixtures/notes <name>`
    graph.json         pinned output of `logbook graph fixtures/notes --json`
```

Run ceremony-free (R9): `jet run examples/capstone/logbook/logbook.jet
<subcommand> [args]`. Imports are relative (`use "note";` etc.), so the directory
is the project; relative-file imports resolve against the importing file's
directory (confirmed in `examples/features/21_imports/`).

Because `serve` uses tasks + networking, the interpreter (`jet dev`) can't run it
(documented E2201/E2202 boundary). All golden tests drive `jet run`, which
compiles through rustc.

Subcommands: `index <dir>`, `lint <dir>`, `find <dir> <query>`, `links <dir>
<name>`, `graph <dir> [--json]`, `serve <dir> [--config f]`, `new <dir> <name>`,
`version`.

---

## 3. The application, concretely

All snippets below use **verified** syntax: `when` arms are `| pattern { … }`
(no `->`), payload binding is `| it == Ctor(x) { … }`, enum payloads are
positional, there is **no tuple/multi-subject `when`**, `@test fn name { … }`
takes no parens, map membership is `contains_key`, and module paths are as in §9.

### 3.1 Note model & parsing (`note.jet`) — strings, enums, Result, the "no regex" showcase

A note file is exactly the memory format:

```markdown
---
name: owner-design-kill-criteria
description: declines features that hollow out defaults...
metadata:
  type: feedback
---
Body text with a [[single-source-of-truth-docs]] link and a #design #owner tag.
```

```jet
enum NoteType { User; Feedback; Project; Reference; }

// positional payloads (struct-style enum payloads are not used — they aren't
// matchable with the ratified pattern forms)
enum ParseError {
    NoFrontmatter(String);          // path
    MissingField(String, String);   // path, field
    BadType(String, String);        // path, got
}

struct Note {
    pub name: String;          // frontmatter `name:` (the slug)
    pub description: String;
    pub kind: NoteType;        // frontmatter metadata.type
    pub path: String;
    pub links: [String];       // [[targets]] in body, first-seen order, deduped
    pub tags: [String];        // #tags in body, deduped, sorted
    body: String;
}

pub fn parse(path: String, text: String) -> Note ? ParseError { ... }
```

`parse` is the **string-API showpiece** and the deliberate "no regex" boundary:

- Split the file on the `---` fences (`text.split("---")` / line scan); the middle
  block is frontmatter. `metadata.type` is nested two lines deep, so the flat
  `yaml.parse` won't serve — hand-scan the block line by line, tracking the one
  level of indentation. README documents this as the intended trade-off and notes
  `yaml.parse` would cover a *flat* frontmatter.
- `extract_links(body)` / `extract_tags(body)` are substring scanners using
  `contains`, `split`, `slice`, `starts_with` — no regex. Dedup links preserving
  first-seen order; dedup + `sort()` tags.
- `NoteType` parse: `when type_str { | "user" { return ok(User) } | "feedback"
  { … } | … | else { return err(BadType(path, type_str)) } }` — exhaustive.
- Missing `name`/`description`/`metadata.type` → `err(MissingField(path, …))`;
  no fences → `err(NoFrontmatter(path))`. `?` propagates upward.

**`@test`s** (`jet test note.jet`):

```jet
@test fn parse_minimal {
    val n = parse("a.md", SAMPLE) ?? panic("should parse");
    require_eq(n.name, "owner-design-kill-criteria");
    require(n.links.contains("single-source-of-truth-docs"));
}
@test fn bad_type_errors { ... }      // require it == err(BadType(_, _))
@test fn tags_sorted { ... }
```

### 3.2 Index & graph (`index.jet`) — tasks/channels, maps, parallel build

```jet
use core.fs as fs;
use core.path as path;
use core.tasks as tasks;
use "note";

// plain-data payload so it crosses the channel (sendable: no view/ref/trait)
struct Parsed { path: String; note: Note?; error: String?; }

struct Index {
    notes: [String, Note];           // name -> Note
    backlinks: [String, [String]];   // target -> [names linking to it]
}

pub fn build(dir: String) -> Index ? String {
    var paths = fs.list_dir(dir)?;
    paths = paths.filter((p) => p.ends_with(".md"));
    val ch: Channel<Parsed> = tasks.channel();
    loop p in paths {
        val sender = ch.sender();
        val full = path.join(dir, p);
        tasks.spawn(take(sender) take(full) () => {
            when note.parse(full, fs.read(full) ?? "") {
                | it == ok(n)  { sender.send(Parsed { path: full, note: value(n), error: null }); }
                | it == err(e) { sender.send(Parsed { path: full, note: null, error: value("{e}") }); }
            }
        });
    }
    var got: [Parsed] = [];
    loop i in 0..(paths.len() - 1) { got.push(ch.receive() ?? break); }
    got.sort_by((r) => r.path);     // determinism: stable order before building maps
    // build notes map (report duplicate `name` slugs), then the backlinks map
    ...
}
```

- **Tasks + channels** parse files concurrently; results **sorted by `path`**
  before building the map, so output is deterministic (golden-testable). Teaching
  moment: ownership across the boundary (`take(sender) take(full)`), sendability
  (a plain `Parsed` struct crosses; a `view`/`ref`/trait value would not),
  `receive()` returning `T ? Closed`.
- **Maps everywhere:** forward links live on each `Note`; `backlinks` is the
  reverse map, built by iterating notes and their `links`.
- **Dead-link detection:** a link target with no `contains_key` hit in `notes`.
  `lint` and `index` report these.
- **Duplicate-slug detection:** two files with the same frontmatter `name` →
  reported by `lint` (which exits 1 via `process.exit(1)`).

> Note (verify at build time): if the parser rejects `Channel<Parsed>` as a
> type-annotation form, the fallback is `tasks.spawn(...) -> Task<Parsed>`,
> collecting handles in a list and `join()`-ing them in path order — same
> determinism, also valid. Prefer the channel form to exercise `send`/`receive`.

### 3.3 Search (`search.jet`) — query enum, trait, generics, pure ranking, fan-out

```jet
enum Query { Tag(String); Kind(NoteType); Text(String); }   // positional payloads

trait Searchable { fn matches(self, q: Query) -> Bool; fn score(self, q: Query) -> Int; }
impl Note: Searchable { ... }

pub fn parse_query(raw: String) -> Query {
    if raw.starts_with("#")     { return Tag(raw.slice(1, raw.len() - 1)); }
    if raw.starts_with("type:") { return Kind(...); }
    return Text(raw);
}

pub pure fn rank(hits: [Hit]) -> [Hit] { ... }   // pure, deterministic
```

Matching uses the verified payload-binding form:

```jet
when q {
    | it == Tag(t)  { return self.tags.contains(t); }
    | it == Kind(k) { ... }
    | it == Text(s) { return self.matches_text(s); }
}
```

- **Generic helper:** `fn top<T: Comparable>(xs: [T], n: Int) -> [T]`. `Int`/
  `String` are comparable built-ins, so no `@Comparable` attribute is needed; the
  README notes `@Comparable` (S55) is how a *custom* struct would opt in.
- **Pure functions:** scoring/ranking are `pure fn` (deterministic) — README shows
  `jet eval --pure` on a tiny ranking driver, and notes printing inside a `pure fn`
  is the documented **E3401** boundary.
- **Closures:** `notes.filter(...).map(...)` to build `Hit`s; `sort_by` on score.
- **Fan-out `f.[…]`:** a fixed bundle of canned counts,
  `count_matching.[Tag("feedback"), Tag("owner")]` yielding `[Int#2]`, then
  `val [a, b] = ...;` — shows `[T#N]` + destructuring (`examples/features/41`).

### 3.4 Render (`render.jet`) — trait, markdown-lite, comptime templates

```jet
trait Render { fn to_html(self) -> String; }
impl Note: Render { ... }
```

A scoped **markdown-lite → HTML** converter (headings `#`, paragraphs,
`[[wikilink]]` → `<a href>`, `#tag` → `<span class=tag>`): line-based, no regex,
intentionally minimal (documented scope — not a full markdown engine). Page chrome
(`<html>`, CSS) lives in **`comptime`** String constants baked at compile time
(`comptime` can hold String values — confirmed via `examples/features/29_embed.jet`).

### 3.5 Server (`server.jet`) — http.serve + routing + JSON

`logbook serve <dir>` builds the index, then calls
`http.serve(addr, (req) => route(req, index))`. **There is no tuple `when`**, so
routing is nested / if-chained on `req.method()` then `req.path()`:

```jet
fn route(req: HttpRequest, idx: Index) -> HttpResponse {
    when req.method() {
        | "GET" {
            val p = req.path();
            if p == "/"               { return html_page(list_view(idx)); }
            if p == "/graph.json"     { return json_page(graph_json(idx)); }
            if p == "/health"         { return text_page("ok"); }
            if p.starts_with("/note/")   { return note_page(idx, p.slice(6, p.len() - 1)); }
            if p.starts_with("/search")  { return search_page(idx, p); }
            return not_found();
        }
        | else { return HttpResponse { status: "405 ...", body: "...", headers: [:] }; }
    }
}
```

- `GET /` list, `GET /note/<name>` rendered note + backlinks, `GET /search?q=…`,
  `GET /graph.json` (the link graph built as a `Json` value, `json.render_pretty`),
  `GET /health`.
- **`crypto.sha256`** of each note body → an `ETag` header (and a cheap change key).
- **`jet.log`** structured request logging (`log.set_level`, `log.info`).
- Documented as blocking, thread-per-connection, HTTP-only.

Live-server output is **not** golden-tested (network flakiness — same stance
`tests/ga.rs` takes on the HTTP showcase). The offline `graph --json` path
produces the same JSON and **is** golden-tested.

### 3.6 Config (`config.jet`) — TOML + env + defaults

```jet
struct Config { addr: String; notes_dir: String; level: String; }
```

`load(path: String?)`: compile-time defaults → overlay `toml.parse` (flat
`[String,String]`) of `config.toml` if `fs.exists` → overlay env vars
(`env.get("LOGBOOK_ADDR")` → `Option`, fall back with `??`). Shows `toml.parse`,
`env.get`, map iteration, option-fallback layering.

### 3.7 Expert tier (`hashid.jet`) — the opt-in seam (minimal, skippable)

A fast non-cryptographic content key (FNV-1a) over a note body's raw UTF-8 bytes
(`body.bytes() -> [U8]`), used as an in-memory change-detection key. Implemented
in the expert low-level tier purely to demonstrate it exists and is bounded.
Follow the shape of `examples/features/48_lowlevel.jet`:

```jet
use core.mem;

@audit("FNV-1a over a borrowed [U8]; length from bytes.len(); no writes, no aliasing")
@unsafe fn fnv1a(bytes: [U8]) -> Int { ... }   // calls must be wrapped in @unsafe { } or another @unsafe fn
```

README states plainly: **you do not need this** — `crypto.sha256` and ordinary
code cover real needs; this module shows the tier and its gates: `use core.mem`
discovery gate (→ E3102 without it), missing `@audit` (→ L3101), calling an
`@unsafe fn` outside an `@unsafe` block (→ E3103). Keep it the **smallest honest
demonstration**; if a safe formulation reads just as well, shrink it further or
drop it and note the gap.

### 3.8 FFI (`ffi.jet`) — one honest, minimal demo

A single std binding, zero external crates (I6 — prefer `"std"` over a pinned
crate). Bind a std free function with boundary types and use it in the `serve`
startup log line:

```jet
extern rust "std" {
    fn process_id() -> Int = "std::process::id";
}
```

Shows the FFI boundary (`extern rust "std"`, the `fn sig = "rust::path";` form
from `examples/features/22_ffi.jet`, boundary-type rules, no callbacks/borrows).
If `std::process::id`'s `u32` return doesn't map cleanly to `Int` at the boundary,
fall back to another std free fn returning an allowed type, or drop the module and
note FFI is shown elsewhere.

### 3.9 Entry (`logbook.jet`) — comptime banner, CLI, log

- **`comptime`** version/build banner printed by `logbook version`
  (`examples/features/28_comptime_table.jet`).
- **CLI dispatch:** `io.args()` → `when` over `args[1]` (`| "index" { … }
  | "lint" { … } | … | else { usage(); process.exit(2); }`).
- **`log`** at startup / per request in `serve`.

---

## 4. Feature coverage matrix

Every row is a shipped capability the capstone uses **because the app needs it**.

| Capability | Verified syntax / API | Where |
|---|---|---|
| val/var/const/comptime bindings | `val` `var` `const` `comptime` | all / logbook |
| Primitives, interpolation, multiline str | `String` `Int` `Bool`, `"{x.m()}"`, `"""…"""` | render, note |
| Functions, `pub`, return | `fn` `pub fn` `return` | all |
| if / loop / loop-in / loop-cond | `if`, `loop p in xs`, `loop cond` | note, index |
| `when` matching (single subject, `\| pat { }`, `\| it == Ctor(x)`) | see §3 | note, search, server |
| Structs + methods (self/mut self/take self) | `struct S { f; fn m(self) {} }` | all |
| Enums + positional payloads | `enum E { A; B(String); }`, `E.B(x)`, `err(E.A)` | note, search |
| Traits + external impl | `trait`, `impl Note: Searchable { }` | search, render |
| Generics + Comparable bound | `fn top<T: Comparable>(…)`, `struct Pair<T>` | search |
| Options | `T?`, `value(x)`, `null` | index, config, note |
| Results / `?` / `??` | `T ? E`, `ok` `err`, postfix `?`, `??` | note, index |
| Closures + map/filter/reduce/sort_by/each | `(x) => …`, list methods | index, search |
| Lists, maps, slicing, methods | `[T]` `[K,V]` `[:]`, `.slice`, `.keys/.values/.contains_key` | index, note |
| Fan-out + fixed-size list + destructure | `f.[a,b]`, `[T#N]`, `val [a,b]=…` | search |
| Pattern-test equality + binding | `\| it == ok(n)`, `\| it == err(e)` | index, note, search |
| Ownership across boundary | `take(name)`, sendability | index |
| Tasks + channels | `tasks.spawn` `tasks.channel` `sender.send` `receive` | index |
| Whole-file fs + path helpers | `fs.list_dir` `fs.read` `fs.exists`, `path.join` | index, config |
| Streaming I/O (`new` cmd) | `files.create`/`write_line`/`flush` | logbook (`new`) |
| JSON render | `json.render_pretty`, build a `Json` enum value | server, graph |
| TOML | `toml.parse` (flat `[String,String]`) | config |
| crypto (SHA-256) | `crypto.sha256` (ETag / change key) | server |
| Structured logging | `log.set_level`/`log.info` | server, logbook |
| env / process / io | `env.get`, `process.exit`, `io.args`, `io.eprint` | config, logbook |
| Pure functions | `pure fn`, `jet eval --pure` | search |
| Comptime constants (incl. String) | `comptime VAL = …` (HTML chrome, banner) | render, logbook |
| Expert low-level tier | `use core.mem`, `@audit`, `@unsafe fn`, `[U8]` | hashid |
| Rust FFI | `extern rust "std" { fn = "…" }` | ffi |
| Tests | `@test fn name { require/require_eq }` | note, search |
| Modules / imports / visibility | `use "note"`, `use core.fs as fs`, `pub` | all |
| Formatting | `jet fmt --check` (in CI) | all |

**Deliberately demonstrated boundaries** (in README BOUNDARIES, each tied to a real
spot in the code): regex (→ hand-written scanners in `note.jet`); nested YAML
(→ `yaml.parse` is flat-only, so `metadata.type` is hand-parsed); tuple/multi-subject
`when` (→ nested `when`/if in `server.jet` routing); struct-style enum patterns
(→ positional payloads); async/await (→ tasks); Mutex/lock (→ index built then
shared read-only; mutation would route through a channel actor, per the E2-M1
spec example); TLS/HTTPS (→ HTTP only); thread-per-connection blocking server;
64-bit `Int` only; no string `+` (→ interpolation); markdown-lite is partial.

---

## 5. Tests & golden outputs (the regression armor)

Mirror the showcase pattern (`tests/showcase.rs`): fixed inputs, pinned outputs.
Add `tests/capstone.rs`:

1. **Front-end clean.** Every `.jet` in `examples/capstone/logbook/` parses +
   passes sema. Guard out rustc-dependent runs when `rustc` is absent (copy the
   guard from `tests/showcase.rs`).
2. **Golden runs** (require rustc; guarded):
   - `index fixtures/notes` → `expected/index.out` (counts, dead links, dup slugs).
   - `lint fixtures/notes` → `expected/lint.out`, **exit 1** (fixtures include a
     dead link, a missing-field note, and a duplicate slug).
   - `find fixtures/notes "#feedback"` → `expected/find_tag.out`.
   - `find fixtures/notes "owner"` → `expected/find_text.out`.
   - `links fixtures/notes <name>` → `expected/links.out` (forward + backlinks).
   - `graph fixtures/notes --json` → `expected/graph.json`.
3. **Unit tests.** `jet test note.jet` and `jet test search.jet` pass.
4. **`jet eval --pure`** on a ranking driver prints a deterministic value.
5. **`jet fmt --check`** clean on every file.
6. **No `unsafe` outside the gate.** `tests/golden.rs` already enforces this
   globally; only `hashid.jet` has a gate. I1 must hold.

**Determinism rules for golden paths:** sort all collections before printing
(notes by name, tags sorted, links first-seen order, hits by score then name);
the parallel parse in `index.build` sorts its gathered results. No `time.now()` /
RNG in any golden path — timestamps and ETags appear only in the non-golden
`serve` path.

Fixtures: ~9 small notes under `fixtures/notes/`, mirroring the memory format,
including intentional issues for `lint` (one `[[dead-target]]`, one note missing
`description:`, two notes sharing a `name:`). `expected/*` blessed once via
`UPDATE_EXPECT=1` after the implementer confirms output matches the diagnostics/
format voice, then frozen (I5).

---

## 6. Build sequence (phased; each phase ends green)

Bottom-up so each layer is testable before the next:

1. **Note + parsing.** `note.jet` (types, frontmatter+body parse, link/tag
   scanners, `ParseError`) + `@test`s. Add `fixtures/notes/`. Green: `jet test note.jet`.
2. **Index + graph (single-threaded).** `loop` over files, build maps, dead-link +
   dup-slug detection. Green: `index` + `lint` golden outputs blessed.
3. **Parallelize the build.** Convert `index.build` to the tasks/channel fan-out;
   re-sort for determinism. Green: identical golden output to phase 2.
4. **Search.** `search.jet` (query parse, `Searchable`, pure ranking, fan-out) +
   `@test`s. Green: `find` golden outputs + `jet eval --pure` driver.
5. **CLI + config + comptime + log.** `logbook.jet` dispatch, `config.jet`, banner.
   Green: `links` + `graph --json` golden outputs blessed.
6. **Render + server.** `render.jet` markdown-lite + `server.jet` routing +
   `http.serve`; ETag via sha256. Smoke-run by hand (loopback); not golden.
7. **Expert tier + FFI.** `hashid.jet` `@unsafe` FNV wired into the change key;
   `ffi.jet` single `extern rust "std"` binding. Green: builds; no `unsafe` leaks
   outside the gate (`tests/golden.rs`).
8. **README + BOUNDARIES + matrix + tests.** Write README (architecture, run
   commands, the §4 matrix, honest BOUNDARIES). Add `tests/capstone.rs`. Full
   `cargo test` green; `jet fmt --check` clean.

Each phase is independently committable. Phases 1, 4, 5 are good `sonnet` sub-agent
delegations once shapes are fixed; keep phase 3 (the actor fan-out) and phase 7
(the `@unsafe` tier) under closer review.

---

## 7. Exit criteria (done = all true)

- [ ] All `examples/capstone/logbook/*.jet` parse + pass sema; front-end-clean test green.
- [ ] `jet test` green for `note.jet` and `search.jet`.
- [ ] Golden outputs blessed and frozen for `index`, `lint` (exit 1), `find_tag`,
      `find_text`, `links`, `graph.json`.
- [ ] `jet eval --pure` ranking driver deterministic and green.
- [ ] `serve` smoke-run by hand over loopback; pages render (documented, not golden).
- [ ] `jet fmt --check` clean on every file.
- [ ] No generated `unsafe` outside `hashid.jet`'s `@unsafe` gate (I1; enforced by `tests/golden.rs`).
- [ ] No new crate in `src/` (I6); FFI uses `"std"` only.
- [ ] No new syntax; nothing relies on a non-shipped feature. Any gap is filed via
      the syntax decision protocol, not worked around.
- [ ] README documents architecture, run commands, the feature matrix, and the
      honest BOUNDARIES list, each boundary tied to a real spot in the code.
- [ ] `nix develop -c cargo test` fully green; `tests/capstone.rs` added.

---

## 8. Notes for the implementer

- The §3 sketches use verified syntax (§9) but are illustrative, not final code —
  spell-check each call against §9 / `src/prelude/*.rs` as you write it. Two
  build-time confirmations are flagged inline: the `Channel<Parsed>` type
  annotation (§3.2 fallback to `Task`/`join`) and the `std::process::id` boundary
  mapping (§3.8 fallback to another std fn or drop).
- Keep modules small and the safe/expert seam sharp (CLAUDE.md style). The expert
  tier and FFI modules should each be the **minimum honest demonstration** — if a
  safe formulation reads just as well, shrink them, or note the gap rather than
  contriving a use (I8, simplicity ratchet).
- Run everything through the Nix dev shell, one invocation at a time.
- This is an *example*, not a product: correctness and legibility over features.
  Keep the fixture note format byte-identical to `memory/MEMORY.md` so the tool
  runs unmodified against the owner's real `memory/` and `docs/` trees.

---

## 9. Verified API & syntax appendix (source of truth for the implementer)

Confirmed 2026-06-18 against `src/prelude/*.rs` and `examples/features/*`. Where a
sketch above and this appendix disagree, **this appendix wins**; where it's silent,
the smallest `examples/features/*` using that API is the reference.

### Imports (exact paths)
```
use core.fs as fs;     use core.files as files;   use core.path as path;
use core.tasks as tasks; use core.json as json;   use core.env as env;
use core.process as process; use core.io as io;    use core.net as net;
use core.mem;                                       // low-level tier (no alias needed)
use jet.toml as toml;  use jet.yaml as yaml;       use jet.crypto as crypto;
use jet.log as log;    use jet.time as time;       use jet.http as http;
use "note";  use "search";  use util as text;      // relative-file imports
```

### `when` (verified — `examples/features/07_switch.jet`, `13_errors.jet`)
- Single subject only. **No** `when (a, b)` tuple form — nest `when` or use `if`.
- Arms: `| pattern { … }`, no `->`, no separator needed between arms.
- Value/guard arms: `| 200 { }`, `| "sat" || "sun" { }`, `| (n >= 400) && (n <= 499) { }`.
- Payload binding: `| it == ok(n) { }`, `| it == err(e) { }`, `| it == Ctor(x) { }`
  (`it` is the subject).
- Bare enum value: `| Red { }`. `| else { }` catches the rest.

### Enums / errors (`13_errors.jet`)
- Declare positional: `enum E { A; B(String); C(String, String); }`.
- Construct: `E.A`, `E.B(x)`; results: `ok(v)`, `err(E.A)`; `??` fallback: `expr ?? 0`,
  `expr ?? return`, `expr ?? panic("…")`. Postfix `?` propagates `err`.

### Generics / traits (`26_generic_types.jet`, `25_traits.jet`)
- `fn largest<T: Comparable>(xs: [T]) -> T? { … }`; `struct Pair<T> { first: T; second: T; }`;
  construct `Pair<T> { first: a, second: b }`.
- `trait Shape { fn area(self) -> Float; }` + external `impl Square: Shape { … }`.
- `Comparable` is a real bound; a *custom* struct opts in with the `@Comparable`
  attribute (S55). `Int`/`String` are comparable built-ins.

### Closures / lists / maps (prelude)
- Closures: `(n) => n * n`, `(n: Int) => (n * n)`, multi-line `(x) => { … }`.
- List: `push pop get len contains first last index_of filter map reduce(init, f)
  sort sort_by(keyf) each find any all join join(sep) reverse insert remove clear`.
  `sort/sort_by/push/...` mutate (return `Unit`).
- Map `[K,V]`: `insert get(k)->Option contains_key remove(k)->Option len keys values
  each((k,v)=>…)`. Empty map literal `[:]`; entry literal `["k": v]`.
- String: `contains split(sep) starts_with ends_with slice(a,b) len to_upper to_lower
  bytes()->[U8] chars()->[Char] trim repeat replace`. No `get_char` (use `.chars()[i]`).
  Slicing is `.slice(a, b)` (inclusive); `s[i..j]` indexing also exists per spec.
- Fan-out: `double.[1,2,3]` → `[Int#3]`; destructure `val [a,b,c] = doubled;`.

### Tasks / channels (prelude)
- `tasks.spawn(() => …) -> Task<T>`; `Task.join() -> T`.
- `val ch: Channel<T> = tasks.channel();` `ch.sender() -> Sender<T>`;
  `sender.send(v) -> Unit`; `ch.receive() -> T ? Closed` (use `??`/`when`).
- Captures: lambda must own captures crossing the boundary — `take(name)`.

### I/O & data (prelude)
- `fs.read(p) -> String ? IoError`, `fs.list_dir(p) -> [String] ? IoError`,
  `fs.exists(p) -> Bool`; `path.join(a,b) -> String`, `path.extension`, `path.parent`.
- `files.create(p) -> FileWriter ? IoError`; `FileWriter.write_line(s) ? IoError`,
  `.flush() ? IoError`; `files.open` + `FileReader.read_line() -> String? ? IoError`.
- `json.parse(s) -> Json ? JsonError`, `json.render`, `json.render_pretty`.
  `Json` variants: `Json.Null` `Json.Boolean(Bool)` `Json.Number(Float)`
  `Json.Text(String)` `Json.Array([Json])` `Json.Object([String, Json])`.
- `toml.parse(s) -> [String,String] ? String` (flat); `yaml.parse` same (flat).
- `crypto.sha256(s) -> String`, `crypto.sha256_bytes([U8]) -> String`.
- `log.set_level(s)`, `log.info(s)` / `debug/warn/error`.
- `env.get(name) -> String?`; `process.exit(code) -> !`; `io.args() -> [String]`;
  `io.eprint(s)`.
- HTTP: `http.serve(addr, (HttpRequest) -> HttpResponse) -> !`;
  `http.get(url) -> HttpResponse ? String`, `http.post(url, body) -> … ? String`.
  `HttpRequest`/`HttpResponse` fields `method/path/body/headers` + method accessors
  `req.method()`, `resp.body()`, `req.header(name) -> String?`. Build a response
  with `HttpResponse { status: "200 OK", body: …, headers: [:] }`.

### Low-level / FFI / comptime / tests
- Low-level (`48_lowlevel.jet`): `use core.mem;` then `@audit("…")` above an
  `@unsafe { … }` block or `@unsafe fn`; `mem.Ptr<T>.from_addr(addr)`,
  `mem.volatile_read(p)`, `mem.address_of(x)`. `body.bytes() -> [U8]`.
- FFI (`22_ffi.jet`): `extern rust "crate@ver" { fn name(a: T) -> R = "rust::path"; }`;
  `extern rust "std" { … }` needs no version. No callbacks/borrows across the boundary.
- comptime (`28_comptime_table.jet`, `29_embed.jet`): `comptime NAME = expr;`,
  values may be String.
- tests (S43, ratified form — examples still show the old `test "…" {}`):
  `@test fn name { require(cond); require(cond, "msg"); require_eq(a, b); }`.
