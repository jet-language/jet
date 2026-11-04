# Declaration style: `fn name(...)` or `name :: fn(...)`?

Research report for the owner, 2026-10-29. Author: DeclStyleResearch.
Repo measured: worktree `Main-dev-collectall`. Samples: `Docs/proposals/jai-style-sample/`.
Scripts and raw output: `measure.mjs`, `census2.mjs` and `out.json` in the research lane.

---

## One-page summary

**The question.** Should Jet declare things keyword-first (`fn area(...)`, `struct Canvas {}`, which is today's style) or name-first, Jai style (`area :: fn(...)`, `Canvas :: struct {}`)? In both styles the kind keyword stays (`fn`, `struct`, …). What changes is where it sits.

**What the evidence says, in five points.**

1. **No controlled study compares the two head shapes.** I searched the eye-tracking and readability literature and found nothing that tests keyword-first against name-first declaration heads. Carbon's designers looked too and found "a lack of compelling data". Everything published on this exact question is designer opinion (Odin, Go, Carbon, Zig, PEP 8, JS style guides). The research that does exist tells us what to optimise for: people read code non-linearly, they scan and search constantly, repeated regular patterns get cheaper with each instance, and familiarity strongly affects speed.
2. **The measured differences in Jet code are geometric and small.** Across the four sample pairs (7,635 lines, 1,064 declarations), Jai style puts every declared name at the indent column. Today the name's distance from the indent varies (mean 3.2–4.3 columns, sd 0.9–1.8). In exchange, the kind keyword moves to a ragged column (mean 11.8–16.4, sd 4.6–5.9). Every declaration line gets exactly 3 characters longer. Lines over 100 columns go from 163 to 176. Across the repo, 946 heads would cross the 100-column limit.
3. **The strongest argument for Jai style comes from Jet itself.** Jet already names values with `NAME :: value` and has no `let`. Seven or more forms are already name-first, and three are hybrids (`alias X :: Y`, `module a :: b<…>(…)`, `marker N = #[…]`). Only name-first can give Jet a single declaration rule. Pillar 4 (Ladders, Not Cliffs) also improves: going from lambda to function never moves the name.
4. **The strongest argument against is also from Jet itself.** In Jai, Odin and Zig, name-first is *semantically true*: procedures and types really are compile-time constants. In Jet it is not. Types are not values (S26), named functions are not generally values (`&`/`^` parameters make a function direct-call-only), and lambdas follow different recursion and return rules. Zig accepted this exact change in 2019 and rejected it in 2023, on the grounds that "functions have symbol names… the syntax inconsistency appropriately models reality". Every keyword-first language that beginners and AI models know well (Python, JS, Rust, Go, Kotlin, Swift) spells the head the way Jet does today.
5. **Tooling cost is small; migration cost is large but mechanical.** The parser needs the same 3-token lookahead that `parser_distinct_ahead` already uses. LSP outline, rename and go-to-definition are unaffected. The tree-sitter and TextMate changes are a few rules each. The codemod touches 76,146 head lines in 14,753 `.jet` files, plus about 1,120 doc heads, about 2,400 golden-output lines and about 2,500 compiler/tool string lines. It also amends about ten ratified decisions.

**Recommendation.** Adopt the engineered version of Jai style (Option B below), but only after a two-part experiment that costs one to three days. My confidence is moderate: about 60/40 in favour. Option B is not the mechanical rewrite in the sample. It adds six explicit rules for the misfit forms: multi-head clauses, bodiless requirements, inline markers, `f :: g`, `extern` entries, and dotted names. If the experiment shows that AI models keep emitting `fn name(` heads after a primer, or that people are measurably slower at "what kind of thing is this?", keep today's style (Option A) and fix its three hybrids instead.

### Comparison table

| Criterion | A: keep `fn name` | B: Jai style, engineered | C: split (types name-first, `fn name` for functions) |
|---|---|---|---|
| One declaration rule | No: two rules (keyword-first items, name-first bindings) plus 3 hybrids | **Yes**: one rule `name :: kind`; 6 non-binding forms stay keyword-led | No: still two rules, but the line falls between types and functions |
| Name at the indent column | 0% of heads | **100%** of heads (when markers sit on their own line) | Types yes, functions no |
| Kind at the indent column | **100%** (minus 6% inline-marker heads) | 0%; kind sits at column 5–33 | Functions yes, types no |
| Line length | baseline | +3 on every head; +946 repo heads over 100 columns | +3 on type heads only (about 10k heads) |
| Lambda → function ladder | name moves | **name stays** | name moves |
| Matches what `::` means in Jet | n/a | spelling yes, semantics partially (see §3) | types: matches `X :: distinct Y`, `X :: alias Y` |
| Prior art | Rust, Go, Kotlin, Swift, C3, Carbon, Python, JS | Jai, Odin (and JS `const f = …`) | Zig (`const P = struct`), Go (`type P struct`), TS (`type P = {}`) |
| Familiarity for beginners and AI models | **highest** (Rust/Go/Python shape) | lower; `fn` keyword kept | mixed |
| Parser/tooling change | none | small (LL(3) lookahead, grammar rules) | small |
| Migration | none | 76k heads + docs + goldens + about 10 decisions amended | about 11k heads + docs |
| Biggest risk | stays inconsistent; ladder cliff stays | suggests a "functions are values" model Jet does not have | two rules and no ladder win; the worst of both |

---

## 1. Readability

### 1.1 What I measured

`measure.mjs` pairs each original Core file with its mechanical Jai-style rewrite and measures every declaration head. Changed lines equal declaration lines in all four files, so the rewrite touched nothing else.

| File | Lines | Decl. heads | Today: name offset from indent (mean / sd) | Jai: kind column (mean / sd / range) | Head length today → Jai | Lines >100 cols today → Jai |
|---|---:|---:|---|---|---|---|
| Core/Math/Math.jet | 2,272 | 432 (406 fn, 25 struct, 1 trait) | 3.2 / 0.9 | 11.8 / 5.5 / 5–33 | 65.9 → 68.9 | 51 → 52 |
| Core/Data/Data.jet | 1,998 | 179 (112 fn, 43 struct, 19 enum, 5 module) | 4.3 / 1.8 | 16.4 / 5.9 / 7–29 | 51.8 → 54.8 | 12 → 12 |
| Core/Http/Server.jet | 1,442 | 122 (95 fn, 17 struct, 3 enum, 7 marker) | 3.8 / 1.6 | 16.3 / 4.6 / 7–27 | 63.5 → 66.5 | 11 → 13 |
| Core/Time/Time.jet | 1,923 | 331 (314 fn, 16 struct, 1 enum) | 3.2 / 0.9 | 14.2 / 4.6 / 7–29 | 81.7 → 84.7 (max 272 → 275) | 89 → 99 |
| **Total** | **7,635** | **1,064** | | | **+3 per head** | **163 → 176** |

How to read the offsets: today the name starts 3 columns after `fn `, 5 after `enum `, 6 after `trait `, and 7 after `struct `, `module ` and `marker `. In Jai style the name always starts at the indent (offset 0, sd 0), and the *kind* takes over the ragged position, at a spread about 3–5 times wider.

**Lines whose first token is a declared name.**

| File | Declarations + top-level bindings, today → Jai | All non-blank lines (includes local `x :: …`), today → Jai |
|---|---|---|
| Math | 0% → 100% | 11.8% → 31.5% |
| Data | 0% → 100% | 14.2% → 24.1% |
| Server | 7.6% → 100% | 21.4% → 30.7% |
| Time | 0% → 100% | 18.4% → 36.7% |

In other words, Jai style turns every declaration line and every binding line into the same left-edge shape. Today, a reader scanning the left edge sees two shapes: keyword-led declarations and name-led bindings.

### 1.2 Name-to-the-left-edge versus keyword-first: what each alignment is good for

The two alignments help different reading tasks. I separate what the research measured from what follows from it.

- **"Where is `X` defined?"** In Jai style the name sits at a fixed column, so a vertical scan finds it without skipping past keywords. Today the eye has to skip a 3–7 character keyword, and the keyword's width varies. *[INFERENCE from geometry; not measured on Jet.]* Search and navigation are a large share of developer time: Google engineers average 5 search sessions and 12 queries per workday, many of them "where is this code" (Sadowski et al. 2015). Ko et al. (2006) measured 35% of maintenance time going to navigation mechanics.
- **"What kind of thing is `X`?" / "Show me all the types."** Today the kind is at the indent column, so the eye reads kinds straight down. In Jai style the kind sits at column 5–33 with sd ≈ 5. *[INFERENCE.]* Jet partly compensates through naming case at the left edge: `PascalCase ::` is a type, marker or trait; `snake_case ::` is a function, module or value; `ALL_CAPS ::` is a constant. Case separates types from functions, but it does not separate `fn` from `module` or `struct` from `enum`.
- **Regularity.** Jbara & Feitelson (2017) measured that repeated, regular code patterns cost the most effort on the first instances, that the cost falls off exponentially, and that later instances get skimmed. Both styles are internally regular. Jai style is regular across *all* naming forms (declarations and bindings); today's style is regular within declarations. *[INFERENCE: this favours the style with fewer distinct shapes, which is Jai style, but the study did not test declaration heads.]*
- **Familiarity dominates small visual effects.** In identifier-style studies, visual form changed speed (Sharif & Maletic 2010: underscores recognised faster with no accuracy difference; Binkley et al. 2009/2013: camelCase more accurate, and people trained in a style were faster in it). The lasting lesson is that training shifts the result. Today's head shape is the one trained by Rust, Go, Kotlin, Swift, Python and JS.

### 1.3 The hardest real examples, both ways

(`{ … }` marks an elided body.)

**(a) A long method with a function-typed parameter** (`Core/Http/Server.jet`):

```jet
// today
    fn __route(self, method: String, pattern: String, handler: ^fn(HTTPRequest) -[Net, FS.Read]> HTTPResponse HTTPError!) Never! -[Net]> {
// Jai style
    __route :: fn(self, method: String, pattern: String, handler: ^fn(HTTPRequest) -[Net, FS.Read]> HTTPResponse HTTPError!) Never! -[Net]> {
```

The name is easy to find in both. In Jai style, the line's two `fn`s (the declaring one and the parameter's type) now both sit mid-line, so "a line starting with `fn` is a function declaration" no longer works as a scanning cue.

