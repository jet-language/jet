# Decision ballots — open queue (owner input needed)

This is the **only** list of decisions still waiting on you. Everything you've
already ratified has been incorporated into the spec and is gone from here:

- **Syntax** lives in `docs/spec/syntax-decisions.md` (Ratified section + decision log).
- **Milestone/strategy gates** live in `docs/plans/epoch-2/` (README checklist + per-milestone owner-decision rows).

Last routed from `decision-ballots-owner.md` on 2026-06-16. Each item below ends
in a single concrete ask. The headline is **§0 — the attributes shape**; several
others are waiting on it.

Glossary: *attribute* = a marker on a declaration that changes how the compiler
treats it ("serialize this", "this is a test"). *scoped effect* = a behavior that
applies to a **region** of code, not a whole declaration ("roll back changes
inside *this block*").

---

## 0 — The attributes shape (ATTR-SHAPE + D-LL2 + D-JSON1) ⭐ read first

You said three things that are really one decision:

- **ATTR-SHAPE:** *"`#[attribute(s)]` so we can support a list of attributes…
  then a block for scoped effects."*
- **D-LL2:** *"`#[...]` rust style for a list or a single attribute; allow
  scoping with blocks, i.e. `async { … async code … }`. Before locking in show
  me what you think I mean."*
- **D-JSON1:** *"treat serialize as an attribute, like transact — a `#[Serialize]`
  block right before `struct Profile {…}`; defined automatically with one word
  but overridable."*

Here is exactly what I think you mean. **Two shapes, one for each job:**

### Shape 1 — `#[…]` attribute on a declaration (annotations / markers)

A `#[…]` prefix on a `fn`, `struct`, `enum`, or `test`. Holds **one or a comma
list** of markers:

```jet
#[Serialize]                       // single marker
struct Profile { name: String; score: Int; }

#[Comparable, Serialize]           // a list
struct Score { value: Int; }

#[pure]                            // would replace today's `pure fn` prefix
fn area(r: Float) -> Float { 3.14 * r * r }

#[test]                            // would replace today's `test "name" { }` shape
fn reversing_twice { … }
```

When a marker needs **configuration**, it takes a block — your "automatic with
one word, overridable" requirement (the glaze partial-override model, spelled as
an attribute):

```jet
#[Serialize {
    rename score -> "user_score";   // only this key changes
    skip internal_id;               // never written/read
}]
struct Profile {
    name: String;
    score: Int;
    internal_id: String;
}
```

### Shape 2 — keyword + block for a scoped effect (a region of code)

When the marker scopes a **behavior to part of a function**, it's a block, not a
declaration attribute:

```jet
fn step(target: Tile) -> Unit ? {
    transact {                       // roll back the mutations below if any `?` fails
        player.spend_stamina(10)?;
        player.step(target)?;
    }
}

unsafe { *ptr = 0; }                 // already ratified (S58) — same shape

async { await fetch(url); }          // Epoch 3 — same shape, reserved for now
```

The rule in one sentence: **`#[…]` annotates a declaration; a `keyword { }` block
scopes an effect.** `transact` can appear as *either* — `#[transact] fn` (whole
function) or `transact { }` (part of one) — exactly like `unsafe` today.

### ⚠ What this reshapes (why I stopped for you)

Adopting `#[…]` **reopens already-ratified spellings.** Decide the *scope* of the
change, not just the look:

| Today (ratified) | Under the `#[…]` model |
|---|---|
| `derive Serialize;` (body line, S55) | `#[Serialize]` (prefix) — **D-JSON1 asks for this** |
| `test "name" { }` (S43) | `#[test] fn name { }` — Rust-style |
| `pure fn f()` (S60) | `#[pure] fn f()` |
| `comptime x = …` (S57) | `#[comptime] val x = …` |
| `unsafe fn` / `unsafe { }` (S58) | block stays; `unsafe fn` → `#[unsafe] fn`? |

Two coherent ways to draw the line:

- **Option α (recommended) — `#[…]` for annotations; keep keywords for
  control-flow-ish effects.** `#[…]` becomes the home for *derive-like markers*:
  `#[Serialize]`, `#[Comparable]`, `#[derive(…)]`, `#[test]`, `#[todo]`. Blocks
  stay for scoped effects: `unsafe { }`, `transact { }`, `async { }`. `pure` and
  `comptime` **stay as prefix keywords** (they read as part of the type/binding,
  not bolt-on metadata). Smallest disruption; reopens **S43 + S55** only.
