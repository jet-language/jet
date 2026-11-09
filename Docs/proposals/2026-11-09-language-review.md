# Jet language review, 2026-11-09: every installment (1 to 8)

Written 2026-11-09 by Main for the owner to read on return. Installments 1 and 2 and every answer so far are in
`$JET_SCRATCH_ROOT/lanes/Main/language-review.md`.

**How to answer:** every item has a number. Reply with numbers, for example "4.1 yes, 5.2 yes, 8.3 tell me more,
6.4 no". Anything you approve becomes a ballot or a criterion; nothing here is built yet.

**How this was made:** from a fresh view, as you asked. I did not read old audits, proposals or reports, and I left
speed out. I read the spec sections that say what Jet is today and ran small programs on the current compiler
(seed a0_042). Where a claim was run it says "checked"; where I am going by the spec or by judgment it says so.

Marks: **[N]** near, buildable in months on today's compiler. **[F]** far, a year or more, or it needs other
pieces first.

---

## Read this first: a correction from the first adversarial review (16:30)

The adversarial reviewer went through the `#` ballot (D-META-HASH2) against Tower and the compiler and marked it
NOT CLEAN. Three of its findings correct things I told you, so they change how to read sections 3 and 8 below:

1. **Much of what I proposed for compile time is already ratified, just not built.** I worked from the spec and
   registry, which still show the old forms. Tower already holds decisions for: no `$` and `#{name}` splices
   (D-META-HASH1); markers as types with typed arguments, including the wrap power (D-DECL-TYPES1); one
   reflection entry `reflect(type-or-value)` with its record types (D-TYPEINFO1, D-META-REFLECT2); and who may
   rewrite code, through a compiler value that only the root package creates (D-META-LADDER1). So `#info(x)` would
   duplicate the ratified `reflect(x)`, and "markers take arguments" needs building, not deciding.
2. **My one-line rule was too strong.** "No `#` means it runs in your program" is false as written: module
   constants, generic parameters and build functions are build-time with no `#`. The honest rule is narrower:
   an explicit request to the compiler carries `#`.
3. **Two parts of my proposal conflict with what you ratified.** Dropping the host gates for build-time effects,
   and letting a marker's presence authorize a wrap, both go against D-META-LADDER1.

The reviewer added an option it prefers to mine as drafted: **a spelling-only cutover** (`#run`, `#if`, `#loop`
replace the `comptime` word) that keeps every ratified contract as it is. My reading now: the compile-time area
feels weak and messy mostly because roughly ten ratified decisions are unbuilt and the spec, registry and guide
still show the old language. The fastest real improvement is to build what is ratified and add the spelling
cutover, not to redesign. The ballot is being repaired to say this plainly, with the same real programs under
every option. Section 3 below is kept as written so you can see what I proposed; read it with this note.

The same caution applies to the rest of this file: I did not check Tower's ratified decisions before writing
it, so some items in sections 4 to 8 may also be decided already. Each one is checked against Tower before it
becomes a ballot.

**Second correction, same day: the cycle rule for shared state (item 2.2, D-SHARED-CYCLE2, NOT CLEAN).** I told
you a two-sentence rule would replace the unpredictable one. The reviewer ran the probes and showed it is not
sound as I stated it:
- "A frozen value may reference its own type freely" means nothing today, because freezing a value that contains
  a `Shared` is already refused (E1114). The exception only exists if immutable shared values are redesigned.
- A check on the type declaration alone misses hidden edges: a `Shared` that stores a callback which captures
  that same `Shared` is a cycle no declaration shows. Catching it needs the flow information I said we could drop.
- You have already ratified an opt-in collector for exactly this (`#Policy(gc)`, D-GC-OPTIN1) and the current
  cycle law (D-SHARED-CYCLE1=C). The collector does not work yet.

The reviewer added the option I now think is the honest one: **repair the existing write-time check and build
the ratified opt-in collector**, and keep `Pool` with `ID` as the taught way to build graphs. That keeps your
rulings and removes the false alarms without a new ban. The ballot is being repaired to show real working
rewrites of five graph shapes under every option.