**(b) A function-typed field next to a method.** This is the one real visual hazard:

```jet
// Jai style
__HTTPHandlerCell :: struct {
    call: fn(HTTPRequest) -[Net, FS.Read]> HTTPResponse HTTPError!      // field
}
impl HTTPHandler {
    handle :: fn(self, request: HTTPRequest) -[Net, FS.Read]> HTTPResponse HTTPError! { … }   // method
}
// today
struct __HTTPHandlerCell {
    call: fn(HTTPRequest) -[Net, FS.Read]> HTTPResponse HTTPError!
}
impl HTTPHandler {
    fn handle(self, request: HTTPRequest) -[Net, FS.Read]> HTTPResponse HTTPError! { … }
}
```

In Jai style, a field and a method differ by one character (`:` against `::`). Jet allows methods inside struct bodies (spec.md §Structs), so the two can sit on adjacent lines. A typo is never silently accepted: a field type can't take `self` or a body, and a method needs a body outside traits. Both mistakes are parse or semantic errors, which a "did you mean `::`?" fix-it can cover. The visual similarity remains, though.

**(c) A generic function with effects and failure** (`Core/Data/Data.jet`):

```jet
fn snapshot<T: [Encode, Decode]>(loader: &DataLoader<T>) -[FS, Net, DB]> DataSnapshot<T> DataError! {
snapshot :: fn<T: [Encode, Decode]>(loader: &DataLoader<T>) -[FS, Net, DB]> DataSnapshot<T> DataError! {
```

In Jai style the generic list moves to the kind (`fn<T…>`), so the name reads cleanly. Today the name is jammed against `<T`. This is a small win for Jai style. Generic heads are rare in the repo (574 of 65,169 `fn` heads), but common in Core.

**(d) A multi-head function (S83).** This is the form that fits Jai style worst:

```jet
// today
fn area(Circle(r: Float)) -> Float { 3.14 * r * r }
fn area(Rect(w: Float, h: Float)) -> Float { w * h }
// Jai style
area :: fn(Circle(r: Float)) -> Float { 3.14 * r * r }
area :: fn(Rect(w: Float, h: Float)) -> Float { w * h }
```

`name :: …` written twice reads as a redefinition, which Jet forbids everywhere else (E0105, E0118), and it looks exactly like Jai's *overloading*, which Jet rejects (D-CAP10). §6 gives the design answer. It is rare: 21 groups with 38 extra heads in the whole repo.

**(e) Bodiless signatures: a trait requirement and an `extern rust` entry:**

```jet
// today
trait Shape { fn area(self) -> Float }
extern rust "base64@0.22" { fn b64encode(s: String) String = "base64::encode" }
// Jai style
Shape :: trait { area :: fn(self) -> Float }
extern rust "base64@0.22" { b64encode :: fn(s: String) String = "base64::encode" }
```

The second line in Jai style uses `::` with two meanings (binding, and the Rust path inside the string) plus an `=`.

**(f) An inline marker** (4,048 repo `fn` heads, 6.2%, put a marker on the same line):

```jet
#Job(.Dev) fn process(job: Job) -[DB]> { … }
#Job(.Dev) process :: fn(job: Job) -[DB]> { … }     // name no longer at the left edge
#Job(.Dev)
process :: fn(job: Job) -[DB]> { … }                // with a formatter rule, at +1 line
```

**(g) The ladder (Pillar 4):**

```jet
// a local lambda...
square :: (n: Int) -> n * n
// ...grows into a function. Today:
fn square(n: Int) -> Int { n * n }          // name moves; keyword inserted before it
// Jai style:
square :: fn(n: Int) -> Int { n * n }       // name stays; `fn` and the return type are inserted
```

---

## 2. Consistency: every declaration-like form, both ways

Forms come from `JET_KEYWORD_LIST` (`Compiler/JetFoundation/Data/Syntax/PackageFiles.jet:488`), the contextual keywords in `EffectsSurface.jet`, the dispatch in `parser_parse_item_or_import` (`Compiler/JetParser/Source/Parser/Items.jet:201–325`) and its `*_ahead` predicates, and `Docs/spec/reference/syntax-surface.jet`. Repo counts come from the census of 17,510 `.jet` files.