- **Option β — full unification.** *Every* marker is `#[…]`: `#[pure]`,
  `#[comptime]`, `#[unsafe]`, `#[derive(…)]`, `#[test]`, `#[transact]`, plus the
  block forms for scoping. One look for every marker — but reopens **S43, S55,
  S57, S58, S60** and reads more like Rust, less like your clean prefix-keyword
  style.

A note on `#`: you now have three uses — `[T#N]` (list length), `pkg#version`
(version pin), and `#[…]` (attribute). The first two are **infix** `#`; `#[`
(hash-immediately-bracket, at the start of a declaration) is a distinct token, so
the lexer tells them apart cleanly. Just a conscious nod that `#` does three jobs.

**Ask:** (1) **α or β** — how far does `#[…]` reach? (2) confirm the two-shape
rule. Once you pick, I'll amend S43/S55 (+ S57/S58/S60 if β), add the `#[…]`
grammar, and unblock transactions (spelling), D-LL2 (`unsafe` audit attribute),
D-JSON1 (`#[Serialize]`), and `#[todo]` in one stroke.

## 1 — D-ERR2: name the concrete error carrier

You chose **`Error` is the capability (a trait)**; the default concrete carrier it
boxes needs its own name (because `-> T ?` already *returns* "some `Error`").
You'd write `impl FileError: Error { … }`, and `-> T ?` means "any `Error`."
Naming menu for the concrete default carrier (message + optional code + optional
source, S80):

| Candidate | Reads as | Notes |
|---|---|---|
| **`Fault`** | `Fault.message("…")` | short, neutral, unoverloaded; "an `Error` is anything that reports a `Fault`" |
| **`Failure`** | `Failure.code(404)` | clear, plain; slightly longer |
| **`Mishap`** | `Mishap.with_source(e)` | friendly/beginner-voiced; a touch informal |
| **`Snag`** | `Snag.message("…")` | warm, memorable, very beginner-first; informal |
| **`Trouble`** | `Trouble.code(n)` | plain-English, approachable |
| **`Defect`** | `Defect.message("…")` | precise but clinical |

My lean: **`Fault`** (neutral, short) or **`Snag`** for the warmer beginner voice.
**Ask:** pick a name, or say "menu again."

## 2 — D-DEV2: Cranelift, and your JIT runtime-type-server idea

**What Cranelift is, plainly.** A small, fast machine-code generator in Rust.
Our normal pipeline hands generated code to rustc/LLVM (slow, maximally optimized
— for shipping binaries). Cranelift compiles to native code in *milliseconds* with
light optimization — what you'd use to "compile and run *now*", repeatedly, while
a program is alive. That's a JIT.

**Your goal — "a JIT runtime type server to replace TypeScript/JavaScript with
high-performance safe apps"** — is bigger than the D-DEV2 ballot (which only asked
"should `jet dev` JIT the hot loop while you edit?"). Splitting it:

- **D-DEV2 as asked (small):** the interpreter is already fast enough for
  save-to-diagnostic feedback (<200 ms, D-DEV3). Recommendation: **design note,
  build nothing in Epoch 2.**
- **Your vision (big):** a long-lived Jet runtime that JIT-compiles and hot-swaps
  *typed* code — the safe/fast JS/TS-runtime alternative. A genuine **Epoch 3
  pillar**, not an Epoch 2 milestone:

  ```
  $ jet serve app.jet           # long-lived process, JIT-compiled
  # edit a handler, save → the running server swaps in the new typed code,
  # no restart, no lost connections — like nodemon, but the swapped code is
  # type-checked and memory-safe before it goes live.
  ```

**Ask:** open `docs/plans/post-epoch-2/jit-runtime-type-server.md` capturing this
as an Epoch 3 pillar (Cranelift JIT + hot-swap + TS/JS-replacement framing), while
keeping D-DEV2-the-ballot at "design note only" for Epoch 2?

## 3 — D-DX5: external subcommands, a real example

Lets *other people* add `jet` subcommands without us building a plugin framework
— the trick `git` uses to find `git-lfs`. A community member publishes an
executable named `jet-bench`; a user installs it; then:

```
$ jet bench app.jet              # `jet` sees no built-in "bench", finds `jet-bench`
                                 #   on PATH, and runs it with the rest of the args
running app.jet … 1.2 ms/op
```

We wrote no "bench" command and maintain no plugin API. Option C would instead be
a formal plugin system (registration, stable ABI, versioning) — far more to build.

