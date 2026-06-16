# Decision ballots (owner's queue) — routed 2026-06-16

This file has been **routed against the owner's responses**
(`decision-ballots-owner.md`, 2026-06-16). Every ID now carries one of three
statuses:

- **✅ Decided** — the owner picked an option. Recorded here with any caveat.
  Genuine *syntax* decisions are staged for `docs/spec/syntax-decisions.md`;
  milestone/strategy gates are staged for the relevant plan under
  `docs/plans/`. See *Part 0 — ratification queue* for exactly what moves where.
- **❓ Needs your input** — the owner wrote "I don't know what this means",
  "need more info", "explain", etc. These are rewritten below in plain language
  with a concrete example and a recommendation, so the call can be made from
  real use, not jargon. **This is the part to read.**
- **💬 Needs discussion** — decided in spirit but the owner wants to talk it
  through or compare community practice first.

> **How to use this document.** Read **Part 1 (needs your input)** first — those
> are the questions blocking nothing-can-proceed-without-them. Part 2 is a
> reference list of what you already decided. Part 3 collects three cross-cutting
> threads your answers opened up (the "attributes" list, the `#` version syntax,
> and two things that turned out to be already done).

Glossary: *epoch* = a large human-facing era (Epoch 1 = language core, Epoch 2 =
production platform). *edition* = a per-project compatibility marker that lets
old code keep compiling when syntax changes (Rust-style). *tier-2* = post-v1
reference features (`view`/`ref`) for experts. *ring* = the first-party
`jet.*` package set that ships beside the compiler. *RAII* = "cleanup happens
automatically when a value goes out of scope" (you never write a `close`).

---

# Part 0 — Ratification queue (what moves where on your "go")

These are **✅ Decided** and ready to propagate. Nothing here is moved into the
canonical files. The syntax decisions are now **officially ratified** into
`docs/spec/syntax-decisions.md` (2026-06-16); the milestone/strategy gates are
recorded into the matching `docs/plans/epoch-2/` plan files.

**✅ Ratified into `docs/spec/syntax-decisions.md` (2026-06-16):**

| Ballot | Ratified as | Decision |
|---|---|---|
| D-FP1 | **S77** | struct field punning — `Source { name, upstream }` when a local has the field's name |
| D-FP4 | **S78** | empty-list inference from expected type; explicit `[]: [T]` still allowed (your caveat) |
| D-FP5 | **S79** | expressions in `for … in <expr>` heads (field access / calls / indexing / ranges) |
| D-PAT1 | **S31 amended** | nested patterns in payload slots — `r == ok(Rect(w, h))` |
| D-PAT3 | **S74 amended** | a refutable bind (`val value(n) = opt;`) requires `?? fallback` |
| D-ERR1 | **S80** | `Error` carrier grows to message + optional code + optional source |
| D-ERR3 | **S80** | `fn main() -> Unit ?` allowed; prints the `Error`, exits non-zero |
| D-ERR4 | **S81** | `?continue` loop-skip added (you chose B over the "defer" rec) |
| D-SUGAR2 | **S69 note** | pipe `\|>` declined for now (newline dot-chains cover it) |
| D-SUGAR4 | **logged** | newtype keyword declined for now (one-field struct covers it) |

> **⚠ D-ERR2 NOT ratified — needs one tiny confirm.** You said "A but rename
> the trait to `Error`." Problem: `Error` is already the *type* you return
> (`-> T ?` == `T ? Error`), so a *trait* also named `Error` collides. I
> recorded the conversion *mechanism* in S80 but left the **name** open — see
> the 30-second decision in **Part 1L**.

> **Already covered — no new entry needed:** D-SUGAR1 (digit separators) is
> already ratified as **S67**. D-SUGAR7 (keep semicolons required) is already
> **S6**. Confirmed, not re-ratified.

**✅ Recorded into the `docs/plans/epoch-2/` plan files (2026-06-16):**
*(Each gate is marked ratified in its milestone plan's owner-decisions section.
Items you left open — e.g. D-DX5, D-NET1/2, D-PURE1/2, D-LIB1/2 — were left as
"needs owner" in the plans and appear in Part 1 here.)*