| # | Form | Today | Jai style | Fit |
|---|---|---|---|---|
| 1 | constant | `MAX_ROWS :: 100` | same | already name-first |
| 2 | compile-time value | `TABLE :: comptime { … }` | same | already |
| 3 | local / module binding | `x :: f()`, `count := 0` | same | already |
| 4 | named lambda | `square :: (n: Int) -> n * n` | same | already |
| 5 | named loop | `outer :: loop { … }` | same | already |
| 6 | marked binding | `#Persist counter := 0`, `#Track speed :: compute_speed()` | same | already |
| 7 | distinct type | `Usd :: distinct Decimal` | same | already (D-DIST1) |
| 8 | function (65,169 heads) | `fn area(c: Circle) -> Float { … }` | `area :: fn(c: Circle) -> Float { … }` | fits |
| 9 | generic function | `fn json<T: Encode>(…)` | `json :: fn<T: Encode>(…)` | fits |
| 10 | method in struct/impl | `fn len(self) -> Int { … }` | `len :: fn(self) -> Int { … }` | fits |
| 11 | extension method (D-EXTMETH1; 11 heads) | `fn Text.shout(self) -> Text` | `Text.shout :: fn(self) -> Text` | **misfit**: first dotted name left of `::` |
| 12 | multi-head function (S83; 21 groups) | `fn area(Circle(r: Float))…` ×2 | `area :: fn(Circle(r: Float))…` ×2 | **misfit**: repeated binding |
| 13 | trait requirement (≈328 bodiless heads incl. extern) | `fn area(self) -> Float` | `area :: fn(self) -> Float` | **misfit**: `::` binds no body |
| 14 | comptime function (5) | `comptime fn table()` | `table :: comptime fn()` | fits; pairs with row 2 |
| 15 | entry point | `fn run() { … }` | `run :: fn() { … }` | fits |
| 16 | property test | `#Test fn commutes(a: Int, b: Int)` | `#Test commutes :: fn(a: Int, b: Int)` | fits (marker rule applies) |
| 17 | spliced name (D-NAME-SPLICE1) | `fn $method(self)` | `$method :: fn(self)` | fits |
| 18 | struct (7,150) | `struct Canvas { … }` | `Canvas :: struct { … }` | fits |
| 19 | generic struct | `struct DataSnapshot<T> { … }` | `DataSnapshot :: struct<T> { … }` | fits |
| 20 | enum (1,883) | `enum Shape { … }` | `Shape :: enum { … }` | fits |
| 21 | trait (181) | `trait Shape { … }` | `Shape :: trait { … }` | fits |
| 22 | tag (37) | `tag Reviewed`, `tag Internal { }` | `Reviewed :: tag`, `Internal :: tag { }` | fits |
| 23 | inline module (1,157 incl. file modules) | `module csv { … }` | `csv :: module { … }` | fits |
| 24 | file-module declaration | `module arrow` | `arrow :: module` | fits; bodiless |
| 25 | generic module | `module cache<K: Hash>(capacity: Int) { … }` | `cache :: module<K: Hash>(capacity: Int) { … }` | fits |
| 26 | module application | `module int_cache :: cache<Int>(64)` | `int_cache :: module cache<Int>(64)` | **hybrid today** → fits |
| 27 | type alias (42) | `alias Id :: UserId`, `alias P<T> :: Point` | `Id :: alias UserId`, `P :: alias<T> Point` | **hybrid today** → fits; now parallel with row 7 |
| 28 | effect (60) | `effect Log.Audit` | `Log.Audit :: effect` | **misfit**: dotted name |
| 29 | marker (332) | `marker Get(path: String, $sites: [.Method])` | `Get :: marker(path: String, $sites: [.Method])` | fits |
| 30 | marker group (D-MARKER-GROUP1) | `marker Api = #[Get, Post]` | `Api :: marker #[Get, Post]` | **hybrid today (`=`)** → fits |
| 31 | fact (115) | `fact Name …` | `Name :: fact …` | fits |
| 32 | policy (8) | `policy audit(topic: String) { wrap(call) { … } }` | `audit :: policy(topic: String) { … }` | fits |
| 33 | protocol (7) | `protocol Payment { client: Hello(version: Int) … }` | `Payment :: protocol { … }` (inner message lines unchanged) | fits |
| 34 | `extern rust` entry (S50) | `fn b64encode(s: String) String = "base64::encode"` | `b64encode :: fn(s: String) String = "base64::encode"` | **misfit**: `::` then `=` |
| 35 | impl | `impl Canvas.Describe { … }` | unchanged | non-binding: attaches to existing names |
| 36 | error conversion | `impl DiskError -> StoreError { fn convert(…) }` | `impl` unchanged, inner `convert :: fn(…)` | non-binding |
| 37 | derive | `derive T.FieldNames { … }` | unchanged | non-binding |
| 38 | migration | `migration UserRecord { rename name -> display_name }` | unchanged | non-binding |
| 39 | state section | `state { Closed, Open }` (inside a struct) | unchanged | section, not a binding |
| 40 | unit family | `#UnitFamily(Currency) { usd, eur }` | unchanged | marker-led; names are inside the arguments |
| 41 | test block | `#Test("reverse twice") { … }` | unchanged | names nothing |
| 42 | `extern rust` block | `extern rust "crate@ver" { … }` | unchanged | names nothing |
| 43 | use / import | `use core.time as time`, `use c.[raylib as rl]` | unchanged | **misfit**: binds `time`, but stays keyword-first |

**Tally.** Today 7 forms are name-first, about 25 are keyword-first, and 3 are hybrids (rows 26, 27, 30). In Jai style, 34 forms share `name :: kind` and 9 stay keyword-led (rows 35–43). Those 9 have a single principled reason: they bind no new name, or (for `use`) they *import* rather than *define*.

**Misfits and what each becomes.**

- **Extension methods and effects with dotted names (rows 11, 28).** Allow a dotted *declaring path* left of `::`, but only at item scope. `::` never assigns to a member anywhere, so `Text.shout :: fn(self)` can't be mistaken for mutation. This is a parser change (scan `IDENT (. IDENT)* ::`).
- **Multi-head functions (row 12).** These become *contiguous clauses* of one declaration (§6.2).
- **Bodiless requirements (rows 13, 34).** The rule is the same as today's, restated: an `fn` head without a body is legal only inside `trait` and `extern` bodies. An `extern` entry fills the body slot with `= "path"`. That follows the historical principle D-CHOOSE-FNBODY1: "Defining a named thing uses `::`; filling a slot inside a definition uses `=`."
- **`use … as name` (row 43).** Keep it. Jai spells imports `name :: #import "…"`, but Jet's `use` has group, rename and package forms (`use x.[A, B]`, `use "limits"`) that don't fit a single binding. It also reads as a beginner sentence. The rule becomes: "*define* with `::`, *import* with `use`".
- **Module application (row 26).** Today it is the only form with both an introducer keyword and `::`. Jai style resolves it.

Keyword-first can't produce a single rule the other way round. That would need `let`/`const` in front of `x :: 1`, and S2/D-BIND1 ratified the keyword-less binding. **Only name-first gives Jet one declaration grammar.** This is the most objective consistency fact in this report.

---

## 3. Elegance and the language model

