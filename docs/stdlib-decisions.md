# Standard library & module system — decision document

**Status:** SL1-SL10 ratified 2026-06-13 by owner direction in planning
discussion. Accepted decisions still need to be folded into docs/02
(syntax), docs/03 (architecture), docs/06 (ballot log), the M10 plan, and
the M12 package plan where applicable. Written 2026-06-12.

**What this answers:**
1. What do developers actually love and hate about other languages'
   standard libraries — and what does that mean for Jet?
2. Is our module/import system (S16/S51, already ratified) on track?
3. Can we have a big, powerful std library *without* bloating every binary
   with dead code? (Short answer: yes — section 5 explains how in plain
   terms.)
4. Ten concrete decisions (SL1–SL10) with worked examples per option and a
   recommendation for each.
5. A proposed layout for the "ring" — the libraries Jet needs for industry
   adoption (files, io, networking, json, etc.) — and the order to build it.

---

## 1. Glossary (read this first)

- **Standard library ("std")** — the code that ships *with the language*:
  `import std.fs`, `std.json`, etc. No download, no version picking; it's
  just there.
- **Prelude** — the things available with *zero* imports. In Jet today:
  `print`, `List`, `Map`, `String`, `Option`, `Result`. In Python: `len`,
  `str`, `open`. The prelude is the language's first impression.
- **The ring** — this doc's name for the layer *around* the core std:
  first-party libraries (http, regex, csv, dates) that ship with std-level
  quality but are versioned and installed like packages. Go calls theirs
  `golang.org/x`. Section 7 proposes Jet's. Their canonical package names
  live under `jet.*` (`jet.std`, `jet.http`, `jet.regex`), but first-party
  short import names are reserved so users can write `import http as http`
  instead of `import jet.http as http`. Ring/package versions may be
  requested at the import site (`import http#0.8.1 as http` or
  `import jet.http#0.8.1 as http`); the package manager records the
  resolved canonical package, version, and hash in the manifest and
  lockfile.
- **Frozen API** — a promise that published functions never change or
  disappear. Go froze its std at 1.0 ("the Go 1 promise"); 16 years of code
  still compiles. The cost: mistakes are permanent.
- **Toolchain-selected std** — Jet's core `std` is part of the selected Jet
  toolchain, not a normal package. Its canonical package identity is
  `jet.std`, but `import std as std` and `import std.fs as fs` are reserved
  short spellings. `import jet.std as std` is also valid when code wants the
  canonical spelling. A package gets one `std` version: the one bundled with
  its locked Jet compiler. Old code stays buildable by pinning the old
  toolchain, not by importing two `std` versions in one file.
- **Dead code elimination (DCE)** — the compiler/linker throwing away
  functions your program never calls, so they cost zero bytes in the binary.
  JavaScript bundlers call the same idea **tree-shaking**.
- **LTO (link-time optimization)** — a final whole-program optimization pass
  that, among other things, makes DCE much more thorough. Jet already turns
  this on (`jet build` uses thin LTO; `--small` uses fat LTO).
- **Qualified call** — `fs.read(path)`: you can see *which module* `read`
  came from at the call site.
- **Selective import** — `from os import path` (Python) or
  `use std::fs::read` (Rust): pulling one name into your file so you can
  call it bare. The opposite of qualified.
- **Monomorphization** — how generics compile: the compiler stamps out one
  copy of `max[T]` per concrete `T` actually used. Unused combinations cost
  nothing.
- **Batteries included** — Python's slogan: the std covers everything.
  The failure mode has a famous nickname: *"the stdlib is where modules go
  to die"* (modules can never be removed or redesigned, so the bad ones rot
  in place forever).

---

## 2. Where Jet stands today

Already ratified (not up for re-decision here, listed so the ballots make
sense against them):

- **S16** — two import forms, quotes distinguish them:
  `import "./lib";` (file path) vs `import scoring;` (logical module).
  Optional `as alias`. Access is always `namespace.item`.
- **S51** — std is a module: `import std.fs as fs;`. Never quoted.
- **S18** — private by default; `pub` exports across files.
- **S54** — no enforced naming convention in v1.
- **M10 plan** — frozen v1 inventory: `fs, io, env, process, math, random,
  time, json`, every fallible call returns `Result`, all backed by a
  generated Rust prelude (no Jet-source std yet).