**Persona:** *Dana, perf-curious*, finds `jet-flamegraph` on the index, installs
it, types `jet flamegraph app.jet` — feels first-class though we shipped nothing.

**Ask:** confirm **A** (PATH discovery, no plugin API). My recommendation.

## 4 — D-FP2: expression-body functions vs lambdas (clarified)

```jet
fn double(x: Int) -> Int = x * 2;     // (1) expression-body FUNCTION — the proposal
val double = (x) => x * 2;            // (2) a LAMBDA stored in a val — you have this (S46)
```

Same computation. The difference is *what they are*: a **function** is a named,
importable, generic-capable, doc-carrying thing; the proposal (1) just lets a
one-liner skip `{ return …; }`. A **lambda** (2) is a value you make inline to
pass somewhere (`xs.map((x) => x*2)`). The only thing (1) buys is dropping
`{ return …; }` on tiny named functions — and you already have lambdas + `fn`.

**Persona:** *Sam, math library author.* With the sugar:
`fn square(x: Float) -> Float = x * x;` reads like a textbook definition. Without:
`{ return x * x; }` — a little ceremony per tiny function.

**Ask:** **A** add the `= expr;` form · **B** require `{ return …; }` always ·
**C** defer until real code shows lots of one-line `fn`s. Lean **C**.

## 5 — D-PAT5: multiple function bodies by pattern (comparison + personas)

```jet
// Option B — multi-head:
fn area(Circle(r)) = 3.14 * r * r;
fn area(Rect(w, h)) = w * h;

// Option A (recommended) — one body, `when` inside:
fn area(s: Shape) -> Float {
    when s {
        s == Circle(r) -> { 3.14 * r * r };
        s == Rect(w, h) -> { w * h };
    }
}
```

| | Option A (one `when`) | Option B (multi-head) |
|---|---|---|
| Logic lives | one place | scattered across N definitions |
| Reads like | "examine `s`, branch" | math-paper case analysis |
| "Where do I look?" | obvious | gather all heads |
| Two ways to branch? | no | yes (heads *and* `when`) |

**Personas:** *Maya, beginner* — under A she reads one function; under B she must
realize there are two `area`s and merge them mentally. *Oli, porting a Haskell
parser* — loves B; `eval(Lit(n)) = n; eval(Add(a,b)) = eval(a)+eval(b);` is how he
thinks.

The tension: our "one obvious way" rule vs. elegance for math/recursion code.
**Ask:** **A** (decline B — my rec) or **B** (you love the look enough to accept
two branching forms)?

## 6 — D-PURE1 & D-PURE2 (separated)

"Pure eval" = running Jet at build time that is **guaranteed side-effect-free**
(no clock, network, random files). Same inputs → same output. That guarantee is
what makes a build **reproducible and cacheable** (the Nix property you like).

**D-PURE1 — what do we build on top of it?**

```jet
comptime val TABLE = build_sine_table(360);   // runs at build time, baked in, cached
```

- **A (rec)** — pure eval **plus sandboxed package recipes** (build instructions
  in the pure subset; this wires into the Jetpack hangar store).
- **B** — pure eval only. · **C** — go all the way to JetOS now.

**D-PURE2 — how strict is the sandbox during eval?**

```jet
comptime val X = read_file("/etc/passwd");   // ← reject?
comptime val Y = http_get("https://…");      // ← reject?
```

- **A (rec)** — **no ambient I/O or network at all**; the only door is the
  explicit `embed_file("path")` builtin (S26). Pure or it doesn't compile.
- **B** — an allowlist. · **C** — trust the recipe author.

The entire value is the guarantee; B/C leak it. **Ask:** confirm **D-PURE1 = A**
and **D-PURE2 = A**, or say where to loosen.

## 7 — E2-V12: redundant (confirm retirement)

E2-V12 bundled three unrelated things: **pure eval** (§6, near-term), **layer 3**
(user `derive`/reflection, post-1.0), **JetOS** (long-horizon). Its only real
question — "how far down this road in Epoch 2?" — is answered by your other
choices: pure eval + recipes ship in Epoch 2 (D-PURE), JetOS stays research,
layer 3 stays post-1.0. **Ask:** OK to **retire E2-V12 as redundant** once you
confirm §6?

## 8 — D-TOOL4: snapshot testing, a real example

Captures a function's output **once** to a file; later runs flag any difference;
you "bless" intentional changes with one keystroke. (It's how this compiler tests
its own error messages.)