### 3.1 What `::` means today

The lexical ledger (syntax-decisions.md lines 21–22, D-BIND1) says: `::` is an immutable binding and `:=` is a mutable one. S2 retired the typed form `name: Type :: expr` (E0393). Outside bindings, `::` appears only inside `extern rust` path strings (S28 note, line 2237). In Jai style, `::` would mean **"introduces a name that can never be rebound"**. That is true of functions, types, modules and markers. It is a fair reading, not a stretch.

### 3.2 What name-first does *not* mean in Jet

In Jai, Odin and Zig, `Name :: struct {}` is literally a compile-time constant whose value is a type. `#run`/comptime can compute it, pass it around and compare it. Jet deliberately does none of this:

- **Types are not values.** S26: "comptime never creates, parameterizes, or selects a type, and never affects dispatch". D-METAMUTATE1 rejects Jai-style AST mutation. So `T :: comptime { … }` can never produce a type, and `Shape :: if fast -> A else B` is meaningless.
- **Named functions are not generally values.** A named function becomes a value only when every parameter has read access. Parameters with `&`/`^` keep it direct-call-only (spec.md 760–763, S47). Function values are called with `.call(…)` (D-CALLVALUE1=B).
- **Lambdas and functions are different kinds.** Lambdas never declare a return or error type (D-LAMBDA-IFACE2=A). Self-recursion through a lambda binding is E0804. Named functions can recurse, have multiple heads, and carry markers and effect ceilings.

So in Jet, Jai style would be a **spelling unification, not a semantic one**. I8 allows exactly this ("Spelling or organization may flex only where that helps users"), but only if the spelling doesn't *teach a wrong model*. Keeping the `fn` keyword after `::` does most of that work: `square :: (n) -> …` (value) and `square :: fn(n: Int) -> Int {…}` (declaration) stay distinct at a glance.

### 3.3 Aliasing (`f :: g`)

- **Today.** Inside a body, `f :: g` binds a *function value*: it is allowed only if `g` has read-only parameters, and the value-call rules apply. At file scope, S14 ("no aliases, ever") and D-CAP10 (one definition per name) forbid a second name for an item.
- **In Jai style**, `f :: g` sits right next to `g :: fn(…) {…}` and invites the reading "`f` is now another name for the function". **Design answer:** a declaration head is `name :: <kind keyword>`, and nothing else declares. `f :: g` stays an ordinary immutable *value* binding with today's value rules. At file scope it is rejected with a teaching diagnostic ("a file-scope `::` without a kind keyword binds a module value; Jet has no item aliases (S14); call `g` directly"). Jai itself does this with `#this`/constants, and Odin keeps explicit procedure groups (`foo :: proc{ a, b }`). Neither treats `f :: g` as a declaration alias.

### 3.4 Comptime and metaprogramming

- `NAME :: comptime { … }` and `f :: comptime fn() { … }` read as one family under Jai style. That is a small consistency win. `:: comptime` stays greppable, which the owner ruling (syntax-decisions.md 100–109) requires.
- `comptime loop` declaration generation (D-STRUCT-ONCE1, D-NAME-SPLICE1) becomes `$method :: fn(self) { … }`. The spliced name sits at the left edge, where generated declarations are easiest to see.
- Jai's headline benefit, where types and procedures are compile-time constants that metaprograms manipulate, **does not transfer**, because S26 forbids it permanently. The elegance argument in Jet is therefore about reading and editing, not about metaprogramming power.

### 3.5 Overloading and shadowing

- Jai uses repeated `name ::` for overloading. Jet rejects overloading (D-CAP10, spec.md 537). Jet's only legal repeat is multi-head clauses (§6.2).
- Shadowing: no same-scope shadowing (E0118). A non-callable local shadows a file function (E0803). With `::` on both sides (`area :: 3` in a body, `area :: fn…` at file scope), shadowing becomes visually *more* obvious. That is a mild benefit. *[INFERENCE.]*

---

## 4. Prior art and research

### 4.1 Languages

| Language | Function head | Type head | Notes |
|---|---|---|---|
| **Jai** | `sum :: (x: float, y: float) -> float { … }`, no keyword | `Vector3 :: struct { … }` | Procedures are constants; repeated `::` overloads; local procedures cannot be forward-referenced. Sources: JaiPrimer (unofficial) https://github.com/BSVino/JaiPrimer/blob/master/JaiPrimer.md; *The Way to Jai* §5, §17 https://github.com/Ivo-Balbaert/The_Way_to_Jai |
| **Odin** | `fibonacci :: proc(n: int) -> int { … }` | `Foo :: struct { … }` | FAQ: `::` is "constant value declarations", `Z :: proc() {}`. Explicit overload groups `foo :: proc{ a, b }`. "Designed to be very easy to parse without a symbol table." Attributes `@(…)` go on the line before the name. https://odin-lang.org/docs/faq/, https://odin-lang.org/docs/overview/ |
| **Zig** | `fn add(a: i32) i32 { … }` (keyword-first) | `const Point = struct { … };` (name-first) | **Proposal #1717** to make functions `const f = fn(…) {…}` was *accepted* in 2019 and *rejected* in 2023: "functions have symbol names… the syntax inconsistency appropriately models reality". Also cited: an `export` ABI footgun and avoiding function pointers. https://github.com/ziglang/zig/issues/1717#issuecomment-1627790251 |
| **Go** | `func Name(…) T` | `type Point struct { … }` | Pike: name first because "it's always obvious which name is being declared". That is argument, not measurement. https://go.dev/blog/declaration-syntax |
| **Rust** | `fn name(…) -> T` | `struct P {}`, `const N: u32 = 1;` | `::` is the path separator, which Jet uses only inside `extern rust` strings. https://doc.rust-lang.org/reference/items/functions.html |
| **Kotlin / Swift** | `fun name(…)` / `func name(…)` | `class P` / `struct P` | Keyword-first; bindings `val`/`let`. https://kotlinlang.org/docs/functions.html, https://docs.swift.org/swift-book/documentation/the-swift-programming-language/functions/ |
| **C3** | `fn int add(int a, int b)` | `struct Foo { … }` | Keyword-first with a C return type. https://c3-lang.org/ |
| **Carbon** | `fn Name(…) -> T` | `class Name {}` | p000438: "Adding a keyword makes it easy for developers to visually identify functions"; "consensus that some keyword should be used in order to simplify parsing". p000618 picked `name: Type` with "lack of compelling data". https://github.com/carbon-language/carbon-lang/blob/trunk/proposals/p000438-functions.md, …/p000618-var-ordering.md |
| **ML / OCaml** | `let area = function Circle r -> … \| Rect (w, h) -> …` | `type shape = …` | Keyword + name-first; multiple clauses grouped under *one* binding. https://ocaml.org/docs |
| **Haskell** | `area :: Shape -> Float` then equations `area (Circle r) = …` | `data Shape = …` | `::` means "has type". All clauses of a function must be **contiguous** (Haskell 2010 report §4.4.3). https://www.haskell.org/onlinereport/haskell2010/haskellch4.html |
| **JS / TS** | `function f() {}` *or* `const f = () => …` | `type P = { … }`, `class P {}` | Style guides disagree. Airbnb 7.1: "Use named function expressions instead of function declarations" (because declarations are hoisted). Google TS: "Prefer function declarations for named functions". ESLint `func-style` exists to enforce either. https://github.com/airbnb/javascript#functions--declarations, https://google.github.io/styleguide/tsguide.html |
| **Python** | `def f(x):` | `class P:` | PEP 8: "Always use a def statement instead of an assignment statement that binds a lambda expression directly to an identifier", because the function gets a real name. https://peps.python.org/pep-0008/ |