- **Roadmap commitments** — streaming I/O, networking, error conversion for
  `?`, all post-v1.

**Verdict up front:** the import system is genuinely on track — S16/S51
independently landed on the design the community evidence favors (Go-style
qualified access, no Rust-style `use` trees, no Python-style `from` dumping).
Section 4 has the evidence and the three calls still open. The std plan is
also sound for v1; the open question is *what happens after M10*, and that's
what most ballots below are about. Deciding the shape now (core vs ring,
error taxonomy, versioning promise) is cheap; retrofitting it after v1.0
freezes the API is what every other language got wrong.

---

## 3. World tour — what communities love and hate

Each entry ends with the lesson Jet should take. These are long-standing,
well-documented community positions, not one survey.

### Go — the std everyone points to

**Loved:** `net/http` is a production web server and client in the box —
companies run real services on it with zero dependencies. `testing`, `fmt`,
`encoding/json`, `flag` mean a useful tool needs *no* third-party code.
One coherent voice: everything takes the same patterns, returns errors the
same way. The **`golang.org/x` ring** keeps experiments out of std until
they're proven (and lets failures die quietly without breaking the
compatibility promise).

**Hated:** the time-format API — you write `"2006-01-02"` as a magic
reference date instead of `"YYYY-MM-DD"`; it's the single most-mocked API in
the language. Pre-generics cruft is frozen in forever by the Go 1 promise
(`math.Max` only worked on floats for a decade). `encoding/json` silently
ignores unknown fields and has surprising struct-tag behavior. The lesson
inside the lesson: *Go's mistakes are all "we froze v1 too casually."*

**Lesson for Jet:** batteries win adoption. A frozen core plus an `x`-style
ring is the proven structure. But every API frozen at v1.0 must survive ten
years — which argues for a *small* frozen core.

### Python — batteries included, batteries leaking

**Loved:** `json`, `sqlite3`, `pathlib`, `itertools`, `collections`,
`http.server` for a quick file server — "I can do real work with zero
installs" is why Python owns education and scripting. The string methods are
the gold standard for a beginner-friendly API surface.

**Hated:** the std contains its own museum: `urllib` is so unpleasant that
the *first thing every tutorial says* is "install `requests`" — the standard
HTTP library lost to a package, permanently. `datetime` has naive/aware
timezone traps everyone falls into. Duplicates pile up (`os.path` vs
`pathlib`, `optparse` vs `argparse`, three string-formatting systems).
PEP 594 had to formally remove ~20 dead modules in 3.13, and it was painful
and controversial even though the modules were objectively dead.

**Lesson for Jet:** "in the std" is where code goes to die *unless* there's
a deliberate removal/evolution policy. Never ship an API into the frozen
core that you'd tell users to avoid. If the community's first advice is
"don't use the standard X," the std has failed at X.

### Rust — quality bar high, walls too close

**Loved:** `Option`/`Result`/iterators are the best-designed core vocabulary
in any mainstream language (Jet already borrowed them, correctly).
Documentation quality. The API design bar: nothing lands without years of
review.

**Hated:** the std is *too* small. Random numbers, regex, serialization,
async runtimes, even error-handling helpers live in third-party crates —
so every new user must discover `rand`, `serde`, `regex`, `tokio`,
`anyhow` by tribal knowledge, and every project carries 200+ transitive
dependencies with supply-chain risk to match. "Which crate do I use for X"
is the perennial newbie thread.

**Lesson for Jet:** a minimal std outsources your beginner experience to a
package index. That's directly counter to our priority #2. Jet should be
*more* batteries-included than Rust, with the Rust-grade design bar.

### JavaScript / Node — the control group with no std

**The cautionary tale:** with no standard library, the ecosystem produced
`left-pad` (11 lines; its removal broke the internet in 2016), `is-odd`
(400k downloads/week), and routine 1000-dependency apps — the worst
supply-chain surface in software. Then two module systems (CommonJS vs ESM)
split the ecosystem for a decade; "cannot use import statement outside a
module" became the most-seen error in the language. `Date` was so bad it's
being replaced wholesale (Temporal) after 25 years.

**Lesson for Jet:** no std is not an option, and *one* module system,
decided early, is a survival trait. Jet has exactly one (S16). Never add a
second form.

### Java — the API graveyard that finally learned

**Loved:** `java.time` (the 2014 redesign) is widely considered the best
date/time API anywhere — built by the author of the library (Joda-Time)
that beat the std's first two attempts. Collections breadth.