```jet
test "weekly report renders" {
    snapshot(render_report(sample_data));
}
```

- First run: writes `tests/snap/weekly_report_renders.snap`, passes.
- Unchanged later: matches → passes silently.
- Changed later: shows a **diff** and fails →
  ```
  - Total: $1,200
  + Total: $1,350
  snapshot changed. intended? run `jet test --bless` to accept.
  ```
- `jet test --bless` once → file updates, passes again.

You never hand-write the expected 40-line string; any drift is caught. Great for
formatters, CLIs, generated code, error messages.

**Ask:** **A** (build into `jet test` with one-key blessing — my rec) or **B**
(defer)?

## 9 — D-CFFI2: finding C libraries (answering your hangar question)

You asked: *"What if a user doesn't have the lib already? Shouldn't it be pulled
into the jet hangar?"* — right instinct. The layered answer:

```jet
extern c "raylib" { fn init_window(w: Int, h: Int, title: String) = "InitWindow"; }
```

1. **No Jetpack (bare `extern c` in a single file).** Fall back to the
   C/C++/Rust convention: `pkg-config raylib` for the flags (A). If raylib isn't
   installed, a clear error: *"C library 'raylib' not found. Install it, or add it
   to your `pack.jet` so Jetpack fetches it."* Never silently fail.
2. **With Jetpack (the good path you're pointing at).** The dep is declared in
   `pack.jet`, and **Jetpack pulls raylib into the hangar** (content-hashed,
   reproducible) and hands the compiler the exact include/link paths — no system
   install, same build on every machine:

   ```jet
   // pack.jet
   deps: { raylib: nixpkgs:raylib }   // Jetpack realizes it into the hangar
   ```

`pkg-config` *locates* a lib once it exists; Jetpack *guarantees it exists*
reproducibly. They compose — the single-file-stays-simple / packages-make-it-
reproducible split (E2-V4). **Ask:** confirm this layered answer (I'll record
D-CFFI2 = A with the hangar-provider path as the documented project default).

## 10 — D-NET2: how a server handles many requests

- **A (rec)** — **thread-per-task + channels.** Each request gets its own worker;
  they coordinate by **passing messages**, never sharing memory (ownership
  forbids the dangerous sharing). Simple; scales like early Go. Great for internal
  services and small public APIs; not aimed at 100k simultaneous connections.
- **B** — a small **async** exception (more scalable, far more complex; `async`
  is Epoch 3, E2-V5). · **C** — a fixed thread pool.

```jet
server.on_request((req) => {             // each call on its own worker
    val user = db.lookup(req.user_id)?;  // no shared mutable state across workers
    respond(req, render(user))
});
```

Matches our concurrency model (S53) and is plenty for Epoch 2's targets (E2-V7=B).
Honest tradeoff: A tops out lower than async, and we're fine with that until
Epoch 3. **Ask:** confirm **A**.

## 11 — D-REF3: inlay hints beyond clone

*Inlay hints* are the grey annotations your editor draws **into** the code (not
real text) to make invisible behavior visible. You have the clone hint; the
question is two more by default:

```jet
fn first_name(u: User) -> String {
    u.name              // inlay: «borrowed return» — value is lent, not moved
}                       // inlay: «cleanup: file, conn» — what RAII frees here
```

- **A (rec)** — **borrowed-return + cleanup-scope** hints on by default (plus the
  clone hint). Teaches ownership gently by showing the wiring (your
  "Blueprint-level friendliness" goal). Dismissible, so low cost.
- **B** — clone hint only. · **C** — all off by default.

**Ask:** confirm **A**, or pick B/C.

---

## Tally

| # | Item | Ask |
|---|---|---|
| **0** | **Attributes shape** | **α or β** + confirm two-shape rule (unblocks transact/unsafe-audit/JSON/todo) |
| 1 | D-ERR2 carrier name | pick a name (lean `Fault`) |
| 2 | D-DEV2 / JIT vision | open the Epoch-3 design doc? |
| 3 | D-DX5 | confirm A |
| 4 | D-FP2 | A / B / C (lean C) |
| 5 | D-PAT5 | A decline / B accept (lean A) |
| 6 | D-PURE1 + D-PURE2 | confirm A + A |
| 7 | E2-V12 | OK to retire as redundant |
| 8 | D-TOOL4 | A / B (lean A) |
| 9 | D-CFFI2 | confirm layered pkg-config + hangar answer |
| 10 | D-NET2 | confirm A |
| 11 | D-REF3 | confirm A |
