# Effects on types: the end state

Status: proposal for owner review (card #6129). Nothing here is ratified.
Each new spelling becomes a ballot only after the owner corrects this
document. Facts about today's language are marked **today** and come from the
2026-10-29 read of dev `04fdfbde1d`. Everything else is the proposed end state.

## 1. The idea in one paragraph

Today Jet attaches effects to functions. A function's row says what it may do
(`-[FS.Read]>`), the empty row says it does nothing (`-[]>`), and rows are
inferred when they are not written. The proposal attaches the same rows to
every other thing that can contain behavior: a **type**, a **trait**, a
**module**, a **package**, a **build target**, and each **compiler extension
point**. A row on any of them states which effects code reached through it may
use and which it must never use. Packages, targets and compiler parts are then
ordinary types that carry rows. That is how "the language is a library" becomes
checkable. A library, a platform or a compiler plug-in is a type, and its
authority is its type.

There is one rule, one vocabulary and one checker on every rung. Beginners
write none of it; every row is inferred. Experts write a row where they want a
hard limit, and the compiler proves it.

## 2. Design rules

1. **One rule.** Every rung has an *effect row*, which is the set of effects
   that code reached through it can perform. A written row is a *ceiling*
   (`A, B` means "at most these") plus *denials* (`!C` means "never this,
   including through calls"). The checker enforces `inferred ⊆ ceiling` and
   `inferred ∩ denied = ∅` on every rung.
2. **Rows only restrict.** A row on a type, trait, module or target never
   grants authority. Authority is granted only at the root, which is the
   package and the run, as today (`authority: { allow: [...] }`, `--allow`,
   `#FX(grant: …)`). This keeps the boundary rule "tighten silently, loosen
   needs a written gate".
3. **Inferred by default.** An unwritten row is inferred from the code. Errors
   appear only where someone wrote a limit, or where a root (a package grant
   or a target) is exceeded.
4. **Zero runtime cost.** Rows are compile-time facts and are erased in code
   generation, like typestate today.
5. **One vocabulary.** The effect tree in the Prelude (`Net`, `FS.Read`,
   `Mem.Alloc` and so on) is used on every rung. Libraries add effects with
   `effect`, which is the existing declaration form.
6. **Nearest rung wins, and denials always win.** Ceilings compose by
   intersection from the outermost rung inward. A denial anywhere above code
   applies to it.

## 3. Spellings

Functions keep their row inside the arrow, because the row is part of the
function's type. Every other rung uses the existing `#FX(...)` marker, which
today already spells the block rung.

|Rung|Today|End state|
|---|---|---|
|Block|`#FX(IO) { … }` and `#FX(grant: FS, IO) { … }`|unchanged|
|Function|`fn f(p: String) -[FS.Read]> String`|unchanged|
|Function type|`fn(T) -[Net]> U`|unchanged|
|**Type**|none|`#FX(Log, !Net) struct Client { … }` (also `enum`, `union`)|
|**Trait**|per-method bound only (D-EFF3)|`#FX(!Net, !FS) trait Codec { … }`|
|**Module**|none (D-EFFECT-RUNGS1 is open)|`#FX(!Net) module parse { … }`, or a file-head `#FX(...)`|
|**Package**|`authority: { allow: [...], deny: [...] }`|unchanged; it *is* the package's row|
|**Dependency**|`authority: { grants: { "dep": [DB, Net] } }`|unchanged; now checked against the dependency's inferred row|
|**Target**|none|`#FX(...) target Gpu { … }` declared in Core as library code|
|**Compiler extension**|tiered `comptime` rules plus an advanced-API opt-in|the row of the extension point's function type|

The list syntax inside `#FX(...)` is exactly the function row's syntax:
`#FX()` means pure, `#FX(Net, FS.Read)` is a ceiling, `#FX(!Net)` is a denial,
`#FX(Log, ..E)` is an open tail, and `#FX(!Mem.Alloc(above: 4096))` is a
bounded denial.

In the Jai-style declaration form the owner is reviewing, the marker sits in
the same place: `Client :: #FX(Log, !Net) struct { … }`.

## 4. Types

### 4.1 Meaning

The row of a type `T` is the union of:

- the rows of all its methods, both inherent methods and trait impls;
- the rows of its function-typed fields (`on_error: fn(Error) -[Log]>` adds
  `Log`);
- the rows of the types of its fields, transitively, computed as a fixed
  point for recursive types.

Written `#FX(...)` on the declaration makes that union a checked ceiling.

```jet
#FX(Log, !Net, !FS)
struct Parser {
    text: String
    on_warn: fn(String) -[Log]>
}

impl Parser {
    fn warn(self, msg: String) -[Log]> { self.on_warn(msg) }   // fits
    fn fetch_schema(self) -> String { http.get(SCHEMA_URL) }   // error
}
```

```text
error[E0749]: `Parser.fetch_schema` uses `Net`, which `Parser` denies
  --> parse.jet:12:39
   |
 1 | #FX(Log, !Net, !FS)
   |          ---- `Parser` says its code never uses the network
12 |     fn fetch_schema(self) -> String { http.get(SCHEMA_URL) }
   |                                       ^^^^^^^^^^^^^^^^^^^^ `http.get` uses `Net`
   = why: every method of a type is checked against the type's effect row
   = fix: move the fetch out of `Parser` and pass the schema text in, or
          remove `!Net` from `Parser`'s row if the parser really may fetch
```

The denial code is the existing **E0749** ("a prohibited effect was
reached"), now reported on every rung. A ceiling overrun that is not a denial
is the existing **E0712**, now generalized from `#FX` blocks to every `#FX`
rung.

### 4.2 Using a type spreads its row

Calling a method adds that method's row to the caller, as today. The new part
is that a type row also bounds code that *receives* a value of the type
through a trait or a function value, where the concrete code is not known:

```jet
fn render_all(items: [Widget]) -[]> [String] {   // pure bound
    items.map(w -> w.render())                  // allowed only if Widget's row is pure
}
```

### 4.3 Generic types

A generic type's row is computed for each instantiation. `Box<T>`'s row is
`Box`'s own methods plus `row(T)` wherever `T`'s methods are called. A
ceiling may name the tail of a type parameter:

```jet
#FX(Log, ..T)                     // Log plus whatever T brings, nothing else
struct Traced<T> { inner: T }
```

`..T` reads as "the effects of `T`". It is the open-tail syntax `..E` from
function rows, with a type parameter as the tail. No separate effect
parameter list is needed. *Choice point C1: alternatively, require a named
effect parameter `struct Traced<T, ..E>`.*

### 4.4 Enums and unions

These follow the same rules. Payload types contribute their rows. A variant
holding `fn() -[Net]>` makes the enum's row include `Net`.

## 5. Traits

A trait row is a ceiling on **every** implementation, including impls in
other packages, and it makes calls through a trait value precise:

```jet
#FX(!Net, !FS, !Exec)
trait Codec {
    fn encode(self) -> Bytes
    fn decode(bytes: Bytes) -> Self?
}

impl Invoice.Codec {
    fn encode(self) -> Bytes { … }                  // fine
    fn decode(bytes: Bytes) -> Invoice? {
        log.info("decoding")                        // fine: Log is not denied
        files.read(SCHEMA)                          // E0749: Codec denies FS
    }
}
```

When a trait has **no** written row, a call through a trait value (for
example `[Shape]` or `s: Shape`) gets the union of the rows of every impl in
the built program. Jet builds whole programs from source, so that union is
known at build time. Beginners never see a row on a trait. Library authors
write one when they want to promise something to callers. The existing
per-method bound (D-EFF3, `fn render(self) -[GPU]> Image`) stays as the method
rung under the trait rung.

## 6. Modules

```jet
#FX(!Net, !FS, !Exec, !Time, !Rand)     // file head: the whole module is
                                         // deterministic and offline
use core.text as text

fn tokenize(s: String) -> [Token] { … }
```

A module row bounds every item in the module, nested modules included. This
answers the module rung of D-EFFECT-RUNGS1 with the same marker as the block
and type rungs. An inline module takes the marker before `module`:
`#FX(!Net) module pure_math { … }`.

## 7. Packages are types

### 7.1 The package row

A package's row is its `authority:` block. **Today** that block already
exists (`allow`, `deny`, `needs`, `grants`, `trust`, `providers`). The end
state keeps that block and that spelling. What changes is its meaning: the
package is a type, the package's public surface is that type's members, and
`allow`/`deny` are that type's row.

```jet
package {
    name: "invoices"
    authority: {
        allow: [FS.Read, Log]
        deny:  [Net, Exec]
    }
}
```

### 7.2 Dependencies are checked by type

When a package depends on another, the dependency's **inferred** row is
checked against what the depending package grants it. This turns
supply-chain review into type checking:

```jet
package {
    name: "shop"
    deps: { json: "1.4", sql: "2.0" }
    authority: {
        allow:  [Net, FS, Log]
        grants: { json: [], sql: [Net, Log] }   // json gets nothing, sql gets Net and Log
    }
}
```

```text
error[E0762]: dependency `json` 1.5.0 uses `Net`, which `shop` does not grant it
  --> package.jet:6:19
   |
 6 |         grants: { json: [], sql: [Net, Log] }
   |                   -------- `shop` grants `json` no effects
   = note: reached through json.parse -> json.schema.fetch_remote -> http.get
   = why: 1.4.2 used no effects; 1.5.0 added a network call
   = fix: keep json at 1.4.2 (`jet update json --to 1.4.2`), or grant it
          `Net` with `jet allow json Net` if the new behavior is wanted
```

The beginner path stays implicit, and the result stays explicit and reviewable:

- `jet add json` writes the dependency's current inferred row into `grants`.
  It is written in the file, so it shows in review diffs.
- An upgrade that keeps the row inside its grant needs nothing.
- An upgrade that widens the row fails with the call path that causes it,
  plus a one-command fix.

The grant is the explicit expert control, and the tool writes it, so
beginners are never asked about effects up front.

### 7.3 Importing a package gives a typed value

`use sql` binds a module value whose type is the package type. Passing a
module value or a member function to code with a pure bound is checked like
any other value. No new syntax is needed for this. *Choice point C2: whether
module values may be stored in fields and passed as arguments (first-class
modules), or only named. The proposal is "named only" for now (simpler,
pillar 7), with first-class modules a separate ballot if a real program needs
them.*

### 7.4 Library-defined effects

**Today** a package can declare an effect leaf with `effect Log.Audit`. The
end state lets a library declare its own effect and say which kernel effects
it is built on:

```jet
// in package `sql`
effect Query uses [Net]          // performing sql.Query relies on Net
effect Migrate uses [Net, FS.Write]

fn select(q: String) -[Query]> [Row] { … }   // body may use Net: covered by Query
```

The rules are:

- Inside the declaring package, a body whose row is `Query` may use
  everything `Query uses`.
- Outside the package, callers see `sql.Query`, which is precise and
  readable, not raw `Net`.
- Denying a kernel effect denies every library effect built on it, so
  `!Net` blocks `sql.Query`. A library effect can never be used to sneak past
  a denial.
- Library effects are qualified like library markers (`sql.Query`,
  D-MARKER-MODULE1).

This is pillar 5 applied to effects. `Log`, `DB`, `GPU`, `Browser` and
`Secret` move out of the kernel and become effects declared in Core with
`effect … uses [...]`. The kernel keeps only true primitives: `FS`, `Net`,
`Exec`, `Env`, `Time`, `Rand`, `Mem`, `FFI`, `Panic`, `IO`. *Choice point C3:
the exact kernel list.*

## 8. Targets are types

A build target is a type whose row says what that platform can do. Targets
are declared in Core as ordinary library code, so a new platform is a library
change:

```jet
// Core
#FX(!FS, !Exec, !Env, !FFI.C, Browser, Net, Time)
target Web { pointer_bits: 32, word_bits: 32, cache_line: 64 }

#FX(!FS, !Net, !Exec, !Env, !Time.Wait, !Mem.Alloc, !Panic, !FFI)
target Gpu { pointer_bits: 64, word_bits: 32, cache_line: 128 }

#FX(!Mem.Alloc(above: 0), !Net, !Exec)
target Embedded { pointer_bits: 32, word_bits: 32, cache_line: 32 }
```

Code is checked against every target it is built for. A function reached
from an entry point built for `Gpu` must fit `Gpu`'s row. The same source can
build for CPU, GPU and web in one `fn build`, and each artifact is checked
against its own target:

```jet
fn build(b: Build) {
    app :: b.add_executable("viewer", ["run.jet"], [])
    b.add_target(app, .Native)                                  // the host CPU
    b.add_target(app, .Web)
    kernels :: b.add_kernels("blur", ["blur.jet"], target: .Gpu)
}
```

```text
error[E0763]: `blur_row` cannot run on target `Gpu` because it allocates
  --> blur.jet:18:16
   |
18 |     scratch :: [Float](len: width)
   |                ^^^^^^^^^^^^^^^^^^^ allocates on the heap (`Mem.Alloc`)
   = why: target `Gpu` denies `Mem.Alloc`; GPU kernels cannot allocate
   = fix: take a preallocated `View<Float>` argument, or use a workgroup array
```

Target facts (word size, pointer size, cache-line size, CPU features) are the
target type's fields, read as `$build.target.word_bits` and so on. This is
also the home for card #6126's per-target settings:

```jet
b.add_target(app, .Native{ cpu: [.AVX2] }, settings: { tile: 64 })
b.add_target(app, .Web, settings: { tile: 16 })
```

Code that should differ per target branches on a fact
(`comptime if $build.target.word_bits == 64 { … }`) or on the target itself
(`comptime if $build.target is Gpu { … }`).

## 9. The compiler is types

### 9.1 Extension points are function types with rows

Every place user code runs inside the compiler is a public function type in
`core.compiler`, and its row is the contract:

|Extension point|Type (in `core.compiler`)|Row|
|---|---|---|
|Compile-time value|`comptime { … }`|`#FX()`: pure by default|
|Pinned build input|`embed_file`, `find`, pinned `fetch`|`Build.Input`, recorded in `.jet/lock`|
|Impure preparation|`#Impure("reason") { … }` plus `--gate impure=allow`|ambient effects, audited|
|Marker expansion|`fn(Site) -[Compiler.Read]> Syntax`|reads the program, cannot touch the machine|
|Derive|`fn(TypeInfo) -[Compiler.Read]> Syntax`|same|
|Lint|`fn(Program) -[Compiler.Read]> [Diagnostic]`|same|
|Code generation|`fn(Program) -[Compiler.Read, Compiler.Publish]>`|may add items, never replace them (D-META-PUBLISH2)|
|Build action|`fn(ActionInput) -[...declared...]> ActionOutput`|declared, which makes caching safe|
|Target provider (GPU and so on)|`fn(KernelIR) -[Compiler.Read]> Bytes`|pure translation|

Today's three comptime tiers become these rows:

- **Tier 0** is `#FX()`.
- **Tier 1** is the new leaf `Build.Input`.
- **Tier 2** is ambient effects granted by `#Impure` plus the build gate.

The checks and codes E3410–E3413 keep their meaning.

### 9.2 Compiler authority is effects

The **today** "advanced compiler API" opt-in (`compiler.advanced.register`,
`register_expansion`, `register_specialization`, `session`; D-META-CONTROL2
and D-META-OPTIN2) becomes the effect leaves `Compiler.Register` and
`Compiler.Session`. They are granted in the package `authority:` block like
any other effect. That leaves one mechanism instead of two. `Compiler.Read`
is part of the comptime floor; `Compiler.Publish` is granted per package.

### 9.3 One compiler library for every tool

`core.compiler` exposes these as versioned public types:

- `Syntax`, `Item`, `TypeInfo`, `Fact`, `Diagnostic`, `Program`;
- `KernelIR` and the shared IR;
- the opaque handles that exist today (`TypeId`, `DeclarationHandle` and the
  rest).

The CLI, the language server, the formatter, `fn build`, markers and outside
tools all call them (pillar 5). Each tool's own row is then checkable. A
language server plug-in typed `-[Compiler.Read]>` provably cannot write files
or use the network.

### 9.4 Build reuse falls out

A build action whose row is pure apart from declared `Build.Input` is
reusable by construction: same inputs, same output. Reuse needs no separate
"verify" mode (D-BUILD-REUSESET1) and no separate trust flag. An action that
declares ambient effects is never reused. Remote builds (D-REAPI1) can ship
exactly the actions whose rows make them reproducible.

## 10. Compile-time folding

This answers card #6125 with the same rule. A `::` binding whose inferred row
is empty, and whose value is computable, is folded at build time as an
optimization, at every rung (module, function, block). `comptime { … }`
demands folding and turns a failure into a build error. A `::` binding whose
row is not empty is computed at run time. That is the silent demotion that
happens today, now stated as the rule.

## 11. How existing mechanisms fit

|Mechanism (today)|End state|
|---|---|
|Function rows `-[E]>`, `-[]>`, `-[!E]>`, `..E`|Kept. The function rung.|
|`#FX(...) { }` blocks, `#FX(grant:/authority: …)` handles|Kept. The block rung; the handle stays the one way to pass authority as a value.|
|Package `authority:` (`allow`, `deny`, `grants`, `trust`, `providers`)|Kept. It is the package type's row. `grants` is now checked against inferred dependency rows.|
|`policy: { effects: { deny } }` (not enforced today)|Retired. The package rung covers it.|
|`effect Name` declarations|Kept, extended with `uses [...]`.|
|Comptime tiers, `#Impure`, `--gate impure=allow`|Kept, restated as rows (section 9.1).|
|Advanced compiler opt-in|Replaced by `Compiler.*` effect leaves.|
|`#Unsafe("reason")` and the inferred Unsafe effect|Kept. Unsafe stays an effect discharged only at an audited boundary.|
|Fact tags (`#Secret`, `#PII`, `#Input`, `#Credential`, `#Scrub`)|Kept and separate. Tags track where *data* flows; rows track what *code* does. A type may carry both.|
|Typestate `#State`|Kept and separate.|
|Per-method trait bounds (D-EFF3)|Kept. The method rung under the new trait rung.|

## 12. Errors

|Code|When|
|---|---|
|E0749 (existing, widened)|A denied effect is reached on any rung.|
|E0712 (existing, widened)|An effect outside a written ceiling is used on any rung.|
|E0747 (existing)|A callback's row exceeds the parameter's bound.|
|E0760 (new)|A type's ceiling is exceeded through a *field* type or function-typed field. The note shows the field path.|
|E0761 (new)|An impl in another package exceeds a trait's row. Reported at the impl.|
|E0762 (new)|A dependency's inferred row exceeds its grant. Shows the call path and the version that widened it.|
|E0763 (new)|Code reached from an entry point does not fit the target's row.|
|E0764 (new)|A library effect's body uses a kernel effect not listed in its `uses`.|
|E0765 (new)|A compiler extension exceeds its extension point's row.|

Every new code ships with what/why/fix text and a UI snapshot (I4). The
numbers are placeholders until the registry assigns them.

## 13. Edge cases decided

- **Recursive types** compute rows by fixed point. Rows only grow, and the
  tree is finite, so this terminates.
- **Closures stored in fields** contribute their function type's row. A
  field typed `fn(T) -> U` with no row is inferred from every closure stored
  into it in the program.
- **Tasks and threads**: spawning a task carries the task body's row into the
  spawner, so a type that spawns work cannot hide its effects.
- **FFI**: a foreign call is `FFI.<lang>`. A foreign function pointer stored
  in a type adds `FFI` to that type's row. The FFI ballot's safe-mapping
  design does not change this.
- **Generic code with no bound** passes rows through (`..T`) and never needs a
  written row.
- **Dynamic trait values without a trait row** get the union of the impls in
  the built program (section 5). A *published* library's metadata records its
  own impls' union, so tools can show it.
- **Comptime versus runtime**: a type used in both places is checked against
  each phase's row separately. A type that prints is fine at runtime and an
  error only inside a pure `comptime` block.
- **Tests**: a test target is a target type. `#Test` code is checked against
  the test target's row, so tests that must be offline can say `!Net` once
  for the whole suite.

## 14. What a beginner and an expert each see

**A beginner** writes no rows, no `#FX`, no grants. `jet add` fills grants.
The only effect errors they meet are:

- a dependency upgrade that suddenly wants new powers (E0762, with a
  one-command fix);
- code that cannot run on a chosen target (E0763, naming the line).

Both are errors they want to see.

**An expert** writes:

- `#FX(!Net)` on a module, type or trait to promise behavior to callers;
- grants to lock down dependencies;
- target types for new platforms;
- typed compiler extensions.

Every limit is visible where it is written (pillar 8), and every limit uses
the same marker and the same list syntax (pillar 4).

## 15. Choice points for the owner

- **C1:** the generic tail spelling, `#FX(..T)` versus a named effect
  parameter `<T, ..E>`.
- **C2:** module values: named only (proposed) or first-class.
- **C3:** the kernel effect list after `Log`, `DB`, `GPU`, `Browser` and
  `Secret` move to Core.
- **C4:** the target declaration keyword. `target Web { … }` is a new
  keyword. The alternative is a Core `struct` that implements a `Target`
  trait (no new keyword; pillar 7 favors this).
- **C5:** the library effect spelling, `effect Query uses [Net]` versus
  `effect Query: [Net]`.
- **C6:** whether `jet add` writes the inferred grant (proposed) or asks first.
- **C7:** whether marker expansion gets `Compiler.Read` by default (proposed)
  or by grant.

## 16. Ballots that this proposal affects

- **D-EFFECT-RUNGS1:** the module rung spelling is answered by section 6.
- **D-BUILD-REUSESET1 and D-REAPI1:** reuse safety comes from rows
  (section 9.4).
- **D-MARKER-REFLECT1, D-MARKER-EXPAND1 and D-MARKER-DIAG1:** marker
  expansion's contract is its row (section 9.1).
- **D-FFI-UNSAFE1:** the FFI root and the Unsafe effect are unchanged.
- **D-SERVICE-TYPED1:** a service is a type, so its row is its declared
  authority.
- **#5006 and #6126:** targets and per-target settings (section 8).
- **#6125:** folding (section 10).