**Hated:** those first two attempts (`Date`, `Calendar`) are still there,
deprecated-but-present, confusing every beginner 30 years later. Old and new
APIs for files, HTTP, etc. coexist forever. Verbosity
(`new BufferedReader(new InputStreamReader(System.in))`) became the
language's public image.

**Lesson for Jet:** v1 mistakes never leave. Date/time APIs in particular
should not be designed casually — ship millis-only (as M10 does) until a
real design exists; copy `java.time`/Temporal's concepts when we do it.

### C++ / PHP / C# — three one-liners

- **C++:** `std::regex` is famously slower than launching PHP to run the
  same regex; the committee can't fix it because the ABI is frozen.
  *Lesson: a frozen bad implementation is worse than absence.*
- **PHP:** `strpos($haystack, $needle)` but `array_search($needle,
  $haystack)` — inconsistent argument order became the language's
  defining joke. *Lesson: consistency across modules is a feature users
  feel daily; one style authority must review every std signature.*
- **C# /.NET:** quietly the best large std — one vendor, one naming
  convention, LINQ everywhere, batteries from day one. *Lesson: a single
  design authority (here: the owner) producing one voice is a real moat.
  It's also evidence that "big and coherent" is achievable.*

### Zig — the newest data point

**Loved:** lazy compilation — *anything you don't reference costs nothing*,
std included, so a huge std coexists with tiny binaries. Explicit
allocators. **Lesson for Jet:** this is the technical proof for section 5 —
"big std, small binary" is a solved problem when the compiler only emits
what's used.

---

## 4. Module & import systems — are we on track?

What the evidence says works and fails:

| Pattern | Where | Community verdict |
|---|---|---|
| Qualified-by-default (`fs.read`) | Go | Loved. Any reader knows where every name came from. Single most-praised readability feature of Go. |
| Selective import (`from x import y`, `use a::b::c`) | Python, Rust | Convenient, but the top source of "where did this name come from?" in code review, name collisions, and circular-import pain. Rust's `mod` vs `use` distinction is its #1 newbie module complaint. |
| Two module systems at once | JS (CJS/ESM) | Ecosystem-splitting disaster, ten years and counting. |
| Filesystem = module tree, no manifest needed | Go, Python | Liked: nothing to configure. |
| Import cycles banned outright | Go | Initially annoying, ultimately loved — architecture stays a DAG. |
| Wildcard import (`import x.*`, `use x::*`) | Java, Rust | Universally discouraged by every style guide that allows it. |

**Verdict: on track.** S16/S51 is the Go model with one improvement Go
doesn't have — quotes visually distinguish "this is a file on disk" from
"this is a logical module," which kills an entire class of beginner
confusion (Python's `sys.path` mysteries, Node's `./` ambiguity). Qualified
`namespace.item` access matches the most-loved pattern in the table.
`E0606` listing ambiguous matches is exactly right.

Three calls keep us honest (ballots below):
- **SL3** — do we ever add selective imports? Decision: no; qualified-only
  is now a recorded decision, not an accident someone relitigates in a PR.
- **Import cycles** — S16 doesn't state a policy. Recommend ratifying Go's
  rule: cycles between files/modules are an error with a diagnostic that
  prints the cycle (`a → b → a`) and suggests extracting shared code into a
  third file. Cheap now, impossible later.