**Patterns.**

1. Name-first is common for **types**: Zig, Go, TS, OCaml, Haskell, and Jet's own `distinct`/`alias`.
2. For **functions**, only Jai and Odin, plus the JS `const f = …` idiom, are name-first. Every mainstream language puts a keyword first.
3. Two designers who tried both came down on opposite sides. Odin switched to a Go-like keyword style twice, for about two weeks each time, and came back to name-first (gingerBill 2018, https://www.gingerbill.org/article/2018/03/12/on-the-aesthetics-of-the-syntax-of-declarations/). Zig accepted function-as-binding and later reverted it. Both decisions rested on taste and semantics, not measurement.
4. gingerBill names the bodiless problem: in name-first style, a body-less foreign procedure can't be told apart from a procedure *type*. That is why Odin needs `---` and `foreign` blocks. **Jet doesn't have this problem**, because Jet never binds types with `::` (S26; `alias` names types). So `name :: fn(…)` without a body can only be a requirement.

### 4.2 Readability and eye-tracking research

**Measured results (controlled studies):**

| Study | What it measured | Relevance |
|---|---|---|
| Busjahn et al. 2015, ICPC, doi:10.1109/ICPC.2015.36 | Novices read code less linearly than prose; experts even less | Readers jump between definitions; anchors at the left edge matter for both styles |
| Jbara & Feitelson 2017, EMSE 22(3), doi:10.1007/s10664-016-9477-x; 2014 ICPC, doi:10.1145/2597008.2597140 | Regular, repeated structures: effort drops exponentially after the first instances; later ones are skimmed | Favours fewer distinct line shapes |
| Sharif & Maletic 2010, ICPC, doi:10.1109/ICPC.2010.41 | Eye tracking: underscore identifiers recognised faster; no accuracy difference | Small visual-form effects on speed exist |
| Binkley et al. 2009 ICPC, doi:10.1109/ICPC.2009.5090039; 2013 EMSE, doi:10.1007/s10664-012-9201-4 | camelCase more accurate for 135 subjects; people trained in a style were faster in it | **Familiarity changes outcomes**; this matters for any syntax switch |
| Stefik & Siebert 2013, TOCE, doi:10.1145/2534973 | Novices using Perl and Java were no more accurate than with randomly chosen keywords; Quorum, Python and Ruby did better | Keyword *choice* matters to novices; both options keep `fn` |
| Buse & Weimer 2010, TSE, doi:10.1109/TSE.2009.70 | 120 annotators; simple local features (line length, blank lines, identifiers) predict judged readability | +3 columns per head is a small negative |
| Scalabrino et al. 2018, JSEP, doi:10.1002/smr.1958 | 600+ snippets rated by 5K+ people; structural features include line length and character alignment | Alignment is a recognised readability feature, but it was not tested on declaration heads |
| Sadowski et al. 2015, FSE, doi:10.1145/2786805.2786855 | Google: 5 search sessions and 12 queries per workday; many "where is this defined" searches | Grep-ability of definitions matters (§5) |
| Ko et al. 2006, TSE, doi:10.1109/TSE.2006.116 | 35% of maintenance time spent on navigation mechanics | Same |
| Gopstein et al. 2017, FSE, doi:10.1145/3106237.3106264 | "Atoms of confusion": small syntactic patterns cause measurable misreadings | Relevant to `:` vs `::` (example 1.3b) |
| Rodeghero et al. 2014, ICSE, doi:10.1145/2568225.2568247 | Eye tracking of 10 professional Java programmers summarising methods | *[INFERENCE from memory, not verified against the full text]*: reports that fixations concentrate on method signatures, which would make the head the most-read line |

**Opinion and design rationale (not measurements):** gingerBill 2018 (name-focused is "elegant and terse"); Pike 2010 (name first is obvious); Carbon p000438 (a keyword helps people spot functions and helps parsing); Carbon p000339 (an informal "early sign" that identifier-first slows "what is the type of x?", with the stated intent to "conduct a larger study"; that study was never published); Zig #1717 (Kelley: the inconsistency models reality); PEP 8; Airbnb; Google TS; JaiPrimer ("syntax-facilitated refactoring": block → local function → global function with minimal edits).

**Gap.** No study compares `fn name(…)` with `name :: fn(…)`, or any keyword-first vs name-first function head, on speed or accuracy. The decision can't be settled by citation. It needs Jet-specific data (§8).

### 4.3 Beginner learnability

- **For keyword-first (today).** Beginners arrive from Python (`def`), JS (`function`), Go (`func`), Rust (`fn`). `fn name(` matches what they already know, and the binding training effects above suggest that matters for early speed. *[INFERENCE.]* Python is the most-taught first language and has two forms (`x = 1`, `def f():`), so two forms are clearly learnable at scale. *[Observational, not controlled.]*
- **For name-first (Jai style).** A Jet beginner learns `name :: value` on day one, because every binding uses it. Jai style makes "how do I give something a name?" a single answer that carries over to functions, types and modules. The ladder also never asks the learner to move a name. A Jet file has an executable body, so a beginner's first program needs no function at all. The first function they write comes after `::` is already familiar.
- **Both styles keep the word `fn`**, which is what Stefik & Siebert's result says matters most to novices. The D-S14 teaching table (`fun/func/def/function → fn`) works in both styles. Under Jai style it gains one entry: `fn name(` → `name :: fn(`.

---

## 5. Tooling

| Tool | Today | Jai-style change | Cost |
|---|---|---|---|
| **Parser lookahead** | The leading keyword decides (`TOKEN_KW_FN`, `STRUCT`, …; `Items.jet:201–325`) | `IDENT (. IDENT)* :: <kind keyword>` decides; otherwise it's a module binding (`parser_module_binding_ahead`, `Items.jet:2170`). `parser_distinct_ahead` (`Declarations/Types.jet:40`) already does `IDENT :: distinct`, so the pattern exists | Small: bounded lookahead, no backtracking |
| **Error recovery** | Line-start keywords are strong sync points | Line-start `IDENT ::` plus a reserved kind keyword at token 3 is equally strong. A missing `::` (`area fn(…)`) gets a fix-it | Small |
| **Formatter** | Width 100 | +3 per head; 946 heads cross 100 and re-wrap. New rule: markers on their own line before name-first heads, about 4,048 extra lines. **Do not** align `::` across consecutive heads, to keep diffs local | Small–medium |
| **LSP** | Outline, rename, go-to-def and semantic tokens come from the AST | Unchanged. Hover and signature-help render `name :: fn(…)`, so those renderings change | Small |
| **tree-sitter** (`Tools/editors/tree-sitter/grammar.js`) | `declaration($, "struct")` etc.; a generic `binding` node `name ::` already exists | Add name-before-kind rules with precedence over `binding`; move the `name` field | Small |
| **TextMate** (`jet.tmLanguage.json:250`) | Function name `(?<=\bfn\s)([a-z_]…)` | `([a-z_]\w*)(?=\s*::\s*(comptime\s+)?fn\b)`, a lookahead, which TextMate supports | Trivial |
| **Zed `highlights.scm`** | Queries on declaration nodes | Same queries on the new nodes | Trivial |
| **grep** | `rg 'fn area\b'` precisely finds functions; `rg 'struct Canvas\b'` finds types | `rg '^\s*area ::'` finds *any* declaration or binding named `area` (including locals, which is lower precision). `rg '\barea :: fn\b'` is exact. `rg ':: (struct\|enum\|trait)\b'` lists all types | Equal for exact searches. Jai style wins on "find anything named X", today's style wins on "list all functions" (`^\s*fn `) |
| **AI-model familiarity** | The head shape equals Rust's | The head shape resembles Odin's | See below |
| **Codemod** | n/a | Mechanical. The sample rewrite shows changed lines = declaration lines | See below |

**AI familiarity data.** GitHub repositories by primary language (GitHub search API, queried 2026-10-29): Go 2,564,647; Kotlin 2,438,858; Swift 1,743,806; Rust 1,443,574; OCaml 43,990; Zig 39,944; **Odin 8,533; Jai 686**; C3 443; Carbon 49. Keyword-first function heads dominate model training data by roughly 2–3 orders of magnitude over `name :: proc/fn`. JS/TS `const f = (…) =>` is a very common name-first *binding* idiom, though, so models are fluent in name-first function bindings in general. Code LLMs perform worse on low-resource languages (Cassano et al., MultiPL-E, TSE 2023, doi:10.1109/TSE.2023.3267446; OOPSLA 2024, doi:10.1145/3689735). Jet is low-resource in *either* style, so the real question is which priors leak. *[INFERENCE:]* under Jai style, models will sometimes emit Rust-shaped `fn name(` heads. That failure is deterministic and machine-fixable (`jet fix`), but it costs a compile round-trip. This repo is written mostly by agents, so this cost is weighted more heavily than usual, and it is the main thing the experiment in §8 should measure.

**Codemod cost (census, worktree, excluding dot-directories).**

| Item | Count |
|---|---|
| `.jet` files / lines | 17,510 / 1,200,274 |
| Files with declaration heads | 14,753 (tests 10,608; Examples 1,372; Compiler 1,354; Jetpack 667; Tools 617; Core 131) |
| Head lines | **76,146**: fn 65,169 (7,803 indented); struct 7,150; enum 1,883; module 1,157; marker 332; trait 181; fact 115; effect 60; alias 42; tag 37; policy 8; protocol 7; comptime fn 5 |
| fn heads with an inline marker | 4,048 (6.2%) |
| Generic / effect-row / bodiless fn heads | 574 / 5,506 / 328 |
| Multi-head groups (same name, same indent, adjacent) | 21 groups, 38 extra heads |
| Head lines over 100 columns, today → after | 11,878 → 12,824 (+946) |
| Markdown files / head lines | 124 / 1,120 |
| Golden/expected files quoting heads (heuristic) | 1,208 files / 2,362 lines (of 10,855 scanned) |
| Compiler/tool string lines that spell heads | ≈2,481 (diagnostics, templates, generators) |

The compiler is self-hosted, so the migration needs Zig's planned sequence (from #1717): the parser accepts both forms for one window, `jet fmt`/`jet fix` rewrites, then the old form is removed and becomes an error with a fix-it. I8 forbids a permanent second spelling, so the window must be short and must end with a hard cutover. Add the codemod commit to `.git-blame-ignore-revs` so that 76k lines don't lose their blame history.

---

## 5b. Fit with Jet's design

**Ranked priorities (philosophy.md):**

1. **Memory/type safety**: neutral.
2. **Beginner experience**: split. Jai style gives one naming rule and a smooth ladder; today's style gives instant recognition. No data decides it. *[INFERENCE: roughly a wash, slightly in favour of Jai style once the first week is past.]*
3. **Runtime performance**: neutral.
4. **One mechanism**: favours Jai style for *spelling*. It must not imply a semantic unification that isn't there (§3.2). Keeping `fn` handles that.
5. **Implementation simplicity/compile speed**: neutral in steady state. The migration is a one-time cost.
6. **Ecosystem breadth**: slightly favours today (familiarity for AI and humans).

**Pillars.** Pillar 2 (easy to reason about, read, write) is split. **Pillar 4 (Ladders, Not Cliffs: "A code block can become a lambda, then a function") is clearly in favour of Jai style.** Pillar 7 (fewer layers) is neutral.

**I8** (AGENTS.md:131): "Each meaning has one canonical semantic mechanism. Spelling or organization may flex only where that helps users." Jai style doesn't add a mechanism; it changes a spelling. It needs owner approval because it amends ratified spellings.

**Ratified decisions a switch would amend:**

| Decision | Current text | Change |
|---|---|---|
| S1 (line 285) | Function keyword is `fn` | Keyword kept; position changes |
| D-CALLABLE-ONE1=A (8315) | `[fn name] ( params ) [Return facts] [arrow] body` | `[name :: fn] ( params ) …`, still one callable shape |
| D-SIG-AFTER1=A (8324–8350) | Examples `fn load(path: String) -[IO]> String {…}` | Examples respelled; rule unchanged |
| S83 (554–560) | Multi-head `fn area(…)` heads | Contiguous-clause rule (§6.2) |
| D-TYPEALIAS1 / D-ALIAS-OP1=B (1456–1475) | `alias X :: Y` | `X :: alias Y` |
| Generic modules / U3 (2130) | `module cache<K>(…) {…}`, `module int_cache :: cache<Int>(64)` | `cache :: module<K>(…)`, `int_cache :: module cache<Int>(64)` |
| D-EXTMETH1 (416) | `fn Type.method(self)` | `Type.method :: fn(self)` |
| S50 (3917) | `extern rust … { fn name(args) T = "path" }` | `name :: fn(args) T = "path"` |
| D-MARKER-GROUP1 (ledger line 26) | `marker Name = #[A, B]` | `Name :: marker #[A, B]` |
| D-S14 teaching table | Names Jet forms `fn`, `::`, `:=` | Add `fn name(` → `name :: fn(` |
| S28 / trait examples | `trait Shape { fn area(self) Float }` | `Shape :: trait { area :: fn(self) Float }` |

Not affected: S2/D-BIND1 (they gain support), S14 no-aliases, D-CAP10 no overloading, S26 comptime limits, S18 visibility, E0105/E0118. D-CHOOSE-FNBODY1=A (historical), "Defining a named thing uses `::`; filling a slot uses `=`", reads like an earlier statement of Jai style.

---

## 6. Benefits and detractors of Jai style

### 6.1 Benefits

1. **One declaration rule.** 34 of 43 forms share `name :: kind`. The 9 keyword-led forms have a single principled reason. Only name-first can unify, because bindings are already keyword-less (§2).
2. **It fixes today's three hybrids.** `alias X :: Y` becomes `X :: alias Y`, parallel with `X :: distinct Y`. Module application and the `=` marker group also stop being special cases.
3. **Ladder.** Going from lambda to function never moves the name (Pillar 4). JaiPrimer's "syntax-facilitated refactoring" argument also carries over.
4. **Left-edge names.** 100% of heads, and 24–37% of all lines, start with the declared name. Both "find anything named X" and visual scanning become uniform.
5. **Generics read better.** `snapshot :: fn<T: …>(…)` separates the name from its type parameters.
6. **Comptime family.** `X :: comptime {…}` and `f :: comptime fn()` line up.
7. **Low parser cost.** The lookahead pattern is already in the codebase.

### 6.2 Detractors, each with a design answer

| Detractor | Design answer |
|---|---|
| **D1. `:` field vs `::` method look alike** (1.3b) | Both typos are already errors (field types can't take `self` or a body; methods need a body outside traits). Add targeted diagnostics: "`call: fn(…) {` — a field can't have a body; did you mean `call :: fn`?" and the reverse. Highlighting colours the method name as `entity.name.function`. Optionally, a formatter rule: a blank line between a struct's fields and its methods. |
| **D2. Multi-head looks like redefinition or overloading** | Rule: *adjacent* `name :: fn` heads at the same scope, all with pattern parameters in the same positions, form **one** declaration made of ordered clauses (Haskell 2010 §4.4.3 requires contiguous clauses; OCaml groups clauses under one `let`). A non-adjacent repeat is E0105, as today. The diagnostic text says "clauses", never "overload". Repo impact: 21 groups. |
| **D3. Bodiless heads look like bindings with no value** (Odin's problem) | Jet never binds types with `::` (S26), so `name :: fn(…)` without a body can't mean "function type". Rule (same as today): bodiless heads are legal only in `trait` and `extern` bodies. `extern` entries fill the body slot with `= "path"` (D-CHOOSE-FNBODY1). |
| **D4. Inline markers push the name off the left edge** | Formatter rule: before a name-first declaration, markers go on their own line (Odin's `@(…)` convention). Costs about 4,048 extra lines. Exception: one-word markers on one-line declarations may stay inline. Measure both in the experiment. |
| **D5. Kind column lost** (sd 4.6–5.9) | Case conventions recover type vs function vs constant at the left edge. Highlighting colours the kind keyword. The outline/LSP lists kinds. Residual cost: scanning "all structs" by eye. Test it in the experiment. |
| **D6. +3 columns per head; 946 heads over 100** | Re-wrap with the formatter; parameter lists already wrap. Permanent cost: 3 characters on 6% of lines. |
| **D7. Suggests functions/types are values** (§3.2) | Keep the kind keyword (no Jai-style keyword-less `name :: (…) {…}` for functions). Never allow `fn(…) {…}` literals outside a declaration head; lambdas stay `(…) -> …`. Docs sentence: "`::` names a thing that can't be rebound. The word after it says what kind of thing." |
| **D8. `f :: g` reads as an alias** | §3.3: a declaration needs a kind keyword. `f :: g` stays a value binding with value rules, and at file scope it gets a teaching diagnostic citing S14. |
| **D9. Dotted names left of `::`** (`Text.shout :: fn`, `Log.Audit :: effect`) | Allowed at item scope only. `::` never mutates, and the dotted path names the owner. Parser scans a bounded path. |
| **D10. Unfamiliar to beginners and AI models** | Keep `fn`. Add `fn name(` to the D-S14 teaching table with a machine-applicable fix. Measure the residual cost (§8). |
| **D11. Zig's reversal** | Kelley's reasons were symbol names, `export` ABI and discouraging function pointers. In Jet the declaration stays a declaration (D7), so symbol names are kept, and there is no `export`-a-pointer path. Zig's real concern, *encouraging lambdas*, doesn't apply because lambdas keep their own syntax. |
| **D12. Migration cost and churn** | One mechanical commit per tree after a short dual-accept window. Blame-ignore the commit. Regenerate goldens. Amend the 11 decisions in one ratification. |
| **D13. Two `::` meanings on an extern line** (`b64encode :: fn(…) = "base64::encode"`) | It's a string, already lexically separate. Accept it. |

---

## 7. Options

All options use the same short program. `{ … }` marks an elided body.

### Option A: keep today's style (optionally fix the hybrids)

```jet
MAX_SHAPES :: 64

enum Shape {
    Circle(r: Float)
    Rect(w: Float, h: Float)
}

trait Describe {
    fn describe(self) -> String
}

struct Canvas {
    shapes: [Shape]
    on_draw: fn(Shape) -[]> Unit

    fn count(self) -> Int { self.shapes.len() }
}

impl Canvas.Describe {
    fn describe(self) -> String { "canvas" }
}

fn area(Circle(r: Float)) -> Float { 3.14 * r * r }
fn area(Rect(w: Float, h: Float)) -> Float { w * h }

#Inline(Always)
fn double(x: Float) -> Float { x * 2.0 }

fn load<T: Decode>(path: String) -[FS]> T LoadError! { … }

fn run() {
    scale :: (x: Float) -> x * 2.0
    print(scale(area(Shape.Circle(r: 1.0))))
}
```

- **Trade-offs.** No migration, maximum familiarity, kinds aligned at the indent. It keeps two naming grammars and the ladder cliff (`scale ::` → `fn scale`).
- **Edge cases.** The three hybrids remain unless respelled keyword-first. That is awkward: `distinct Usd = Decimal` would break `::` = immutable binding, which shows that keyword-first can't be made fully uniform.
- **Beginner path.** Learn `x :: 1`, then separately learn `fn name(…)`. Promoting a lambda means rewriting its head.
- **Expert path.** Keyword-led scanning; `rg '^\s*fn '` lists functions.

### Option B: Jai style, engineered (recommended, pending the experiment)

```jet
MAX_SHAPES :: 64

Shape :: enum {
    Circle(r: Float)
    Rect(w: Float, h: Float)
}

Describe :: trait {
    describe :: fn(self) -> String
}

Canvas :: struct {
    shapes: [Shape]
    on_draw: fn(Shape) -[]> Unit

    count :: fn(self) -> Int { self.shapes.len() }
}

impl Canvas.Describe {
    describe :: fn(self) -> String { "canvas" }
}

area :: fn(Circle(r: Float)) -> Float { 3.14 * r * r }      // clause 1 of one declaration
area :: fn(Rect(w: Float, h: Float)) -> Float { w * h }     // clause 2 (must be adjacent)

#Inline(Always)
double :: fn(x: Float) -> Float { x * 2.0 }

load :: fn<T: Decode>(path: String) -[FS]> T LoadError! { … }

run :: fn() {
    scale :: (x: Float) -> x * 2.0
    print(scale(area(Shape.Circle(r: 1.0))))
}
```

The engineering rules on top of the mechanical rewrite:

1. A declaration is `name :: <kind keyword> …`; the kind keyword is mandatory.
2. Adjacent `fn` heads with the same name are clauses of one declaration (D2).
3. Bodiless heads only in `trait`/`extern` bodies; `extern` fills the body with `= "path"` (D3).
4. Markers go on their own line before a name-first head (D4).
5. `f :: g` is never a declaration (D8).
6. Dotted declaring paths only at item scope (D9).

Non-binding forms (`impl`, `derive`, `migration`, `state`, `#Test("…") {}`, `#UnitFamily`, `extern` blocks, `use`) stay keyword-led.

- **Trade-offs.** One rule, the ladder, left-edge names. Costs: +3 columns, the kind column, familiarity, and a large one-time migration.
- **Edge cases.** Multi-head clauses, `:`/`::` adjacency, dotted names, generic alias (`P :: alias<T> Point`), file modules (`arrow :: module`).
- **Beginner path.** Learn `x :: 1` on day one, then "functions and types are named the same way, with a word saying what kind". The lambda → function step only inserts `fn`, the return type and braces.
- **Expert path.** `rg '\bname :: '` finds any named thing; `rg ':: fn\b'` lists functions; the outline gives kinds.

### Option C: split, with types name-first and functions keyword-first (the Zig/Go/TS pattern)

```jet
MAX_SHAPES :: 64

Shape :: enum {
    Circle(r: Float)
    Rect(w: Float, h: Float)
}

Describe :: trait {
    fn describe(self) -> String
}

Canvas :: struct {
    shapes: [Shape]
    on_draw: fn(Shape) -[]> Unit

    fn count(self) -> Int { self.shapes.len() }
}

impl Canvas.Describe {
    fn describe(self) -> String { "canvas" }
}

fn area(Circle(r: Float)) -> Float { 3.14 * r * r }
fn area(Rect(w: Float, h: Float)) -> Float { w * h }

#Inline(Always)
fn double(x: Float) -> Float { x * 2.0 }

fn load<T: Decode>(path: String) -[FS]> T LoadError! { … }

fn run() {
    scale :: (x: Float) -> x * 2.0
    print(scale(area(Shape.Circle(r: 1.0))))
}
```

- **Trade-offs.** It follows the strongest prior-art pattern (types are name-first in Zig, Go, TS and ML; functions keep a keyword, matching Kelley's "symbol names" argument). It fixes `alias`, the marker group and module application. Migration is about 11k heads. But it still has two rules, with the line moved to "types vs functions". It gives no ladder win, and functions are 86% of heads, so most lines don't change shape.
- **Edge cases.** Modules: name-first (`csv :: module`) or keyword-first? Either answer is arbitrary. Markers and effects: same question.
- **Beginner path.** Two rules, split on a semantic line the beginner must learn first.
- **Expert path.** `rg ':: (struct|enum|trait)'` for types, `rg '^\s*fn '` for functions.

I considered and dropped a fourth option, the **mechanical Jai rewrite** (the sample as it stands, without B's rules). It leaves D2, D3, D4 and D8 unanswered. B is that option plus the six rules.

---

## 8. Recommendation

**Adopt Option B (engineered Jai style), gated on the experiment below. Confidence: moderate, about 60/40.**

Why B over A:

1. It is the only way to one declaration grammar, given Jet's keyword-less bindings.
2. It removes the lambda → function cliff named in Pillar 4.
3. Its steady-state costs are small and measured: +3 columns, 13 more over-width lines per 7.6k lines, and a ragged kind column that naming case partly offsets.
4. Its biggest conceptual risk, implying that functions and types are values, is contained by keeping the kind keyword and by D7/D8.

Why the confidence is only moderate:

1. No study measures the core readability trade-off.
2. Familiarity is a measured effect, and it currently favours today's head.
3. Zig, a closely related language, reversed exactly this change after accepting it.
4. Most code here is written by AI models whose priors are Rust-shaped.

**What would change my mind (towards A):**

- After a one-page primer, models emit wrong-style (`fn name(`) heads in more than about 5% of generated declarations, or Jai-style edits pass compile less often than today's style by more than about 3 percentage points.
- People are more than about 10% slower, or less accurate, at "what kind is X / list the types" tasks, with no matching gain on "find X".
- The six rules in B can't each be stated in one sentence in the spec, or multi-head clauses need more than one new diagnostic.
- The owner rates the D1 (`:` vs `::`) hazard as unacceptable after reading real struct bodies in both styles.

**What would raise my confidence (towards B):** the "find X" and "promote a lambda" tasks are clearly faster in Jai style, and models adapt within the primer.

**Cheapest experiment that could settle it (1–3 days, mostly compute):**

1. **AI edit test (the main one).** Take the four sample pairs. Write about 40 edit tasks: add a method, add a generic effectful function, write a multi-head function, write a trait plus impl, promote a lambda, add a marker. Run 2–3 models with the same one-page style primer in each style. Measure:
   - parse-valid rate (style-checked by regex, then mechanically converted back and compiled with today's compiler);
   - wrong-style head rate;
   - task success.

   No parser work is needed, because the mechanical rewrite already exists.
2. **Human reading test (small).** 6–10 people (owner and contributors), within-subjects and counterbalanced, on printed sample excerpts with and without highlighting. Three task types, 10 items each: "find the definition of X", "what kind is X", "list the types in this region". Record time and accuracy.

**Decision rule:** adopt B if (1) stays within the thresholds above and (2) shows no kind-task loss larger than the find-task gain. Otherwise keep A and respell only what can be respelled without breaking `::`. In that case, `alias X :: Y` probably becomes `X :: alias Y` either way, because it then matches `X :: distinct Y`.

If B is adopted, the rollout:

1. Ratify the six rules and the 11 decision amendments in one entry.
2. Parser accepts both forms for one short window.
3. `jet fix` codemod per tree (76k heads), plus docs (1,120 heads), goldens (≈2.4k lines) and tool strings (≈2.5k lines).
4. Update tree-sitter, TextMate and Zed.
5. Old form becomes a hard error with a fix-it; add the codemod commits to `.git-blame-ignore-revs`.

---

### Appendix: sources

- gingerBill, "On the Aesthetics of the Syntax of Declarations" (2018): https://www.gingerbill.org/article/2018/03/12/on-the-aesthetics-of-the-syntax-of-declarations/
- Odin FAQ and overview: https://odin-lang.org/docs/faq/, https://odin-lang.org/docs/overview/
- JaiPrimer (unofficial): https://github.com/BSVino/JaiPrimer/blob/master/JaiPrimer.md. *The Way to Jai*: https://github.com/Ivo-Balbaert/The_Way_to_Jai
- Zig #1717 and the rejection comment: https://github.com/ziglang/zig/issues/1717, https://github.com/ziglang/zig/issues/1717#issuecomment-1627790251
- Rob Pike, "Go's Declaration Syntax": https://go.dev/blog/declaration-syntax
- Carbon: https://github.com/carbon-language/carbon-lang/blob/trunk/docs/design/README.md, proposals p000339, p000438, p000618 under https://github.com/carbon-language/carbon-lang/tree/trunk/proposals
- PEP 8: https://peps.python.org/pep-0008/. Airbnb JS 7.1: https://github.com/airbnb/javascript#functions--declarations. Google TS style: https://google.github.io/styleguide/tsguide.html
- Haskell 2010 report ch. 4: https://www.haskell.org/onlinereport/haskell2010/haskellch4.html
- Language references: https://doc.rust-lang.org/reference/items/functions.html, https://kotlinlang.org/docs/functions.html, https://docs.swift.org/swift-book/documentation/the-swift-programming-language/functions/, https://c3-lang.org/, https://ocaml.org/docs, https://ziglang.org/documentation/master/
- Studies: see the DOIs in §4.2 and §5. Abstracts checked via OpenAlex and Crossref.
- GitHub repository counts: `https://api.github.com/search/repositories?q=language:<lang>`, queried 2026-10-29.