**Third review: the arena block (item 2.5, D-REGION-ALLOC1, NOT CLEAN).** The idea stands, but the hard part is
not yet designed: when code inside the block calls a function, the compiler must know that what the function
returns lives in the arena, across packages, or the escape check cannot hold. Also open: what a copy out of the
block means for identity, and what happens on a panic or when a task starts inside the block. It is being
repaired with those rules and a real workload.

**Fourth review: effect arguments (O8, D-EFFECT-ARGS1, NOT CLEAN).** Two corrections to what I told you. First,
my example `Billing.Charge(max: usd(100))` does not work as a "covers" rule: two charges of 60 are each under
100 and together over it, so a budget is not something a per-call check can express. Arguments fit scopes (a
path prefix, a host), not running totals. Second, written effect rows with literal arguments are an error today,
and allowing them reverses a ratified decision (D-DECL-TYPES1); the ballot now says so. The reviewer added a
simpler option: arguments compared by equality only, with no prefix or wildcard rules.

## Where we stand

| Decided | State |
|---|---|
| A file of top-level statements just runs | Ruling, #6281 c128. ScriptLead is building it. Top-level bindings are locals of the program, in order. |
| The arrow always comes before the result; error-first signatures are invalid | Ruling, #6281 c127 (455 Core signatures to migrate). |
| No warnings; tidiness never stops `jet run`, and `jet check` / `jet build` list everything | Ruling, #6281 c129. |
| `shared` is a type: `Shared{value}`; the keyword goes | Ruling, #6281 c130. |
| A module-level `name: value` is always evaluated while building; failure is a build error | Ruling, #6281 c132. |
| `#Persist` means only "survives hot reload" | Ruling, on #6281. |
| `#` means "for the compiler"; `#run`, `#if`, `#loop`; `#{name}` splices; `comptime` and `$` go; `use` stays a word | Approved in direction. Ballot **D-META-HASH2** is being drafted, then reviewed. See the note below. |
| One type-level rule for cycles in shared state | Approved as a proposal. Ballot **D-SHARED-CYCLE2** is drafted and with the reviewer. See the note below. |
| A block in which ordinary collections allocate from an arena | Approved as a proposal. Ballot D-REGION-ALLOC1 is being drafted. |
| Withdrawn after your answers | Call-site `&` marks; removing `<>`; `is` for arm tables; `State` as a written effect. |

**Two of these reopen decisions you already ratified.** I worked from a fresh view and did not know; the agent
drafting the ballots found them. You said everything ratified is open in this review, but you should see what
you would be reversing:
- **D-META-HASH1, ratified 2026-10-30.** It already decided "no `$` at all" and `#{name}` for splices, so on
  those two points today's proposal agrees with you. It differs on three: HASH1 made compiler facts plain values
  (`build.target.os`, `T.reflect()`) and site values `#line`, `#file`, `#package`; today's proposal writes
  `#build`, `#info(x)`, `#here`. HASH1 is not built yet: the spec and registry still show `$`. The new ballot
  shows HASH1 exactly as ratified as one of its options.