- **SL2** — what the ring's import spelling is, so `import std.fs` and
  ring imports feel like one system, not two (JS's fate).

---

## 5. The binary-bloat question, answered plainly

**Question:** can Jet have a huge standard library without every hello-world
binary carrying it as dead weight?

**Answer: yes — this is a solved problem, and Jet's architecture is already
the best-case setup for it.** Three layers, outermost first:

1. **Jet's prelude is generated per-program.** Std functions aren't a
   library we link against — codegen *writes* the Rust helpers into the
   generated program (M10 plan). So the first and strongest tool is simply:
   **only emit the helpers the program actually calls.** A program that
   never imports `std.json` gets zero lines of JSON parser in its generated
   Rust. This is Zig's lazy-compilation property, and we get it almost for
   free because sema already knows every call site. No other mainstream
   language gets to do DCE *before the optimizer even sees the code*.
2. **rustc/LLVM dead-code elimination + LTO.** Already on (`-O`,
   `strip=symbols`, thin LTO default, fat LTO + `opt-level=z` +
   `panic=abort` under `--small`, per S15). Whatever layer 1 emits but a
   branch never reaches, the optimizer removes.
3. **Monomorphization.** Generic std functions (`max[T]`, `List<T>`
   methods) only exist in the binary for the `T`s the program actually
   uses.

**The honest floor** (already recorded in docs/00): Rust's std runtime sets
a low-hundreds-of-KB baseline (allocator, panic machinery, formatting). We
accepted that floor deliberately rather than going `no_std`. Every std
module Jet adds on top of that floor costs a binary **only what that
program uses** — adding `std.json` to the *language* costs programs that
don't use it **zero bytes**.

**What this means for ambition:** binary size is *not* a reason to keep the
std small. The std can grow as large as design quality allows. The real
constraints are the ones in section 3: API freeze risk and one-voice
consistency. Ballot SL9 turns the size property into a CI-enforced
guarantee so it never silently regresses.

---

## 6. The ballots

Each: the question, the options with a worked example you can squint at,
and a recommendation. Hypothetical syntax inside options is **illustration
only** — any accepted spelling still goes through the normal syntax
protocol before code is written.

---

### SL1 — Size philosophy: how big does std itself get? *(ratified 2026-06-13: Option C)*

The single most consequential call. Everything else hangs off it.

**Option A — Minimal core (Rust model).** Std stays roughly the M10 list
forever; everything else is packages.

```
$ jet add http        # day one of any real project
$ jet add regex
$ jet add dates
```
*Day-to-day:* every tutorial starts with installs; "which package for X?"
threads; supply-chain exposure for basics. This is the most-complained-about
property of Rust. **Conflicts with priority #2.**

**Option B — Everything in std (Python model).** http, regex, csv, dates,
crypto all under `import std.*`, frozen at v1.0.

```jet
import std.http as http;            // nothing to install, ever
val page = http.get("https://example.com")?;
```
*Day-to-day:* wonderful in year one. In year ten: Jet's `std.http` is its
`urllib` — the version we froze before we knew better, with the good one
living in a package and every tutorial opening with "don't use std.http."
Python needed a formal PEP and years of fights to delete its dead batteries.

**Option C — Small frozen core + first-party ring (Go model). (Ratified)**
Core std = M10's list (+ later: streaming io, path, net once *proven*).
Frozen, compatibility-promised, curated by the owner. Around it, the
**ring**: first-party, owner-curated libraries that ship with releases but
are *versioned*, so they can evolve (or be retired) without breaking the
core promise. Graduation path: ring module proves itself for N releases →
owner may promote it into frozen std (how Go's `x/context` and `x/slices`
became `context` and `slices`).

```jet
import std.fs as fs;     // core: short spelling for jet.std/fs
import jet.std as std;   // core: explicit canonical package spelling
import http as http;     // ring: short spelling for jet.http
import http#0.8.1 as h;  // ring: exact version request (spelling = SL2)
```
*Day-to-day:* beginners see no difference — both are "just there." Experts
get evolvable APIs. Mistakes die in the ring instead of fossilizing in std.

**Decision: C.** It's the only option whose failure mode (a ring
module gets redesigned) is survivable. A's failure mode is Rust's
dependency sprawl; B's is Python's museum. C also matches our existing
instincts — M10's "out of scope" list (networking, regex, TOML/CSV) is
already implicitly a ring waiting for a name.

---

### SL2 — The ring's import spelling, versions, and delivery *(ratified 2026-06-13: Option A)*

Only live if SL1 = C. How does ring code arrive and what does the import
look like?

**Option A — Canonical `jet.*` packages + reserved short imports +
import-site versions. (Ratified)**

```jet
import std as std;                // short spelling for jet.std
import jet.std as std;            // equivalent explicit spelling; choose one
import std.fs as fs;              // submodule of toolchain-selected jet.std
import http as http;              // short spelling for jet.http
import jet.http as http;          // equivalent explicit spelling; choose one
import http#0.8.1 as http08;      // exact first-party ring version
import jet.http#0.8.1 as http08;  // same request, canonical spelling
import acme.auth#2.4.0 as auth;   // exact third-party package version
```
```
$ jet run tool.jet       # just works; nothing was installed
```
`jet.*` is the canonical first-party package namespace. Source files may use
reserved short spellings for first-party packages: `std` means `jet.std`,
`http` means `jet.http`, `regex` means `jet.regex`, and so on. Explicit
canonical imports remain valid, including `import jet.std as std`; the short
form is convenience, not the only spelling.