| Ballot(s) | Plan | Decision summary |
|---|---|---|
| E2-V1, V3, V4, V6, V9 | epoch-2/README | audience = beginners **and** small teams; beat Python/Node/Go/Rust/Zig (all, non-negotiable); single-file `jet run` sacred, packages optional; full low-level **but safe-by-default, expert opt-in**; ship VS Code/Cursor + Zed + Neovim |
| E2-D1, E2-D2, D-REL1, D-REL2, D-GA4 | epoch-2/m2 | **normal SemVer forever**; no encoded "epoch" version, ever; you control version bumps manually |
| D-REL3, D-REL4, D-REL5 | epoch-2/m2 | `edition` field (A); no LTS until GA (C); only owner-approved `jet fix` may migrate (A) |
| D-DX1, D-DX2(+auto-fix), D-DX3, D-DX4, D-DX6 | epoch-2/m3 | stable `--json`; `jet doctor` health **and** auto-fix; Zed dev extension; ship completions+man pages; OSC-8 hyperlinks |
| D-DEV1(+flag), D-DEV3 | epoch-2/m4 | interpret common programs, add an opt-in "try anyway" flag with no guarantees; <200 ms save-to-diagnostic budget |
| D-REF1 | epoch-2/m5 | teach references after the beginner ownership chapter |
| D-IO1(+ergonomics) | epoch-2/m7 | `std.path` helper module — but invest in ergonomics |
| D-PKGS1, D-PKGS2, D-PKGS3 | epoch-2/m8 | git registry now (hosted later); reserved `jet.*` namespace; optional signed metadata v1 |
| D-LR1, D-LR2, D-LR3, D-LR4 | epoch-2/m9 | ship **all** ring libs in Epoch 2; sqlite via C FFI now / pure-Jet later; crypto as broad as safely possible (vetted impls only); **add `jet.yaml` in wave 1** (you chose B) |
| D-NET3 | epoch-2/m10 | sqlite-first service showcase |
| D-TEST3(B-first), D-TEST4 | epoch-2/m11 | docs-guided learning first, `jet tour` later; doctests run under `jet test` |
| D-OBS2 | epoch-2/m12 | panic shows safe locals in dev mode only |
| D-LL1, D-LL3(+wider API) | epoch-2/m13 | amend I1 for user-gated `unsafe`; narrow `std.mem` core **plus** an opt-in wider expert API (name TBD) |
| D-CFFI1(+export later), D-CFFI3 | epoch-2/m14 | import-only C FFI first, export-to-other-languages post-Epoch-2; **ship a raylib showcase** |
| D-CROSS1 | epoch-2/m15 | first cross target = one CLI target (e.g. `aarch64-linux`) |
| D-PURE3 | epoch-2/m16 | **ship the signed cache in M16** (you chose B over "design now, ship later") |
| D-GA1, D-GA2, D-GA3 | epoch-2/m17 | **all 6 showcases mandatory** (B); **hard CI perf/size gates** (B); **no beta** before GA |
| D-BUILD1, D-BUILD2 | epoch-2/m3 | `jet doctor` reports FFI/cargo health; `jet build -v` prints the bridge steps |
| D-FP3 | epoch-2/m6 | core `module name { … }` typed declaration |
| D-OWN1, D-OWN2, D-OWN3 | epoch-2/m6 | keep + strengthen the implicit-clone lint (see your perf question, answered in Part 1); add ownership mini-examples; suggest `take` at the call site |
| D-JSON2 | epoch-2/m6 | JSON decode ignores unknown keys by default, opt-in strict |
| D-FS2 | epoch-2/m7 | ship the game-loop example **and** the `poll_input` helper (A & B) |
| D-TOOL1, D-TOOL3 | epoch-2/m3+m11 | doctests under `jet test`; ship gated `jet emit --rust` expert window |
| D-PAT4 | post-v1 | list rest-spread `[h, ...t]` deferred (you asked where it's used — answered in Part 1) |
| D-REPL* (the decided ones) | epoch-2/m18 | see Part 2 table |

---

# Part 1 — Needs your input (read this)

Each item: **what it actually means** in plain language, a **concrete example**,
and **what I'd pick / what I need from you**. Grouped by theme.

## 1A — REPL (the interactive `jet` prompt)

A REPL is the prompt you get when you type `jet repl` — you type one line of Jet,
it runs immediately and prints the result, and you keep going. Like the Python
`>>>` prompt or a browser console. It's for quick experiments without making a
file. Several of your "I don't know what this means" answers are about *how* that
prompt should behave.

**D-REPL3 — How do you start it?** You asked "why not B?"
- A (my rec): you type `jet repl` to start it.
- B: you just type `jet` with no file, and if you're in a terminal it drops you
  into the REPL.
- **Why I lean A:** with B, a beginner who fat-fingers `jet` instead of
  `jet run x.jet` lands in a mystery prompt with no idea how to leave. `jet repl`
  is explicit and discoverable (`jet --help` lists it). B saves three keystrokes
  at the cost of a confusing accident. If you'd rather optimize for "feels like
  magic, zero ceremony", B is defensible — your call.

**D-REPL4 — What runs your code in the REPL?** ("more info, should feel like
magic")
- A: a small **interpreter** runs your lines directly. Starts instantly, feels
  live, but is a separate engine from the real compiler.
- B: each line is **compiled to a real binary** and run. Slower per line (compile
  + link each time), but guarantees identical behavior to `jet run`.
- C: **hybrid** — interpret for speed, fall back to compile for the hard cases.
- **For "magic / low friction", A wins** — instant feedback is the magic. The
  risk is the interpreter and the compiler disagreeing on a corner case; we
  already plan a test battery (the M4 interpreter) to keep them in lockstep, so
  A is safe. Recommend A.

**D-REPL5 — What are you allowed to type?** ("explain tradeoffs")
- A: statements + control flow (`val x = 1;`, `if`, `for`, function calls).
- B: also full declarations (define a `struct`/`trait`/`fn` mid-session).
- C: expressions only (`2 + 2`), no bindings.
- **Tradeoff:** C is too weak to prototype with. B is the most powerful but
  raises "what happens when you redefine a struct you already used?" edge cases.
  A is the sweet spot for rapid prototyping and is what most REPLs start with;
  we can grow into B. Recommend A, with B as a later upgrade.

**D-REPL6 — Does the REPL see your project?** ("no jet.toml anymore, explain")
Correct — there is no `jet.toml`; the manifest is now `pack.jet` (already done,
see Part 3). The real question: when you open a REPL inside a project folder,
should it automatically load that project's code and dependencies, or start
clean?
- A (rec): start in a **clean sandbox**; opt in with `jet repl --project` to load
  the current `pack.jet`.
- B: always auto-load the project.
- **Why A:** a clean prompt always starts the same way and never fails because
  the project doesn't build. `--project` is there the moment you want it.
  Recommend A.

**D-REPL7 — How does the session remember things?** ("don't know what this
means") When you define `val x = 5;` on line 1, line 2 should still see `x`.
- A: **accumulating module** — every line adds to one growing invisible file;
  later lines see everything earlier. (How Python's `>>>` works.)
- B: **cells** — independent blocks you can re-run out of order (like a Jupyter
  notebook).
- C (rec): accumulating by default, with an optional `:cell` command when you
  want a fresh isolated block.
- **Recommend C** — accumulating is the intuitive default; cells are a power
  feature for the rare case. This is purely REPL-internal; no language syntax
  rides on it.

**D-REPL14 — Snippets the REPL can't run live.** ("which is more magic?") Some
code can't be interpreted (FFI, low-level, tasks). Two options when you type one:
- A: **reject it** with a message like "the REPL can't run FFI; put it in a file
  and `jet run`."
- B: **silently compile-and-run** just that snippet behind the scenes so it
  appears to "just work."
- **B feels more magic** (everything you type runs), but it's slow and
  occasionally surprising (a one-liner pauses for a compile). A is honest and
  fast. My lean is A for predictability, but if "magic" is the priority here,
  B — tell me which value wins for you.

**D-REPL20 — How do we test the REPL?** ("don't know what this means") Just an
internal engineering choice, no user-facing effect:
- A (rec): record **transcripts** (type these lines, expect this output) as test
  fixtures.
- B: also drive a real pseudo-terminal for arrow-keys/history testing.
- **Recommend A**; it's the standard, cheap way. Not a decision you need to own
  unless you care — safe to defer to engineering.

## 1B — Pattern matching & functions

**D-PAT2 — Guards (extra condition on a match arm).** ("don't know what this
means") A *guard* is an extra "…but only if" test on a `when` arm. Note: we
already renamed `switch` → `when` (S24 — Part 3).
- A (rec): use `&&` and let the bound name flow into the test:
  `when r { r == ok(Code(n)) && n >= 500 -> { … }; }` — match an error code
  *and only if* it's ≥ 500.
- B: add a dedicated `when`-style guard keyword (but we just *took* `when` for
  the whole construct, so this would need a different word).
- **Recommend A** — it reuses `&&`, which you already know, and needs no new
  keyword. The example: "match a 5xx HTTP error" reads naturally.

**D-PAT5 — Multiple function bodies by pattern.** ("more discussion") Some
languages (Elixir, Haskell) let you write the same function several times, once
per input shape:
```
fn area(Circle(r)) = 3.14 * r * r;
fn area(Rect(w, h)) = w * h;
```
- A (rec): **decline** — write one `fn area` with a `when` inside. One obvious
  way.
- B: allow the multi-head form above.
- **Recommend A.** B is elegant for math-heavy code but gives two ways to do the
  same branching, and scatters one function across many definitions (harder for
  beginners to follow). Worth discussing if you love the look; my vote is A.

**D-PAT6 — Destructuring in parameters.** ("don't like B, show me real
examples") Pulling a struct apart *in the parameter list* instead of the body:
```
// B (the form you don't like):
fn distance(Point { x, y }: Point) -> Float { … }   // x and y available directly
// A (defer): take the whole value, unpack inside:
fn distance(p: Point) -> Float { val x = p.x; val y = p.y; … }
```
Where it's genuinely nice: small math/geometry functions, event handlers
(`fn on(Click { x, y })`), and tuple returns. Where it hurts: the parameter line
gets noisy, and the parameter has no name to refer to as a whole.
- **Recommend A (defer).** It's a readability nicety, not a capability — you can
  always unpack on the first line. Revisit if real Jet code shows the unpack
  boilerplate is constant. (You asked for real examples — I can pull a dozen from
  the showcases if you want to judge from those.)

**D-PAT4 — List patterns like `[head, ...tail]`.** ("how common, where?") This
splits a list into "first element" + "the rest":
```
when xs {
    xs == [first, ...rest] -> { print("head {first}, {rest.len()} more"); };
    xs == [] -> { print("empty"); };
}
```
**How common:** very common in functional languages (Elixir, Haskell, Rust
slices) for recursive list processing — parsers, interpreters, anything that eats
a list one item at a time. **Why we defer:** Jet's `List<T>` is a flat array, so
"the rest" would copy the tail every time (slow and surprising). It's safe and
fast once we have a slice/view design. Recommend A (defer) — it's a real feature,
just blocked on the slice work, not declined.

## 1C — Functions, sugar & readability

**D-FP2 — "Expression-body" functions, and your question: what's the difference
between a named lambda and a quick function?** You picked C (defer). Here's the
distinction:
```
fn double(x: Int) -> Int = x * 2;        // expression-body FUNCTION (the proposal)
val double = (x) => x * 2;               // a LAMBDA stored in a val
```
They behave almost identically. The differences:
- A **function** (`fn`) is a top-level named thing other files can `import` and
  call; it can be generic; it's the unit of documentation. A **lambda** is a
  value you make on the fly, usually to pass to something (`xs.map((x) => x*2)`).
- The expression-body form is just a shorthand so a one-line function doesn't
  need `{ return …; }`. You already have lambdas, so the *only* thing it buys is
  saving `{ return ; }` on tiny named functions.
- **Defer (C) is reasonable** because lambdas + normal `fn` already cover the
  ground. If you later find lots of `fn f(x) { return expr; }` one-liners in real
  code, we add the `= expr;` form then. No urgency.

**D-FP6 — List spread `[...xs, y]`.** ("don't understand") This builds a new list
by "pouring in" an existing one and adding items:
```
val more = [...names, "Zoe"];        // every name in `names`, then "Zoe"
val joined = [...a, ...b];           // a's items followed by b's items
```
- A (rec): **defer** — for now use a library call like `names.concat(["Zoe"])`.
- B: add the `...` spread syntax.
- **Recommend A** — a `concat`/`with` method does the same job without new
  syntax, and `...` spread interacts with ownership (does it copy or move `xs`?)
  in ways worth settling carefully later. Plain explanation: it's a convenience
  for "this list plus a few more"; deferring loses nothing but a little sugar.

**D-SUGAR3 — Transparent type alias.** ("community tradeoffs") A *type alias*
gives a second name to an existing type — purely cosmetic, no new type:
```
type UserId = Int;        // UserId is just Int, interchangeable everywhere
fn ban(id: UserId) { … }  // documentation value only — an Int still fits
```
**Community split:** people like aliases for readability in signatures
(`Headers` instead of `Map<String, String>`), but the classic complaint is they
*hide* the real type and lull you into thinking `UserId` and `Int` are distinct
when they're freely swappable (a real bug source). The stronger alternative is a
**newtype** (D-SUGAR4) — a genuinely distinct one-field type the compiler keeps
separate. My rec: defer the transparent alias; if you want "a distinct ID type",
that's the newtype conversation. Tell me whether you value the cheap readability
(alias) or the safety (newtype).

**D-SUGAR5 — `defer` cleanup keyword.** ("more info") `defer` schedules a bit of
code to run when the current function exits, no matter how it exits:
```
fn write_report() {
    val f = files.create("out.txt");
    defer f.close();          // runs on every exit path, even on error
    f.write(data)?;
}
```
The thing is, Jet already does this **automatically** via RAII (S63): the file
closes itself when it goes out of scope — you never write `f.close()` at all. So
`defer` would mostly duplicate something that's already free. Where `defer` still
helps is *non-resource* actions (stop a timer, log "done"). My rec (A): rely on
automatic cleanup, no `defer` keyword, and reopen only if real code shows a
recurring need RAII can't cover. This ties into the broader safety-by-default
theme you raised in D-IO2.

**D-SUGAR6 — `?.` through methods.** ("don't know what this means") You already
have `?.` for *fields* — "reach into this only if it isn't empty":
```
val city = user?.profile?.city;       // works today — stops safely if user is null
```
The question is whether `?.` should also work before a **method call**:
```
val label = user?.display_name();     // call display_name() only if user isn't null
```
Today that second line is an error (`?.` reaches fields but not methods). Option
A extends it to methods too — same idea, no new symbol, just removes an
arbitrary limitation. **Recommend A**; it's a natural completion of a feature you
already shipped.

## 1D — Error handling, I/O & resources

**D-IO2 / D-IO3 — Resource cleanup & whole-file helpers.** ("more discussion —
everything should be safe by default" / "don't know what you're asking")
These are two halves of one story:
- **D-IO3** asks: keep the simple `fs.read("file")` / `fs.write("file", text)`
  one-liners that read or write a whole file at once? (vs forcing everyone to
  open a handle, loop, and close.) **Yes — keep them** as the easy default; they
  are the safe, obvious tool for small files. This is the "works how you expect"
  default you want.
- **D-IO2** asks how cleanup works for the *bigger* case (streaming a large file,
  a network socket) where you hold an open handle:
  - A (rec): **automatic** — the handle closes itself when it goes out of scope,
    on every path including errors (RAII, S63). You never forget to close.
  - B: you must call `close()` yourself.
  - C: a `defer` keyword (see D-SUGAR5).
  - **A is exactly "safe by default, no footgun"** — forgetting to close is
    impossible because there's nothing to forget. This is the recommendation and
    it matches your stated principle. The example:
    ```
    fn copy(src: String, dst: String) -> Unit ? {
        val input = files.open(src)?;     // both handles close automatically
        val output = files.create(dst)?;  // on scope exit — even if `?` fails
        input.stream_to(output)?;
        ok(unit)
    }
    ```
  Let's confirm A; it's the no-footgun choice. The "more discussion" you wanted is
  really about whether RAII alone is enough vs adding `defer` (D-SUGAR5) — and my
  answer is RAII first, `defer` only if proven necessary.

## 1E — Tooling, testing & build

**D-DX5 — External subcommands (`jet-foo` → `jet foo`).** ("not clear") This lets
anyone extend the `jet` CLI without us building a plugin system: if there's a
program named `jet-bench` on your PATH, then typing `jet bench` just runs it.
Exactly how `git` finds `git-lfs`.
```
$ which jet-bench-compare          # any executable named jet-<x>
/usr/local/bin/jet-bench-compare
$ jet bench-compare old.json new.json   # jet dispatches to it
```
- A (rec): support this PATH discovery, no formal plugin API.
- B: don't.
- C: a full plugin API.
- **Recommend A** — zero cost, lets the community add commands, and keeps the
  core small (no plugin framework to maintain). It's the cheapest possible
  extensibility.

**D-TEST1 — Property testing.** ("don't know what this means") Normal tests check
one example ("`add(2, 3)` is `5`"). A *property* test checks a rule across
*hundreds of random inputs* the framework generates for you:
```
test "reversing twice gives the original" {
    for_all((xs: [Int]) => {
        require(xs.reverse().reverse() == xs);   // tried on 100s of random lists
    });
}
```
It's great at finding edge cases you'd never think to write by hand (empty list,
huge numbers, weird Unicode). - A (rec): include it **if a small clean design
exists**; B: require it; C: defer. **Recommend A** — it's a beloved feature, but
only worth it if it stays simple. We design it small or skip it for now.

**D-TEST2 / D-TOOL2 — `todo` typed holes (and your "syntax for compiler-level
things" idea).** ("don't know what this means") A *typed hole* lets you leave a
blank that still compiles, so you can sketch a program top-down:
```
fn parse(s: String) -> Config {
    todo;        // compiles; if it ever runs, panics "not implemented: expected Config here"
}
```
The compiler even tells you *what type* belongs in the hole. Great for
"scaffold now, fill in later" without red squiggles everywhere. - Rec was defer
(B) unless cheap. **Your bigger idea** — "a syntax for higher-level/compiler
things with clean ergonomics" — is worth its own thread: `todo`, `derive`,
`comptime`, `pure`, maybe `transact`/`async` are all "compiler-directed markers."
That's the **attributes** discussion in Part 3; let's design their look together
rather than piecemeal. Recommend: defer `todo` until we settle the attribute
look.

**D-TOOL4 — Snapshot testing.** ("more info") A *snapshot* test captures a
function's output once, saves it to a file, and on later runs flags any
difference — you "bless" the new output with one keystroke when the change is
intentional. (It's how this very compiler tests its error messages.) Great for
testing anything with big text output (formatters, reports, error messages)
without hand-writing the expected string.
- A (rec): build it into `jet test` with one-key blessing.
- B: defer.
- **Recommend A** — it's a force-multiplier for testing CLIs and we already use
  the pattern internally, so the design is proven.

**D-TOOL5 — Build-time capability summary.** ("more info") This prints what
*powers* a program uses, computed from its imports — an honesty feature so you
can see at a glance "this CLI touches the network and the filesystem":
```
$ jet build report.jet --capabilities
report.jet uses:  filesystem (read/write), network (http)   — no FFI, no unsafe
```
Like Deno's permission summary, but informational. - A (rec): **defer** — nice
transparency, not blocking. B: add now. **Recommend A (defer).**

## 1F — Expert / low-level, networking, FFI

**D-CFFI2 — How C libraries get found.** ("show me use cases") When you call into
a C library (say raylib), the compiler needs to find its header file and the
compiled `.so`/`.a` to link against. Two real-world scenarios:
```
// Scenario 1 — a system library installed via your package manager:
extern c "raylib" { fn init_window(w: Int, h: Int, title: String) = "InitWindow"; }
// A: jet asks `pkg-config raylib` for the include/link flags automatically.

// Scenario 2 — a header sitting in your own repo:
// C: you point jet at the paths yourself in pack.jet.
```
- A (rec): use **pkg-config + standard flags** (the Linux/Mac convention) read
  from `pack.jet`; works out of the box for any properly installed lib like
  raylib, sqlite, zlib.
- B: bundle the library's source.
- C: manual paths only.
- **Recommend A** — it's how C/C++/Rust all locate system libs, so raylib "just
  links". C is the fallback for odd cases. Since you picked raylib as the
  showcase (D-CFFI3), A is what makes that demo painless.

**D-NET1 — TLS/HTTPS.** ("don't know enough") To make an HTTPS request you need
TLS (the encryption behind the padlock). Writing crypto yourself is famously
dangerous, so the question is *whose* TLS we borrow:
- A (rec): use **rustls** (a respected, audited Rust TLS library) through our FFI
  layer — never hand-rolled.
- B: use **openssl** (the C one — ubiquitous but a historical source of CVEs and
  a build headache).
- C: defer TLS to Epoch 3.
- **Recommend A** — rustls is memory-safe (fits I1's spirit), pure-Rust (clean
  builds), and well-audited. This is the safe-by-default choice; you don't need
  to be a TLS expert to bless it.

**D-NET2 — How servers handle many requests at once.** ("don't know the
difference") When 100 people hit your service at once, how does it juggle them?
- A (rec): **thread-per-task + channels** — each request gets its own worker;
  they coordinate by passing messages, never by sharing memory (the ownership
  model forbids the dangerous sharing). Simple to reason about; scales like Go
  did early on — great for internal services, not for 100k simultaneous
  connections.
- B: a small **async** exception (more scalable, much more complex, and async is
  reserved for later — see E2-V5).
- C: a fixed thread pool.
- **Recommend A.** It's the honest, safe model that matches our concurrency
  story (S53) and is plenty for the services Epoch 2 targets. The tradeoff:
  A is simpler and safer but tops out lower than async; we're explicitly fine
  with that ceiling for now.

**D-LL2 — How experts audit `unsafe` code (and the "attributes" idea you liked).**
("I like the term attributes — track async, transact, etc.") When an expert uses
`unsafe` (raw memory), we want a paper trail. Options: a structured audit comment
+ a lint (A), an **attribute** marker (B), or an external tool (C). You flagged
that "attribute" is the right concept and should also cover `async`, `transact`,
etc. — so I've pulled this into a dedicated **attributes thread (Part 3)**: let's
list every "marker that changes how a function is treated" in one place and pick
their shared look together, rather than deciding `unsafe`'s in isolation. Holding
D-LL2 for that thread.

## 1G — Pure eval, JetOS, cross-compile, observability

**D-PURE1 / D-PURE2 — "Pure eval" and sandboxed package recipes.** ("discussion")
"Pure eval" means running Jet code at build time that is **guaranteed to have no
side effects** — it can't read the clock, hit the network, or touch random files;
same inputs always give the same output. That guarantee is what lets a build be
*reproducible* and *cacheable* (this is the Nix idea you like). "Recipes" =
package build instructions written in this pure subset.
- **D-PURE1:** A (rec) = pure eval **plus** sandboxed recipes; B = pure eval only;
  C = full JetOS. Recommend A — recipes are the payoff (reproducible package
  builds), and they're what connect to the Jetpack/hangar store.
- **D-PURE2:** how strict is the sandbox? A (rec) = **no ambient I/O or network
  at all** during eval; B = an allowlist; C = trust the author. Recommend A — the
  whole value is the guarantee; an allowlist leaks it. This is the
  "safe-by-default, reproducible like Nix" position. Worth a short discussion to
  confirm scope, but the direction is clear.

**E2-V12 — "JetOS / pure eval / layer-3 boundary."** ("what is this, doesn't make
sense") Fair — it bundled three unrelated things. Untangled:
- *Pure eval* (above) — build-time pure functions. Real, useful, near-term.
- *Layer 3* — the most advanced compile-time feature (user-defined `derive`,
  reflection); explicitly post-1.0.
- *JetOS* — the long-horizon "Jet all the way down to the OS" vision.
The original question was just "how far down this road do we commit in Epoch 2?"
Given your other answers (pure eval yes via D-PURE; JetOS is a someday), the
answer is effectively: **ship pure eval + recipes in Epoch 2, keep JetOS as
research, defer layer-3.** I'd retire E2-V12 as redundant once D-PURE1/2 are
settled — it's not a separate decision.

**D-CROSS2 — Crash behavior on tiny targets.** ("discussion") When Jet runs on a
microcontroller (no operating system), and something goes wrong (a `panic`), what
should happen? On a normal computer it prints an error and exits. On bare metal
there's nowhere to print and nothing to exit to.
- A (rec): **abort** — just halt. Simple, predictable.
- B: let the developer install a **custom handler** (blink an LED, reset).
- C: full unwinding (heavy, usually unavailable on tiny chips).
- **Recommend A** as the default with B as an opt-in hook later. Embedded folks
  expect "halt on fault" as the baseline. Low urgency (this is the M15
  freestanding milestone).

**D-CROSS3 — Proving embedded actually works.** ("more info") How do we *test*
that Jet runs on a microcontroller without buying a lab of hardware?
- A (rec): a **documented local harness** — run the freestanding build under an
  emulator (like QEMU) so CI can prove it boots, no physical board needed.
- B: real hardware wired into CI (expensive, flaky).
- C: docs only, no actual smoke test.
- **Recommend A** — emulator-based smoke tests are the standard, cheap way to keep
  the embedded target honest. Defer-friendly; it's an M15 detail.

**D-DEV2 — JIT (just-in-time compilation) in `jet dev`.** ("more info") `jet dev`
gives instant feedback while you edit. A *JIT* would make the running program
itself faster by compiling hot code to machine code on the fly (what the JVM
does). It's a big, optional engineering investment.
- A (rec): **write a design note, build nothing** in Epoch 2.
- B: actually implement one (using Cranelift) — large scope, needs your sign-off.
- C: don't mention JIT at all.
- **Recommend A** — keep the idea documented but don't spend the effort now;
  `jet dev`'s interpreter is fast enough for the feedback loop. Revisit post-GA if
  profiling shows a need.

**D-OBS1 — Step debugger timing.** ("don't know what this means") A *debugger*
lets you pause a running program, step line by line, and inspect variables — the
red-dot-breakpoint experience in VS Code. "DAP" is the standard protocol that
makes that work across editors. The only question is *when*:
- A (rec): ship it for VS Code/Cursor in M12, **before** GA.
- B: at GA. C: after GA.
- **Recommend A** — a working debugger is table-stakes for "production platform"
  and a huge credibility signal, so land it before launch.

**D-OBS3 — Metrics conventions.** ("don't know what this means") When a service
runs in production, ops people want numbers out of it — request counts,
latencies, error rates ("metrics"). The question is how much of that we build in:
- A (rec): start with **simple structured logs** (machine-readable log lines),
  and align with the **OpenTelemetry** standard (the industry-standard metrics
  format) later.
- B: full OpenTelemetry now (heavy).
- C: logs only, forever.
- **Recommend A** — structured logs cover the common need immediately; we grow
  into full metrics when there's demand, using the standard so it interoperates.

## 1H — Strategy & supply chain

**E2-V2 — What does "production platform" mean at GA?** ("not sure what the
question is") When we declare Epoch 2 "done", how high is the bar?
- A (rec): credible for **internal services and CLIs** (tools a company runs for
  itself).
- B: also **public-facing SaaS** (apps strangers on the internet use).
- C: also **regulated/audit-heavy** industries (finance, healthcare compliance).
Each step up adds years of hardening (B needs serious security review; C needs
formal audit trails). Given your "be the endgame language eventually but I launch
when *I'm* happy" stance (E2-V10), I'd set the *GA* bar at A — internal services
and CLIs done excellently — and treat B/C as the road *after* GA. This isn't
lowering ambition; it's picking what "version 1 of the platform" must nail. Your
call on the bar.

**E2-V5 — Concurrency, and your "function trait/feature" idea.** ("reserve for
epoch 3, more discussion; may want this as a function attribute like transact")
The question was whether to add `async`/`await` (a second, more complex way to do
concurrency) in Epoch 2. You said reserve it for Epoch 3 — agreed and recorded.
Your deeper note — that `async` might be a *function attribute* like `transact` —
is exactly the **attributes thread (Part 3)**. So: `async` is **out of Epoch 2**
(decided), and `async`-as-an-attribute goes on the attributes list for when we
design that. Two clean outcomes from one question.

**E2-V7 — Networking ambition.** ("don't know what this means; be better than Go")
This just asks how far the *networking* story goes in Epoch 2:
- A: internal HTTP services and CLIs only.
- B (rec): also small **public** APIs with HTTPS/TLS.
- C: defer networking to Epoch 3.
Your "better than Go for tooling and libraries" goal points at B — Go's
reputation is built on easy network services, so to beat it our showcase service
must terminate TLS and serve a real API (which D-NET1's rustls choice enables).
Recommend B. (The "better than Go" bar is captured under E2-V3, which you already
set to "all, non-negotiable.")

**E2-V8 — Supply chain, the Nix store idea, and `package#version`.** ("want
nix-store-style hashes; change package syntax to `package#version`") Three
things in your answer:
1. **Hash-locked store like Nix** — yes, this is already the direction: the
   Jetpack **hangar** store at `/etc/jet/hangar/` keys realized packages by
   content hash, exactly like the Nix store (D-JPK12/16). Your instinct matches
   the plan. ✅
2. **Enterprise support (vendoring, audit, SBOM, mirrors)** — recorded as
   in-scope (option B), since you said enterprise support matters. The
   per-project-vs-store flexibility you mentioned (E2-V4) is honored: teams can
   vendor if company policy demands it.
3. **`package#version` syntax** — ⚠️ **this conflicts with an already-ratified
   decision** and needs its own call. See the dedicated thread in **Part 3**,
   because we previously *rejected* `#` for package refs (`nixpkgs:fastfetch`,
   not `nixpkgs#fastfetch`). Your reasoning ("`#` means number") is reasonable
   for the *version* slot specifically, so this is a live question, not a flat
   no — Part 3 lays out the options.

## 1I — Library authoring & references (jargon decoded)

**D-LIB1 — Timing of two library features.** ("not clear") Two upgrades for
people *writing reusable libraries*, and whether they land together in M6:
- **S61 "labels & defaults":** call a function with `name: value` for clarity and
  let parameters have default values — `schedule("backup", delay: 30)` where
  `repeat` defaults to 1.
- **S62 "delegation":** a one-line way to forward a capability to a field instead
  of hand-writing wrapper methods.
- A (rec): ship **both** in M6. B: labels only, delegation later. C: delegation
  only.
- **Recommend A** — both are already-designed (S61/S62), both make library code
  cleaner, and they pair well. This is just "do the planned thing"; nothing exotic.

**D-LIB2 — How far generics go.** ("don't know what this means") *Generics* =
write a function/type once that works for many types (`Stack<T>` works for
`Stack<Int>`, `Stack<String>`). The question is which advanced generic features
M6 includes:
- A (rec): **associated types + default method bodies** — enough to write rich
  traits (e.g. an `Iterator` trait with a built-in `.map`).
- B: also trait inheritance (one trait requires another).
- C: also blanket impls (apply a trait to *every* type matching a bound).
- **Recommend A** — it's the practical sweet spot; B and C add power but also the
  kind of complexity that makes error messages worse. Start at A, grow later if
  real libraries need it. (Jargon: "associated type" = a type that rides along
  with a trait, like "the element type of this collection".)

**D-LIB3 — Same as D-ERR2** (the `?` error-conversion trait). Decided as A with
your "rename to `Error`" note — see Part 3.

**D-REF2 — Arenas.** ("no idea what this is") An *arena* is an expert
memory-management tool: instead of allocating and freeing a thousand small
objects one by one, you grab one big block, allocate everything inside it
super-cheaply, then throw the whole block away at once. It's a performance
technique for things like parsers (allocate a whole syntax tree, free it in one
shot). - A (rec): **only ship arenas if the parser example actually needs them**;
B: always ship; C: never in Epoch 2. **Recommend A** — it's an expert
optimization; we add it when a real showcase demands it, not speculatively. You
never *need* to know arenas exist as a beginner.

**D-REF3 — Inlay hints beyond clone.** ("more explanation") *Inlay hints* are the
little grey annotations your editor draws *into* the code (not actual text) to
show what's happening invisibly — e.g. showing where Jet auto-clones a value, or
where a borrowed value is returned, or where cleanup runs. They teach the
ownership model by making the invisible visible.
- A (rec): turn on **borrowed-return + cleanup-scope** hints by default (plus the
  clone hint we already have).
- B: clone hint only. C: all hints off by default.
- **Recommend A** — these hints are a core part of teaching ownership gently
  (your "Blueprint-level friendliness" goal — show the wiring). They're
  dismissible, so the cost is low.

## 1J — Transactions (`transact`)

**D-TXN1 / D-TXN2 / D-TXN3 — the `transact` feature.** ("collect attributes
first" / "community perspective" / "discussion") `transact` wraps a block so that
if anything inside fails partway, **all the in-memory changes are undone** — the
program's state looks like the block never ran (borrowed from Verse). Example:
```
transact {
    player.spend_stamina(10)?;   // changes player
    player.step(target)?;        // if THIS fails, the stamina is refunded
}
```
- **D-TXN1 (adopt it, and as what shape?)** — you want to first **list every
  "attribute"-like marker** (`transact`, `async`, `pure`, `unsafe`, …) and pick
  their shared syntax together. Agreed — that's the **attributes thread (Part 3)**.
- **D-TXN2 (I/O inside a transaction?)** — the honest answer is I/O **can't** be
  rolled back (you can't un-send a network packet). So the rec is: doing I/O
  inside a `transact` is a **compile error** (A), which keeps the "everything
  undone" promise truthful. The "community perspective" you asked for: every
  serious implementation (databases, STM in Haskell/Clojure) draws exactly this
  line — transactions roll back *memory*, never *the outside world*. So A is the
  well-trodden, safe answer.
- **D-TXN3 (cost)** — snapshotting state isn't free. Rec: snapshot **only the
  bindings actually mutated**, and only when you opt in with `transact` — never a
  cost on normal code (A). B (snapshot the whole scope) is simpler but wasteful.
- **Net:** the feature is sound; the only open piece is its *spelling*, which
  rides on the attributes thread. Semantics (A/A/A) are ready to confirm.

## 1K — JSON typed decode (D-JSON1) — glaze-inspired options

You asked me to study C++'s **glaze** library and translate its ergonomics into
Jet. Here's what makes glaze pleasant, and three cohesive Jet surfaces built
from pieces Jet *already has* (so this adds almost no new syntax).

**What glaze does well** (from its docs): (1) **pure reflection** — an ordinary
struct serializes to/from JSON with *zero* annotations or macros, computed at
compile time; (2) **opt-in customization** — when you need to rename or skip a
field, you specialize `glz::meta<T>` and *only* the keys you mention change, the
rest stay automatic; (3) **automatic enums**; (4) it's one of the fastest
libraries in the world because it maps straight onto the struct's memory.

The Jet question (D-JSON1) is the *decode surface*: how does `text → Profile`
look? Three options, in increasing "magic":

**Option A — explicit `derive` (most cohesive with what's ratified).** Reuses
S55 (`derive Serialize`) + generics (S33) + the rich `Error` (S80). You mark a
struct serializable once; decode is a generic call:
```jet
struct Profile {
    name: String;
    score: Int;
    derive Serialize;          // S55 — the one opt-in line
}

fn load(path: String) -> Profile ? {
    val text = fs.read(path)?;
    ok(json.decode<Profile>(text)?)   // typed; field mismatch → JSONError w/ field name
}
```
Pro: explicit, consistent with S55's "Serialize is a deliberate opt-in" rule
(a wire format is a semantic commitment). Con: one `derive` line per type —
slightly less magic than glaze.

**Option B — glaze-style pure reflection (most magic).** *Any* struct decodes
with no `derive` at all — `json.decode<Profile>(text)?` just works:
```jet
struct Profile { name: String; score: Int; }   // no derive line
val p = json.decode<Profile>(text)?;            // works anyway (reflection)
```
Pro: glaze's headline ergonomic — zero ceremony, matches your "feels like
magic" instinct. **Con / tension:** it *contradicts* S55, which deliberately
made `Serialize` an explicit opt-in because committing a public wire format
silently is a footgun. Choosing B means amending S55 for JSON specifically.

**Option C — A as the default, with glaze-style partial overrides for the 10%.**
Ship A (explicit `derive`), and when you need to rename/skip a field, attach the
mapping *inside* the derive — only the fields you name change, the rest stay
automatic (exactly glaze's `glz::meta` "modify" model):
```jet
struct Profile {
    name: String;
    score: Int;
    internal_id: String;
    derive Serialize {
        rename score -> "user_score";   // only this key changes
        skip internal_id;               // never written/read
    }
}
```
Pro: covers the real-world need (snake_case APIs, hidden fields) the way glaze
does, while keeping S55's explicit-opt-in safety. Con: a small amount of new
syntax inside the `derive` block.

**My recommendation: C** — it's the faithful glaze translation that *respects*
your existing decisions: explicit opt-in (S55), one generic call to decode
(S33), great field-level errors via the rich `Error` (S80), automatic enums
(glaze-style), and the partial-override escape hatch for renames/skips. It only
goes "full magic" (B) where you've already decided not to (silent wire formats).
Pick **A** (minimal), **B** (full magic, amend S55), or **C** (recommended).
Unknown-key policy is already decided — **D-JSON2 = ignore unknown keys by
default, opt-in strict.**

## 1L — D-ERR2: name the error-conversion capability (30-second confirm)

You chose A for `?` cross-type conversion and said "rename the trait to `Error`."
The mechanism is ratified in **S80**; only the *name* is stuck, because `Error`
is already the **type** you return. Three ways out:
1. **Capability is `Error` (a trait); the concrete default carrier gets a new
   name** (e.g. `Fault` / `Failure`). You'd write `impl FileError: Error { … }`,
   and `-> T ?` boxes "some `Error`" (consistent with S48 trait-as-type). Most
   Rust-like; needs a name for the concrete type (a quick naming menu from me).
2. **Keep `Error` as the concrete type; name the conversion trait something
   else** — e.g. `impl FileError: IntoError { … }` or `: AsError`. No type
   rename; the trait reads as "can become an Error."
3. **No trait at all** — `String` and std errors convert automatically; your own
   error converts by implementing a method `to_error(self) -> Error`. Least
   machinery, but no `impl … : …` capability to attach.
**My recommendation: 1** (it's the glaze/Rust-idiomatic "`Error` is the
capability" model and reads best), and I'll bring you a naming menu for the
concrete carrier. Pick 1, 2, or 3.

---

# Part 2 — Decided (for the record)

These are settled; listed so you can scan what you chose. Full caveats live in
Part 0's ratification queue. ★ marks where you chose **against** the prior
recommendation (so it's not lost).

| ID | Your decision | Note |
|---|---|---|
| E2-V1 | A & B | beginners + small teams |
| E2-V3 | **all of them** | beat Python/Node/Go/Rust/Zig — non-negotiable |
| E2-V4 | A (+ optional packages) | single-file `jet run` stays sacred |
| E2-V6 | full low-level, safe by default | expert opt-in for low-level |
| E2-V9 | A & C | VS Code/Cursor + Zed + Neovim |
| E2-V10 | manual launch | you launch when happy |
| E2-V11 | post-Epoch-2 | governance deferred |
| E2-D1 / E2-D2 | normal SemVer / never | no encoded epoch version ★(E2-D2 was rec C) |
| D-REL1 | A | normal SemVer |
| D-REL2 | manual | you control bumps |
| D-REL3 | A | `edition` field |
| D-REL4 | C | no LTS pre-GA |
| D-REL5 | A | only `jet fix` + edition upgrade may migrate |
| D-DX1 | A | stable `--json` schema |
| D-DX2 | A & B | health checks **and** auto-fix |
| D-DX3 | A | Zed dev extension |
| D-DX4 | A | ship completions + man pages |
| D-DX6 | A | OSC-8 terminal hyperlinks |
| D-DEV1 | A (+try-anyway flag) | interpret common programs |
| D-DEV3 | A | <200 ms diagnostic budget |
| D-REF1 | A | references taught after ownership chapter |
| D-LIB1 | (see Part 1I) | leaning A |
| D-FP1 | A | field punning |
| D-FP3 | A | core `module name { … }` |
| D-FP4 | A (+explicit typing) | empty-list inference |
| D-FP5 | A | expressions in `for` heads |
| D-IO1 | A (+ergonomics) | `std.path` module |
| D-PKGS1 | A (B later) | git registry now |
| D-PKGS2 | A | reserved `jet.*` namespace |
| D-PKGS3 | A | optional signed metadata v1 |
| D-PKGS4 | A (probably) | immutable releases + yank — wants brief discussion |
| D-LR1 | all in Epoch 2 | full ring this epoch |
| D-LR2 | A (pure-Jet later) | sqlite via C FFI |
| D-LR3 | broad as safe | crypto — vetted impls only |
| D-LR4 | **B** | add `jet.yaml` in wave 1 ★(rec was A defer) |
| D-NET3 | A | sqlite-first showcase |
| D-TEST3 | **B** first | docs-led learning, `jet tour` later |
| D-TEST4 | A | doctests under `jet test` |
| D-OBS2 | A | safe locals in dev-mode panics |
| D-LL1 | A | amend I1 for user-gated `unsafe` |
| D-LL3 | A (+wider expert API) | `std.mem` narrow core + opt-in wide tier |
| D-CFFI1 | A (+export later) | import-only C FFI first |
| D-CFFI3 | raylib | ship a raylib showcase |
| D-CROSS1 | A | one CLI cross target |
| D-PURE3 | **B** | ship signed cache in M16 ★(rec was A) |
| D-GA1 | **B** | all 6 showcases mandatory ★(rec was A) |
| D-GA2 | **B** | hard CI perf/size gates ★(rec was A) |
| D-GA3 | none | no beta before GA |
| D-GA4 | normal SemVer | = E2-D2 |
| D-ERR1 | A | grow the `Error` carrier |
| D-ERR2 | A (rename to `Error`) | opt-in conversion trait |
| D-ERR3 | A | fallible `main` allowed |
| D-ERR4 | **B** | add `?continue` in loops ★(rec was A defer) |
| D-PAT1 | A | nested patterns in `when` arms |
| D-PAT3 | **B** | refutable bind requires `?? fallback` |
| D-SUGAR1 | A | already S67 — no-op |
| D-SUGAR2 | A | decline pipe `\|>` |
| D-SUGAR4 | A | decline newtype keyword (but see D-SUGAR3) |
| D-SUGAR7 | A | already S6 — no-op |
| D-OWN1 | A | keep clone lint (your perf question answered in Part 1, below) |
| D-OWN2 | A | ownership mini-examples |
| D-OWN3 | A | suggest `take` at call site |
| D-FS2 | A & B | game-loop example + `poll_input` helper |
| D-JSON2 | A | JSON ignores unknown keys, opt-in strict |
| D-TOOL1 | A | doctests run under `jet test` |
| D-TOOL3 | A | gated `jet emit --rust` |
| D-BUILD1 | A | `jet doctor` FFI section |
| D-BUILD2 | A | `jet build -v` prints bridge steps |
| D-REPL1 | A | ship terminal REPL in Epoch 2 |
| D-REPL2 | A | terminal only (no web playground yet) |
| D-REPL8 | A | real move semantics across lines |
| D-REPL9 | A | brace-count multi-line prompt |
| D-REPL10 | A | sandbox (no `jet.toml` — now `pack.jet`) |
| D-REPL11 | C | line-editing crate + completion |
| D-REPL12 | A & B | REPL and `jet eval --pure` both |
| D-REPL13 | A | REPL shares library with `jet dev` only |
| D-REPL15 | B | meta-commands `:load` `:type` `:help` |
| D-REPL16 | B (+`;` suppression) | echo type+value; `;` silences |
| D-REPL17 | A | identical diagnostics to batch |
| D-REPL18 | A | `rustyline` (needs I6 waiver) |
| D-REPL19 | C | defer web playground |
| D-REPL21 | A | separate E2-M18 milestone |

**D-OWN1 — answering your performance question** ("how is cloning all the time
good for performance?"). It isn't, and the lint is precisely how we stop it: the
implicit-clone lint (L0201) **flags every place Jet inserts a clone** so you can
see the cost and remove it (by passing ownership with `take`, or borrowing). The
clone is a *beginner safety net* — your program is always correct — but the lint
makes the cost **visible and removable**, so experts write zero-clone code. So
"keep the lint" (A) is the pro-performance choice: it's the opposite of silent
cloning. The alternative (B, silence it for scalars) would hide cheap clones; we
keep it loud and teach the fix. Cloning is never forced — it's the safe default
you can always opt out of.

---

# Part 3 — Cross-cutting threads your answers opened

## 3A — The "attributes" list (you asked to collect these first)

You said (D-TXN1, D-LL2, E2-V5, D-TEST2): before picking syntax for `transact`,
`async`, etc., gather **every marker that changes how a function or block is
treated** into one place, then design their shared look once. Here is the full
inventory to design against. (★ = already has a spelling; the rest are open.)

| Marker | What it does | Status |
|---|---|---|
| `pure` ★ | function provably has no side effects | ratified S60 (`pure fn`) |
| `comptime` ★ | binding/expr evaluated at compile time | ratified S57 |
| `unsafe` ★ | block/fn may break memory safety; audit-gated | ratified S58 |
| `derive` ★ | auto-generate a trait impl | ratified S55 (`derive Trait;`) |
| `test` ★ | a test block | ratified S43 |
| `transact` | roll back in-memory changes on failure | **open** (D-TXN) |
| `async` | cooperative concurrency | **open, Epoch 3** (E2-V5) |
| `todo` | typed hole / unimplemented stub | **open** (D-TEST2) |
| `extern` ★ | foreign (Rust/C) function | ratified S50/S59 |

**The design question to settle once:** these currently use *three* different
shapes — a **prefix keyword** (`pure fn`, `unsafe fn`, `comptime x`), a **body
line** (`derive Trait;`), and a **block** (`unsafe { }`, `transact { }`). Do we
want one unifying look (e.g. all prefix keywords), or is "keyword for
fn-modifiers, block for scoped effects" actually the right split? My
recommendation: **keep the two natural shapes** — prefix keyword when it modifies
a whole function (`pure`, `async`, `transact fn`), block when it scopes an effect
to part of a function (`unsafe { }`, `transact { }`) — and *don't* invent a
single `@attribute` sigil (it reads as noise and collides with nothing we
currently have). But this is a syntax decision that's yours: I've put the
inventory in one place so you can judge the whole set. **Nothing in this table
gets built until you rule on the shared shape.**

## 3B — `package#version` syntax (⚠️ conflicts with a ratified decision)

You want package versions written `package#version` because "`#` means number".
Two ratified facts collide with that:
- **D-JPK7/D-JPK15** ratified that package *refs* use a colon —
  `nixpkgs:fastfetch`, `github:owner/repo` — and **explicitly rejected `#`** (so
  we don't look like Nix's `nixpkgs#fastfetch`).
- **S76** (just ratified, fixed-size lists `[T#N]`) **already uses `#`** and its
  text claims "`#` appears nowhere else in Jet surface syntax." So adding
  `package#version` would make `#` mean two things.

**But there's a silver lining that actually helps your idea:** in `[T#N]`, the
`#` already reads as *"a specific count/number"* (`[Point#2]` = "2 points"). So
`parsekit#1.2.0` (`#` = version number) is *thematically consistent* — `#`
always introduces "a pinned number." That makes a unifying story possible rather
than a clash. Three coherent ways forward:

1. **`#` = "a pinned number" everywhere** — keep `:` for source, add `#` for
   version: `github:acme/parsekit#1.2.0`, and amend S76's "appears nowhere else"
   line to "`#` introduces a pinned number — a list length `[T#N]` or a package
   version `pkg#ver`." Internally consistent, honors your instinct, doesn't
   reopen the rejected source-`#`. **My recommendation if you want `#`.**
2. **Keep the current ratified forms (status quo)** — version lives in the dep
   struct (D-JPK23): `parsekit: { git: "…", tag: "v0.4.1" }`; simple pins are
   `textkit: "1.2.0"`. No `#` for versions; S76 keeps `#` to itself.
3. **Reopen and switch sources to `#` too** — `nixpkgs#fastfetch#1.2.0`. Advise
   against — discards the deliberate "don't look like Nix" decision and `#` twice
   is ambiguous.

**What I need:** pick 1, 2, or 3. If 1, I'll amend D-JPK + S76 to reserve `#` for
the "pinned number" role (list length *or* version) and ratify the
`source:pkg#version` form. Genuine new syntax, so it stops here until you choose.

## 3C — Two things you flagged that are already done ✅

- **"We are changing to `when` from `switch`" (D-PAT1).** Already shipped —
  `when` was ratified as **S24** on 2026-06-15; `switch` now only produces a
  teaching error pointing at `when`. The pattern-matching examples in this file
  already use `when`. Nothing to do.
- **"There is no `jet.toml` anymore" (D-REPL6, D-REPL10).** Correct and already
  done — the manifest is **`pack.jet`** and the single lockfile is **`.jet/lock`**
  (S52 amended 2026-06-16); the old TOML constants were removed from
  `src/syntax.rs`. All ballots have been updated to say `pack.jet`.

---

## Tally (open items only — what still needs you)

| Thread | IDs awaiting your input |
|---|---|
| REPL behavior | D-REPL3, 4, 5, 6, 7, 14, 20 |
| Pattern/functions | D-PAT2, D-PAT5, D-PAT6, D-FP2 |
| Sugar | D-SUGAR3, D-SUGAR5, D-SUGAR6, D-FP6 |
| I/O & resources | D-IO2 (confirm A) |
| Tooling/testing | D-DX5, D-TEST1, D-TEST2/D-TOOL2, D-TOOL4, D-TOOL5 |
| Low-level/net/FFI | D-CFFI2, D-NET1, D-NET2, D-LL2 |
| Pure eval / cross / obs | D-PURE1, D-PURE2, D-CROSS2, D-CROSS3, D-DEV2, D-OBS1, D-OBS3, E2-V12 |
| Strategy / supply chain | E2-V2, E2-V5 (confirm), E2-V7, E2-V8 |
| Library / references | D-LIB1, D-LIB2, D-REF2, D-REF3 |
| Transactions | D-TXN1/2/3 (semantics ready; spelling → 3A) |
| **Attributes shape** | **3A — design the shared look** |
| **`#` version syntax** | **3B — pick option 1/2/3** |
| Research-blocked | D-JSON1 (analyze glaze; offer: run a deep-research pass) |

## Already ratified (do not re-ballot)

Groups 1–11, 13 are decided and live in their canonical homes
(`docs/spec/syntax-decisions.md`; plans under `docs/plans/`). Concurrency (S53),
`pure fn` (S60), labels/defaults (S61), delegation (S62), RAII cleanup (S63), C
FFI gate (S59), `when` (S24), the `pack.jet`/`.jet/lock` manifest (S52 amended)
are all ratified; the ballots above decide only their *timing and surface
details* inside Epoch 2.