- **D-SHARED-CYCLE1, ratified as option C** (card #6066, with a criterion already met). Today's type-level
  rule would reverse it. The new ballot says so.
- The arena-block ballot (D-REGION-ALLOC1, drafted, with the reviewer) builds on two ratified decisions about
  allocation context and shows them as an option.

**Still waiting for your answer from installments 1 and 2:**

| # | Question | My recommendation |
|---|---|---|
| O1 | A plain module variable: `score := 0` at file scope that any function reads and writes directly, owned by the main task; the compiler refuses to start a task that touches it | Yes. It is what games and scripts need, and it stays race-free. |
| O2 | `String ParseError!` becomes `String ! ParseError` | Yes. Two types joined by a space is the only such place in the grammar. |
| O3 | Delete the second result-branch form `r ? ok -> … ! error -> …` | Yes. The arm table already does it, and `?` and `!` keep one meaning each. |
| O4 | `fn(…) { … }` is a value in any scope (today `add: fn() { … }` inside a function is an error; checked) | Yes. |
| O5 | Replace the `try_new` / `try_push` / `try_insert` twins with one scope in which allocation failure is an error value | Yes, but it needs design. Expert-only. |
| O6 | One-liners move under their own verb, `jet eval '…'`, as you suggested | Yes. |
| O7 | Markers get the fourth power, wrap (retry, cache, trace) | Offer it in the ballot with and without. I lean yes, with `jet expand` showing the result. |
| O8 | Effects take arguments: `FS.Read("assets/")`, `GPU(.Vulkan)` | Yes, limited to build-time constants. Own ballot, D-EFFECT-ARGS1. |

---

## 3. Compile time: the whole design on one page

You asked for a big rework here. The `#` proposal you approved is its base. This is the full shape, so you can
judge it as one thing.

**3.1 One rule.** No `#`: it runs in your program. `#`: it is addressed to the compiler.

| Kind | Looks like | Set |
|---|---|---|
| Directive: "do this while building" | `#run`, `#if`, `#loop` | Small and closed |
| Marker: "this has a property" | `#Test`, `#Unsafe("…")`, `#Retry(times: 3)` | Open; libraries declare their own |
| Question to the compiler | `#build`, `#package`, `#here`, `#caller`, `#phase`, `#info(x)` | Small and closed |
| Splice | `#{name}` | One form |

**3.2 A marker is a type; its fields are its arguments.** Typed, named, with defaults. Its `impl Marker` block
gives it meaning, as a ladder: tag, check, add, wrap (O7). A whole-program query, "everything marked with this",
is what route tables, test lists, command lists and a game's system list are built from.

**3.3 One reflection record.** `#info(x)` returns one typed record for a type, function or value. The build
function gets the same record, and the run-time `reflect.of(value)` returns the part that exists at run time.
Today there are four ways to read these facts.

**3.4 One way to generate code.** Typed templates of ordinary items. Never strings, never rewriting code the
user wrote. `derive`, marker bodies and the build function's generation become one mechanism.

**3.5 Build-time code obeys the ordinary effect rules.** Pure by default. Pinned inputs (`embed_file`,
`fetch(url, sha256:)`) are recorded in the lock. Anything else needs a grant from the package, the same grant
mechanism run time uses. `#Impure`, the gate flag and the tier table go.

**3.6 Literal hooks, one mechanism for every embedded language. [N]** SQL, regex, CSS, HTML, format strings and
paths all need the same thing: text that is checked while building and becomes a typed value. Jet already has
checked text types and a build-time literal hook. I would make that the single way to embed a language: a type
declares a build-time parser for its literal, written in ordinary Jet. Then CSS is a library, not a language
feature, and the CSS ballot that failed review today is replaced by something general. (From the spec; not run.)

**3.7 The fence `<: … :>`.** It is the least guessable form in the language. I have not studied it. With
`#loop` and `#{…}` in place I would check whether it is still needed, and cut it if not.

**3.8 What I would not add.** Types as ordinary values (Zig's way: it removes generics but makes signatures hard
to read); string macros; anything that rewrites existing code. Generics with trait bounds, plus `#info` and
templates, cover the ground with code a reader can follow.

---

## 4. Everyday semantics: numbers and small things

**4.1 Number literals should adapt, the way Go's constants do. [N]** Checked: Jet's number defaults are right
for beginners. `Int` does not overflow (9223372036854775807 + 1 prints 9223372036854775808), `7 / 2` is 3.5,
`0.1 + 0.2 == 0.3` is true, and `"héllo".len()` is 5.

The friction, also checked: a literal adapts to what it is passed to, but a named one does not.

```jet
half: fn(x: Float) -> Float { x * 0.5 }

print(half(3.0))    // works: the literal becomes a Float
y: 2.5
print(half(y))      // error E0112: y is a Decimal
```

Naming a number should not change what it is. Proposal:
- An immutable binding of a number literal (`y: 2.5`, `GRAVITY: 9.81`) has no fixed type until it is used, and
  each use adapts, exactly as the literal would.
- A mutable one (`speed := 2.5`) takes its type from how the function uses it, and falls back to Decimal or Int
  only when nothing decides.

This matters most for games and numeric code, where almost every number is a Float. Cost: a little more
inference, and the error message must say which use fixed the type.

**4.2 Ranges are right.** `..` is inclusive and `..<` stops short. Checked: `loop i in 0..xs.len()` is refused
with a message that offers three correct forms. Keep.

**4.3 Two small defects, found by running the spec's examples.** Both are now a criterion on #6281.
- The spec's three-part loop, `loop i := 0, i < 3, i + 1 -> print(i)`, does not parse.
- The E0112 fix text says `Float{3.4}` whatever the number was.

**4.4 Teach three ways to hold changing state, not seven.** After your `Shared` ruling: a plain value (the
default), `Shared{…}` (across tasks), and `Pool` with `ID` (graphs). `Cell`, `Atomic` and guards stay for
experts and leave the beginner path. This is documentation, not a language change.

**4.5 One question I could not answer by reading.** Is an `Int` field in a struct a machine word or a big
number, and what does a game developer write to get a plain 32-bit or 64-bit field? If the answer is "write
`I32`", that is fine; it just needs to be the first thing the game guide says.

---

## 5. Modularity and scale

**5.1 Core is one connected ball. [N]** Evidence from today: `use tasks: core.tasks` loads 64 Core modules
(2.7 MB of source). The chain: the time module's encode and decode implementations are always live; they load
encoding; that loads files, math and memory; then system, process, text, testing and crypto follow. Two rules
fix the cause:
- A trait implementation is loaded only when the program uses both the type and the trait.
- Core gets layers, and a lower layer may not import a higher one: Prelude; then data (text, math, collections,
  time); then system (files, net, process); then services (HTTP, database, UI). A check enforces it.

**5.2 A public API should not change silently. [N]** Jet infers effects and errors from function bodies. That
is right inside a package. At a package's public edge it means an edit to a body can change the public
signature with no one noticing. Core already has a surface ledger with a check. Proposal: every package gets
one. `jet check` compares the public surface (types, effects, errors, markers) with the committed ledger; a
difference is an error until the ledger is updated; and the size of the difference gives the version bump, so
the version number is computed, not chosen.

**5.3 One strictness dial per package. [N]** Script, app, library, hardened. Each step turns on more: a
library must write its public types, effects and errors; hardened forbids panics and unchecked operations. Jet
has the pieces as separate policies and profiles (I have not checked how many). One named dial is easier to
teach, and it is the "ladder, not cliff" pillar applied to rigor.

**5.4 The compiler runs its own migrations. Do this first. [N]** Today's rulings mean rewriting thousands of
sites: 455 signatures, every `shared`, every `comptime` and `$`. The closed-set `else` rule alone touched 4,520
sites in 783 files, with a tool one agent wrote for that job. Rule: every breaking change to the language
registers its rewrite in `jet fix`, so one command updates any code base, the repository's and a user's.
Libraries get the same power for their own APIs: `#Deprecated(replace: …)` with a typed rewrite, so upgrading a
package updates your call sites. Breaking changes that fix themselves are what make it affordable to keep
improving the language, before and after v1.

**5.5 The library test.** For every mechanism, ask: could Core's own use of it be written by a library? Markers,
effects, targets, literal hooks and allocators should all pass. Where the compiler still knows a Core type by
name, that is a defect. One ballot on this is held for the rework (D-TYPE-FACTS1).

**5.6 One verb for "tell me about my program". [N]** There are `jet explain`, `jet inspect`, `jet audit` and
`jet expand`, each showing compiler knowledge. I would fold them into one family, `jet explain <topic>`:
effects, copies, checks, the expansion of a marker, the layout of a type. (Seen in the spec; I have not counted
the flags.)

---

## 6. FFI and incremental adoption

**6.1 What exists.** The spec lists foreign interfaces for 22 languages, from C, C++, Rust, Python, JavaScript
and Go to COBOL, Ada and COM, plus schema binders. I have not tested their depth.

**6.2 The missing direction: Jet called from other languages. [N]** I did not find it in the spec's FFI
section. It is the adoption wedge: nobody rewrites an application, but many will write one new module in a
better language. Proposal: `jet build --lib` produces a C-callable library and its header, and from the same
public surface a Python wheel and an npm package (JVM and .NET later). Types map through the existing binders
run in reverse. The library's effects become the permissions its documentation states. Your module-level ruling
helps here: with no code running before `run`, loading a Jet library has no hidden start-up step.

**6.3 The adoption ladder for an existing code base.**
1. Use `jet` around it: as the script, build and test tool (`jet run x.c` already works).
2. Call the existing code from Jet (exists).
3. Write one new module in Jet and call it from the host (6.2).
4. Move files over one at a time. **[N to F]** `jet port file.c` writes the Jet version, with `#Unsafe` blocks
   wherever it could not prove safety, and generates the tests that show the two behave the same. The repository
   already built this proof for itself: the paired-tool entries with identical-output cases. That mechanism is a
   product feature, not just a house rule.

**6.4 Foreign code under the effect system.** Today a foreign call is one coarse effect. With arguments (O8) it
can name the library: `FFI.C("sqlite3")`. **[F]** Further out: run an untrusted foreign library compiled to
WebAssembly inside the process, holding only the effects it was granted.

**6.5 Zero-copy data exchange. [N to F]** For data and machine-learning work the entry point is a Jet function
called from a Python notebook. That needs Jet's views to map onto Arrow and the Python buffer protocol without
copying.

**6.6 Out of scope for now:** a stable binary interface between Jet libraries. Source packages are enough
until the language stops changing.

---

## 7. What each domain needs

Core already has modules for most of these (App, Web, Ui, Game, Db, Http, Compute, Data, Units and more). I
listed the folders; I did not test their depth. This table is my judgment of what decides adoption and what I
would close first.

| Domain | What decides adoption | Jet's asset today | Close first |
|---|---|---|---|
| Scripts and command-line tools | Starts instantly; one file; no setup | `jet run`, effects that announce themselves | A file of statements (decided); adapting numbers (4.1); `jet eval` (O6); start-up time (the gauntlet's job) |
| Back-end services | Safe dependencies; database; deploys | Effects and grants; tasks; HTTP, TLS and SQL written in Jet | Queries checked while building (8.7); tracing from effects with no code (8.5); least-privilege dependencies (8.8) |
| Web front end | Small downloads; fast edit-and-see loop | Web, Reactive and Ui modules; `jet dev` | CSS and HTML as checked text (3.6) |
| Games | Globals; Floats; frame memory; hot reload | Value semantics; `Pool` and `ID`; `#Persist` | Plain module variables (O1); adapting numbers (4.1); arena blocks (approved); layout markers (8.9); `#System` lists from marker queries (3.2) |
| Data, numeric, machine learning | Works from a notebook; arrays; accelerators | Units, Data and Compute modules | Called from Python with no copies (6.2, 6.5); shapes in types and kernels (8.2, 8.11) |
| Systems and embedded | No runtime surprises; small; auditable | No C underneath; `#Unsafe` with reasons; "no allocation" and "no panic" promises | Targets declared by libraries (ballot being drafted); the report of remaining run-time checks (8.5) |
| Desktop and mobile apps | One code base; looks right; accessible | The single UI framework direction | Not examined |
| Operations and infrastructure | One static binary; safer than shell | No C; permission-scoped scripts | `jet eval`; process and pipe ergonomics (not examined) |
| Teaching | Install in one step; errors that teach | Diagnostics with a fix on every error | One downloadable binary instead of Nix |

---

## 8. Trailblazing ideas, ranked

Each one grows out of something Jet already has that other languages lack: effects the compiler tracks, value
semantics, typed migrations, and a compiler that can answer questions about the program.

**8.1 Breaking changes that fix themselves. [N]** Item 5.4. First, because it pays for everything else.

**8.2 "Where can this run?" as a fact the compiler infers. [N, then F]** This is the Mojo question, answered
Jet's way. The compiler already knows whether a function is pure, whether it allocates, whether it can panic and
which target facts it reads. A function that is pure, allocation-free and bounded can run on any device: SIMD
lanes, many cores, a GPU, an accelerator that does not exist yet. So eligibility is inferred, like effects, and
never annotated: no shader dialect, no second language for kernels. A target is a library-declared type that
states what it can run; `jet explain` says why a function is or is not eligible for it. A new chip becomes a
package. Near: the eligibility fact and CPU SIMD and multi-core. Far: GPU and accelerator back ends.

**8.3 Record and replay, and simulated worlds, from the effect boundary. [N, then F]** Every touch of the
outside world goes through thirteen effect roots. So the runtime can record every result that came in (`jet run
--record`) and replay the run exactly: a bug report ships its recording, and a recording becomes a test. The
same boundary can be handed a simulated world: a fake clock, a network that drops packets, a disk that fails on
the ninth write. That is the testing method behind the most reliable databases, available to every Jet program
with no test doubles written. Near: single-task record and replay. Far: deterministic task scheduling.

**8.4 A public surface the compiler guards, and versions it computes. [N]** Item 5.2. Others do this for types.
Jet can include effects and errors, so "this update started using the network" is a version change the tool
sees.

**8.5 The glass box. [N]** The compiler knows every run-time check it could not remove (bounds, ranges,
overflow), every copy, every allocation and every outside-world call. Show them per line through `jet explain`,
and let a region demand none: "no check remains here" joins the existing "no allocation" and "no panic"
promises. The same knowledge gives tracing for free: a span around every effect call, with no code written.

**8.6 One evolution mechanism for data on disk, on the wire and in memory. [F]** Jet has typed migrations for
schemas and `#Persist` for hot reload. They are the same idea: a type's history is part of the type. Unified,
one declared migration moves data on disk, messages from an older peer and live state across a code reload;
later, a new version of a running service can take over the old one's state with no downtime.

**8.7 The database as typed collections. [N to F]** Jet now has a SQL engine written in Jet. So a table can be
a typed collection, a query an ordinary Jet expression checked against the schema while building, and the
schema's history the migrations of 8.6. No mapping layer and no query strings.

**8.8 Least privilege for dependencies and for AI-written code. [N, after O8]** "This package may read
`./templates` and reach `api.example.com`, and nothing else", checked while building. The same mechanism runs
code an AI agent wrote with a hard ceiling and no sandbox process. In an era of generated code, a language whose
permissions are part of the type system is the safe place to run it.

**8.9 Layout as a marker. [N to F]** `#Layout(.SoA)` on a list of structs: you write ordinary struct code and
the storage is columns. Games and data code both want it; other languages bolt it on or drop it. It would also
be the proof that library markers (3.2) are powerful enough, because it should be a library.

**8.10 The program as a database you can query. [N]** "Which functions touch this state?", "what can fail
here, and where does each error come from?", "what changed in meaning between these two commits?". With `#info`
as one record this is a small step, and it gives merges and code review that work on declarations, not on
lines.

**8.11 Shapes in types, and derivatives as a library. [F]** Arrays whose dimensions are build-time parameters,
so a shape mismatch is a compile error; automatic differentiation generated from `#info`, not built into the
compiler. Together with 8.2 this is the machine-learning story without a second language.

**8.12 Tasks that can move. [F]** A task whose captured values are sendable and encodable could run in another
process or on another machine, and its effects say what that place must allow. The rules that make tasks safe
today are most of what is needed.

---

## 9. The order I would build in

1. Migrations in `jet fix` (5.4), because every item below rewrites code.
2. The `#` rework with markers as types (section 3), then literal hooks (3.6).
3. Adapting numbers (4.1) and the plain module variable (O1): the two biggest everyday frictions I found.
4. `jet build --lib` with the Python wheel (6.2): the adoption wedge.
5. The guarded public surface (5.2) and the Core layering rules (5.1).
6. Record and replay (8.3) and the glass box (8.5): the two trailblazing items that are near and that nobody
   else can copy quickly, because they rest on the effect system.

## 10. What I did not check

- The depth of any Core module or foreign binder; I only know they exist.
- Whether a pure function may read module state today.
- How `#Persist` variables behave across tasks.
- What a user-declared marker can do today (the ballot starts by probing it).
- The fence `<: … :>` beyond its one-line description.
- How many policies and profiles exist behind the strictness dial (5.3), and how the explain, inspect, audit
  and expand commands overlap (5.6).

---

## Appendix: installments 1 and 2, as discussed

### Installment 1: syntax and semantic coherence

| # | Finding | Proposal | Outcome |
|---|---|---|---|
| 1.1 | A write is invisible at the call: `bump(score)` | `&` on write arguments | Dropped: messy in the odd cases. A `:` name can never be changed by a call; only a `:=` name can. |
| 1.2 | `print("hi")` alone in a file is an error | A file of statements just runs | Decided. |
| 1.3 | Two signature orders (1,258 arrow-first, 455 error-first in Core) | The arrow always comes before the result | Decided. |
| 1.4 | `String ParseError!`: two types joined by a space | `String ! ParseError` | Open: O2. |
| 1.5 | Three implicit names: `it`, `<>`, `err` | Drop `<>` | Withdrawn: `<>` marks where piped data goes and pairs with the fence. |
| 1.6 | A second way to branch on a result: `r ? ok -> … ! error -> …` | Delete it | Open: O3. |
| 1.7 | `==` that destructures and binds in arm tables | `is` | Withdrawn after testing: every case behaves as a reader would guess. |
| 1.8 | Compile time has too many spellings | Rework | Section 3. |
| 1.9 | Style lints as warnings | No warnings | Decided, with the builder-friendly rule. |

Also from installment 1: one-liners under their own verb (O6); a comprehension should build a set or map directly;
a pipe step with no `<>` should pass the value as the first argument (not checked whether it does today).

### Installment 2: memory and reasoning

| # | Finding | Proposal | Outcome |
|---|---|---|---|
| 2.1 | Value semantics (checked: after `b: a` and `a.push(4)`, `b` still has 3 items) | Keep; it is the strongest part of Jet | Keep. |
| 2.2 | Shared mutable state: many holders, and a cycle rule nobody can predict | One type-level rule; teach `Pool` and `ID` | Approved as a proposal: ballot D-SHARED-CYCLE2. |
| 2.3 | `add: fn() { … }` inside a function is an error (checked) | `fn(…)` is a value in any scope | Open: O4. |
| 2.4 | `try_new`, `try_push`, `try_insert` double the API | One scope where allocation failure is an error value | Open: O5. |
| 2.5 | No way to give ordinary code arena memory | A block in which ordinary collections allocate from an arena | Approved as a proposal: ballot D-REGION-ALLOC1. |
| 2.6 | Module-level mutable state is a trap: `score := 0` can be declared but no function may write it (checked) | First a `State` effect (withdrawn: friction for games); now a plain module variable owned by the main task | Open: O1. |

Leave alone: `name: value` everywhere; one `if` and one `loop`; read by default with `&` and `&&`; effects as
inferred permissions; errors as values; explicit `.copy()` and `.move()`; views with inferred owners instead of
lifetimes; tasks that must be joined or detached; `freeze`; no async/await; raw pointers only inside `#Unsafe`.