Unversioned `import http as http` or `import jet.http as http` means
"resolve the latest version compatible with this Jet toolchain." Versioned
imports request an exact package/ring version. In both cases the package
manager writes the resolved canonical package name, version, and content
hash into the generated manifest/lockfile, so future builds are Nix-style
reproducible.

Multiple versions of an ordinary package may coexist only when explicitly
aliased:

```jet
import http#0.8.1 as old_http;
import jet.http#1.4.0 as new_http;
```

Core `std` is the exception: canonical package `jet.std` is selected once
per package by the locked Jet toolchain. Do not support `import std#1.0 as
old_std` or `import jet.std#1.0 as old_std` beside `import std#2.1 as
new_std`; that splits foundational type identity (`Result`, `Option`,
`String`, `Path`, I/O handles, errors) inside one compiled package. A
workspace may still contain separate packages on different Jet majors, each
with its own lockfile/toolchain.

**Option B — Ring as auto-resolvable packages: `import jet.http;` works
only after `jet add jet/http`.**

```
$ jet run tool.jet
error[E1101]: `jet.http` is not installed
  fix: run `jet add jet/http`
```
*Tradeoff:* cleaner versioning story, but reintroduces install ceremony for
batteries — the exact thing batteries exist to avoid — and breaks the
single-file story for ring users.

**Option C — No namespace separation: ring modules also live under `std.`,
marked unstable in docs only.** *Tradeoff:* users cannot tell frozen from
evolving at the import site; this is how Python's `asyncio` churn burned
people — the import said "standard," the API said "experimental."

**Decision: A.** The import site stays ergonomic (`import http as
http`) while the resolver and lockfile keep a precise canonical identity
(`jet.http`). The stability tier is still explicit after resolution:
`jet.std` is the toolchain-selected frozen core; `jet.http` is curated but
versioned. Reserve the first-party short names (`std`, `http`, `regex`,
`csv`, `toml`, `crypto`, `archive`, etc.) and the canonical `jet`
namespace, then reject ambiguous local/package imports with a diagnostic.
Version syntax (`#0.8.1`) is proposed because it is visually attached to
the imported package, not to the alias or call site.

---

### SL3 — Selective imports: do we ever allow them? *(ratified 2026-06-13: Option A)*

**Option A — Qualified-only, forever (status quo S16). (Ratified)**

```jet
import std.math as math;

fn main() {
    print(math.clamp(value, 0, 100));   // reader sees where clamp lives
}
```

**Option B — Add selective form** (illustration: `import std.math { clamp,
pi };`) so hot names can be called bare:

```jet
import std.math { clamp, pi };

fn main() {
    print(clamp(value, 0, 100));        // shorter…
}
```
*Tradeoff:* every reader of line 4 must now scroll up to learn where
`clamp` came from; two files importing different `clamp`s read identically;
collisions need rules; code review gets harder. Python/Rust experience says
teams end up writing style guides *against* their own feature.

**Decision: A — and record it as rejected, not merely absent.**
Aliasing (`as m`) already handles long names. Go proved qualified-only at
ecosystem scale, and it's the option that protects the beginner reading
code, not just the expert writing it. One carve-out worth pre-deciding:
enum variants already have a sanctioned short form in `switch` (S30 dot
shorthand), so nobody needs `import` games for that.

---

### SL4 — Prelude scope: what works with zero imports? *(ratified 2026-06-13: Option A + method/function review rule)*

The prelude is the first five minutes of Jet. Today: `print`, core types,
their methods. M10 adds nothing to it (math/io/etc. all need imports).

**Option A — Frozen tiny prelude (status quo). (Ratified)** A beginner's
second program hits `import std.io as io;` the moment they want input —
one line of ceremony, and it teaches the import system honestly.

```jet
import std.io as io;

fn main() {
    val name = io.input("name? ") or return;
    print("hi {name}");
}
```

**Option B — Pull common math/io into the prelude** (`input`, `abs`, `min`,
`max` bare).

```jet
fn main() {
    val name = input("name? ") or return;   // zero imports
    print("hi {name}");
}
```
*Tradeoff:* saves one line for beginners, but: bare names squat the global
namespace forever (user-defined `fn input` now collides or shadows —
needs a rule either way), and "which names are blessed?" becomes a
permanent bikeshed. Python's 71 builtins are a known wart.

**Option C — Methods over modules where natural.** Not prelude *functions*
but *methods*: `(-5).abs()`, `xs.max()` instead of `math.abs(-5)`,
`math.max(...)`. This is how M5 strings/lists already work
(`text.to_upper()`, not `strings.to_upper(text)`).

**Decision: A for the prelude, plus a standing method/function review
rule.** Libraries may choose dot methods or module functions based on the
operation. When an operation has one obvious receiver, a method is usually
the most discoverable spelling (`text.to_upper()`, `xs.length()`). Modules
fit operations with no single receiver (`fs.read`, `json.parse`,
`random.int`), side effects, domain services, or two equal arguments
(`math.min(a, b)`). M10 core std should avoid duplicate method+function
pairs unless a specific API has a strong reason; this is not a permanent
ban on future libraries offering both forms if Jet later designs that as an
explicit capability.

---

### SL5 — One voice: the std API style sheet *(ratified 2026-06-13: Option A)*

PHP's needle/haystack is what happens without one. Proposal: before M10
implementation starts, ratify a one-page style sheet every std/ring
signature must pass. Draft rules (each from a section-3 lesson):

1. Argument order: subject first, then what's done to it —
   `fs.write(path, text)`, `map.insert(key, value)`. Never varies.
2. Names are full words: `read`, not `rd`; `length`, not `len`… *except*
   the prelude's already-ratified short forms stay (consistency with M5
   beats abstract purity).
3. Every fallible call returns `Result` (already M10 law). No panicking
   variants and no `_unchecked` twins in core (expert tier S58 owns that).
4. No abbreviated module names in std: `random`, not `rand` (S51 already
   spells these out; keep it that way in the ring).
5. Verbs read as actions, nouns as values: `fs.read(path)` returns data;
   `fs.exists(path)` returns Bool and asks a question.
6. One way per task: a new function must not duplicate an existing one
   "but slightly nicer" — it replaces it (ring) or doesn't land (core).

**Decision: A — adopt for now.** This is the M10 core std review sheet, not
a forever lock on every future library shape. In particular, rule 6 means
M10 should choose one canonical spelling per task by default; a later
library/ring decision may allow deliberate method+function pairs if the
capability is designed explicitly instead of drifting in by accident. C is
how PHP happened — one designer having taste is not the same as a written
rule, because agents and future contributors don't share the taste.

---

### SL6 — Std error taxonomy (decide before networking, not after) *(ratified 2026-06-13: Option B)*

M10: each module gets one small error enum (`IoError`, `JsonError`). The
roadmap already knows v1's "`?` only works within one error type" rule
dies in multi-module programs. The shape question is *how* errors compose,
and it gates the entire ring (http errors wrap io errors wrap...).

```jet
fn load_config(path: String) -> Config or ??? {
    val text = fs.read(path)?;      // IoError
    val data = json.parse(text)?;   // JsonError — what type unifies them?
    return ok(parse_config(data));
}
```

**Option A — One giant `std.Error` enum every std call returns.**
*Day-to-day:* `?` always works… and every `switch` on an error must
consider `JsonError` cases on a file-only function. Go's pre-1.13 flat
`error` interface had this "everything is anything" flavor and everyone
string-matched. Loses type precision exactly where Jet's exhaustive
`switch` shines.

**Option B — Per-domain enums + declared conversion (a `From`-equivalent),
beginner spelling TBD. (Ratified)** Each function keeps its precise
error; a user-side error enum declares "an IoError becomes
`Config.ReadFailed`," and `?` applies the conversion automatically.

```jet
enum ConfigError {
    ReadFailed(from: IoError);     // illustration only — spelling needs
    BadSyntax(from: JsonError);    // its own ballot before implementation
}

fn load_config(path: String) -> Config or ConfigError {
    val text = fs.read(path)?;     // ? wraps IoError into ReadFailed
    val data = json.parse(text)?;  // ? wraps JsonError into BadSyntax
    ...
}
```
This is Rust's most-loved error pattern (`thiserror`) with the boilerplate
designed out, and it keeps switch-exhaustiveness meaningful.

**Option C — Errors carry context strings, no typed composition (Go 1.13
`fmt.Errorf("%w")` style).** Simple, but checking "was it NotFound?"
becomes string archaeology — the thing Go users complain about most in
their otherwise-loved std.

**Decision: B**, ratified as a *direction* now (the roadmap item 3
already points here) so every std/ring error enum is designed to compose,
with the surface spelling balloted separately before implementation.

---

### SL7 — Paths: strings forever, or a Path type later? *(ratified 2026-06-13: Option A)*

M10 v1: paths are `String` (ratified, stays). The ballot is the *post-v1
direction*, because ring modules (http, archives, watchers) will take paths
in their signatures and we shouldn't churn them later.

**Option A — Strings forever + a `std.path` module of pure string helpers
(Go's `filepath` model). (Ratified)**

```jet
import std.path as path;

val full = path.join(dir, "notes.txt");   // handles / vs \ for you
val stem = path.stem("report.pdf");       // "report"
if path.extension(file) == "json" { ... }
```
*Day-to-day:* zero new concepts; covers ~95% of real path work; Go shipped
an entire ecosystem on it. Weakness: non-UTF-8 filenames (rare, real) can't
round-trip perfectly — Go accepted that cost for fifteen years of
simplicity; Rust's "correct" `PathBuf`/`OsString` answer is its
most-cursed-at everyday type.

**Option B — A dedicated `Path` type post-v1 (Python `pathlib` model).**
```jet
val p = Path("data") / "notes.txt";       // illustration only
```
Nicer once learned, but it's a whole new type with operator behavior,
conversion rules, and "String or Path?" signature questions across every
std module — high churn for a marginal gain over A.

**Decision: A.** Matches priorities (beginner experience over edge
correctness), and is forward-compatible: if real demand for B appears, a
`Path` type can land in the ring without breaking anything that took
strings.

---

### SL8 — Date & time: how long do we hold the line? *(ratified 2026-06-13: Option A)*

The single most-regretted API category across *every* language (Java
needed three tries; JS needed 25 years and Temporal; Python's datetime
still traps everyone). M10 ships `time.now() -> Int` (unix millis),
`sleep`, `Stopwatch` — deliberately tiny.

**Option A — Hold: millis-only through v1.x; a real calendar/timezone
module is designed *once, whole* in the ring, copying Temporal/java.time
concepts (instant vs civil time as different types, explicit timezone at
every conversion). (Ratified)**

```jet
// v1.x: honest and limited
val started = time.now();
...
print("took {time.now() - started} ms");
```
The day someone needs "what date is it in Tokyo," they use the ring module
*when it exists*; until then Jet honestly doesn't do calendars — better
than doing them wrong, which is the universal regret.

**Option B — Add "just a little" date support to std now (format
`now()` as `"2026-06-12"`, parse ISO dates).** Every language that added
"just a little" date support grew a haunted house around it (naive
datetimes, local-time assumptions). This is the top of the slippery slope.

**Decision: A.** M10 stays millis-only, and Jet should build an excellent
calendar/timezone library post-v1 as a whole design rather than accreting
"just a little" date support. Also ratify the design constraints for the
eventual ring module now (instant/civil split, no implicit local timezone,
no format mini-language without a ballot) so a future agent can't casually
re-create `Date`.

---

### SL9 — Make "pay for what you call" a tested guarantee *(ratified 2026-06-13: Option A)*

Section 5 says the architecture gives us tiny binaries. This ballot makes
it a *promise* instead of an accident.

**Option A — Ratify as an architecture rule + CI test. (Ratified)**
- Rule (new R10, docs/03): *codegen emits a std helper into the generated
  program only if sema proves the program can call it. Importing a module
  is free; only calls cost bytes.*
- CI: golden size tests — `01_hello.jet` built `--small` stays under a
  pinned byte budget; a fixture importing all of std but calling nothing
  must produce a binary within noise of hello-world. A regression fails CI
  like any snapshot.

```
$ jet build --small examples/01_hello.jet
$ ls -l build/01_hello
-rwxr-xr-x  1 nate nate  312K  build/01_hello     # pinned in CI ± noise
```

**Option B — Best-effort, no test.** It'll be true on day one and silently
rot the first time someone emits the whole prelude unconditionally for
convenience.

**Decision: A.** Cheap, permanent, and it's a marketable property —
"a Jet binary contains your program, not our library" is a sentence
experts choosing between Go/Rust/Zig actually care about.

---

### SL10 — JSON's two modes (confirm direction) *(ratified 2026-06-13: Option A)*

M10 ships dynamic JSON (`Json` enum you walk by hand). S55 already ratified
`derive Serialize;`. The confirm-it ballot: Jet's end state is **both**
modes, like the combination users praise (Go's struct-tag decoding, Rust's
serde) without their warts:

```jet
// Mode 1 (M10): exploring unknown JSON — walk the enum
val data = json.parse(text)?;

// Mode 2 (post-M10, via S55 derive): known shape — typed, no ceremony
struct Config { name: String; port: Int; }
derive Serialize;

val cfg: Config = json.load(text)?;       // illustration; spelling TBD
print(cfg.port);
```

With one Go-lesson fix worth pre-ratifying: **unknown fields are an error
by default** (Go's silent-ignore is its most common production bug),
with an explicit opt-out for tolerant parsing.

**Decision: A — confirm both modes + strict-by-default.** M10 ships dynamic
JSON. Typed JSON lands later via the derive direction, and unknown fields
are errors by default with an explicit tolerant-parsing opt-out. B leaves
Jet without the single most-used serialization workflow in industry code.

---

## 7. The ring — proposed inventory and build order

Assuming SL1=C and SL2=A. Core std stays the M10 eight (+ `path` per SL7,
+ streaming `io` per roadmap). The ring's canonical package names are
`jet.*`, but source files may use the reserved short names (`import http as
http`, `import regex as regex`). In adoption-priority order, each item maps
to a "can't use Jet at work without it" scenario:

| Order | Module | Unlocks | Notes |
|---|---|---|---|
| 1 | `jet.http` (client) | calling APIs — the #1 thing every modern tool does | blocking, on streaming I/O; design after SL6 lands |
| 2 | `jet.regex` | grep-class tools, validation | hand-rolled or vetted approach needs I6 owner call; C++'s std::regex is the anti-model |
| 3 | `jet.csv` + `jet.toml` | data files, configs | JSON (std) proves the pattern; these copy it |
| 4 | `jet.http` (server) | small services — Go's killer demo | needs tasks (M11/v2); client must not wait for server |
| 5 | `jet.time` (calendar) | dates/timezones done once, whole | SL8 constraints |
| 6 | `jet.crypto` (hash/random/hmac) | checksums, tokens | vetted primitives only; never hand-rolled |
| 7 | `jet.archive` (zip/tar/gzip) | release tooling, data pipelines | |
| 8 | `jet.db` (sqlite) | Python's most-loved battery | FFI-tier under the hood (M7 machinery) |

Everything below this line is community-package territory (websockets,
yaml, image codecs, ORMs…) — the ring's curation value comes from staying
small enough that one owner can actually review every signature against
SL5.

---

## 8. Ratified course corrections (the "right the ship" list)

The ship is largely on course; these are trims, not turns:

1. **Name the ring and reserve its namespace** (SL1/SL2) before M10 ships,
   so `std` never absorbs modules it can't evolve. Update E1002 to reserve
   the canonical `jet` namespace and first-party short import names such as
   `std`, `http`, `regex`, `csv`, `toml`, `crypto`, and `archive`.
2. **Record "no selective imports" as a rejected decision** in S16's
   rejected list (SL3) + ratify an import-cycle policy with its own
   diagnostic.
3. **Add import-site package versioning to the M12 package-manager plan**
   (SL2): first-party short names are reserved aliases for canonical
   `jet.*` packages, `import pkg#version as alias` requests a package/ring
   version, unversioned imports resolve to the latest compatible version,
   the generated manifest/lockfile pins canonical package identities and
   exact hashes, `import jet.std as std` is valid, and `std#version` /
   `jet.std#version` are not valid inside one package.
4. **Add the std style sheet** (SL5) to docs and the M10 plan checklist
   before implementation starts — cheapest possible moment.
5. **Re-sequence error conversion (SL6) ahead of any ring work** — it
   currently sits as roadmap item 3 with no plan file; it gates http.
6. **Add R10 + size-regression CI** (SL9) during M10, when the prelude
   becomes per-module — the natural moment to make emission usage-gated.
7. **Write the SL8 date/time constraints down** so no future agent ships
   "just a little" date formatting.

Nothing in M10's frozen v1 API conflicts with any recommendation above —
the plan can proceed as written once the ballots are decided.
