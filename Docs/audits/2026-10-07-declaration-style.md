# Declaration form: keyword-first, name-first everywhere, or types-by-binding?

Author: DeclStyle1 · 2026-10-07 · Jet `277033b7c` · read-only research; nothing in the repo or Tower was written.
Measurement code and raw numbers: `measure.mjs`, `metrics.json`, `migration.mjs`, `migration.json` beside this file. Corpora: `corpora/` (Odin `827b5b4`, Go `b756cce`, shallow sparse clones), Zig 0.16 std from `/nix/store/…-zig-0.16.0/lib/zig/std`, Rust crates from `~/.cargo/registry` (syn, serde, hashbrown, regex-automata, tokio, clap_builder, rayon-core, crossbeam-channel, anyhow, toml_edit).

## 0. Answer first

**Do not adopt the Odin/Jai form everywhere. Adopt it for every type, and keep the keyword for code.** Concretely:

- Every name that stands for a type or a fixed value is a binding: `Point :: struct { … }`, `Shape :: enum { … }`, `Named :: trait { … }`, `Usd :: distinct Decimal`, `Rows :: [[String]]`, `IntPair :: Pair(Int)`, `LIMIT :: 10`, `clamp :: core.math.clamp`.
- Everything that is code, or a declaration that binds no value, keeps its keyword: `fn`, `prep fn` (including type recipes), methods, trait slots, `impl`, `module`, `marker`, `fact`, `effect`, `migration`, `derive`, `extern`, `#Test`, `use`.

This is Zig's split (`const Point = struct {…}` but `fn area() …`). Five measured or ratified facts decide it; section 6 has the full argument.

1. **Ratified law already makes `Point :: struct { … }` grammatical.** D-GENERIC-TYPEFN1=A (2026-10-02) made `struct { … }` an expression (recipe bodies return it), and D-ALIAS-BIND2 (open, rec A) makes `Name :: <type>` legal. Keeping `struct Point { … }` as well leaves two spellings for one thing. Functions have no expression form (D-SIG-AFTER1=A and D-CALLABLE-ONE1 refuse `fn(…) { }` literals), so `fn name` is already the only spelling, and `name :: fn` would need a new pseudo-expression or a second anonymous-function shape.
2. **Search: type names never collide with locals; function names do.** On a name-first rewrite of all 7,468 tracked `.jet` files, the one query `Name ::` finds any type of any kind at **98.6 % precision**, but finds a *function* at **56 %**. The cause: `name :: expr` is Jet's everyday local binding (49,756 lines, 64 % of local bindings; Odin's local `::` share is 18 %). PascalCase local `::` lines: **0** in the corpus.
3. **Functions are named machine artifacts.** Zig accepted "function definitions as const bindings" in 2019 and rejected it in 2023, because functions carry symbol names into stack traces, profilers and debuggers ([ziglang/zig#1717](https://github.com/ziglang/zig/issues/1717#issuecomment-1627592395)). The owner ratified `fn name(…) -> T { }` over `name :: fn(…)` on 2026-09-18 (D-SIG-AFTER1=A, option C rejected).
4. **Name-first functions import Odin's known ambiguity.** A bodiless signature is indistinguishable from a function type; Odin needs `---` and `#type` to tell them apart ([Odin overview](https://odin-lang.org/docs/overview/)). In Jet this hits 99 trait slots and 23 extern functions. In struct bodies, `area: fn(Shape) -> Float` (a field) and `area :: fn(self) -> Float { … }` (a method) would differ by one colon, across 3,249 methods sitting beside 14,132 fields.
5. **Recovery and reading stay anchored.** Today every item starts with a reserved word, and the parser's recovery syncs on them (`crates/jet-parser/src/Parser/mod.rs:909-925`). Under name-first everywhere, 45 % of lines that start `ident ::` would be declarations and 55 % locals (41,334 vs 49,756), so no token-level sync point exists. Under the split, `fn` stays and `PascalName ::` is a clean new sync point.

Two real costs of the recommendation, with mitigations, are in §6.3.

---

## 1. What Jet does today, per declaration kind

Sources: AST `crates/jet-foundation/src/AST/items.rs:9-85` (the `Item` enum), parser dispatch `crates/jet-parser/src/Parser/Modules.rs:1086-1228` and `Parser/Items/visibility_items.rs:3-90`, keywords `crates/jet-foundation/src/Syntax/core_surface.rs:57,418-426,471-487`, recovery `Parser/mod.rs:909-925`, spec `Docs/spec/syntax-decisions.md`. Counts are statement-start lines from the scope-aware scanner over all 7,468 tracked `.jet` files (§2.1 explains the scanner).

| Kind | Today (law) | Count | A: types by binding (rec.) | B: keyword-first | C: Odin/Jai everywhere |
|---|---|---:|---|---|---|
| Function | `fn name(p: T) -> R { }` (S1, D-SIG-AFTER1=A, spec L228-251) | 29,522 | unchanged | unchanged | `name :: fn(p: T) -> R { }` |
| Method in type/impl body | `fn area(self) -> Float { }` (S27, L356) | 3,249 | unchanged | unchanged | `area :: fn(self) -> Float { }` |
| External method | `fn Type.method(self)` (D-EXTMETH1) | 11 | unchanged | unchanged | `Type.method :: fn(self) { }` |
| Trait slot (no body) | `fn label(self) -> String` | 99 | unchanged | unchanged | `label :: fn(self) -> String` + a "no body" mark (Odin `---`) |
| Build-time function | `prep fn from_literal(…)` (D-PREP-FN1=A) | 5 | unchanged | unchanged | `from_literal :: prep fn(…)` |
| Type recipe (generic type) | `prep fn Pair(T: Type) -> Type { struct { … } }` (D-GENERIC-TYPEFN1=A; replaces `struct Pair<T>`) | 0 yet | unchanged; use `IntPair :: Pair(Int)` | unchanged | `Pair :: prep fn(T: Type) -> Type { struct { … } }` |
| Struct | `struct Point { x: Int }` (M2, S29) | 4,073 | `Point :: struct { x: Int }` | unchanged | `Point :: struct { x: Int }` |
| Enum | `enum Shape { … }` (S30, L1019) | 1,276 | `Shape :: enum { … }` | unchanged | `Shape :: enum { … }` |
| Trait | `trait Named { … }` (S28, L1967) | 76 | `Named :: trait { … }` | unchanged | `Named :: trait { … }` |
| Associated type | trait `type Elem`; impl `type Elem = Int` (D-LIB2) | — | slot `type Elem` stays; impl `Elem :: Int` | unchanged | as A |
| Distinct type | `Usd :: distinct Decimal` (D-DIST1, L1032) — already name-first | 47 | unchanged | unchanged | unchanged |
| Type alias | `alias Name<T> :: …`, generic-only (D-TYPEALIAS1 L1247, D-ALIAS-OP1=B); D-ALIAS-BIND2 open, rec `Rows :: [[String]]` | 7 | `Rows :: [[String]]` | `Rows :: [[String]]` (if D-ALIAS-BIND2=A) | `Rows :: [[String]]` |
| Unit family | `#UnitFamily(Currency) { usd, eur }` (D-QUAL3) | 68 | unchanged (marker form) | unchanged | unchanged |
| Constant | `LIMIT :: 10`, `ROW :: prep { … }` (S2, D-PREP-SURFACE2=A, D-CORE-PLAINCONST1) | 2,235 | unchanged | unchanged | unchanged |
| Re-export | `pub use …` (D-MOD4); D-VIS-REEXPORT1 open, rec `clamp :: core.math.clamp` | 7 | `clamp :: core.math.clamp` | same | same |
| Impl block | `impl Type { }`, `impl Type.Trait { }`, `impl A -> B` (S27, S28, D-ERR-CONV) | 474 | unchanged (binds nothing) | unchanged | unchanged |
| Inline module | `module arith { … }` (U3, D-MOD2) | 319 | unchanged | unchanged | `arith :: module { … }` |
| File module decl | `module math` | 86 | unchanged | unchanged | `math :: module` |
| Generic module / instance | `module cache<K>(capacity: Int) { }`, `module int_cache :: cache<Int>(32)` (D-GENMOD1/2; respelled by D-GENERIC-TYPEFN1 to `module cache(prep K: Hash, capacity: Int)`, `module cache64 :: cache(String, 64)`) | 30 aliases | unchanged | unchanged | `cache :: module(prep K: Hash, capacity: Int) { }`, `int_cache :: cache(Int, 32)` |
| Private module | `priv module name { }` (card #4723 owner directive) | — | unchanged | unchanged | `name :: priv module { }` or `priv name :: module { }` (new choice) |
| Marker (absorbs tag, policy) | `marker CardNumber { deny: [Log] }`, `marker Audit(topic: String) { wrap(call) {…} }` (D-MARKER-DECL1=A) | 126 (+18 tag) | unchanged | unchanged | `CardNumber :: marker { … }`, `Audit :: marker(topic: String) { … }` |
| Fact row | `fact Exactness($holds: …)` (D-FACTDECL1=A) | 49 | unchanged | unchanged | `Exactness :: fact($holds: …)` |
| Effect leaf | `effect FS.Read` (D-EFFECT-DECL1=A, L2785) | 18 | unchanged | unchanged | `FS.Read :: effect` (a dotted name on the left) |
| Protocol | `protocol Chat { … }` (D-PROTO1/2, L722) | 5 | unchanged (it generates types; it is not one) | unchanged | `Chat :: protocol { … }` |
| Migration | `migration User { rename a -> b }` (D-MIGRATE1) | 11 | unchanged | unchanged | `User :: migration { }` would collide with the type name `User`; must stay keyword |
| User derive | `derive T.Wire { … }` (D-METADERIVE1, L3033) | 24 | unchanged | unchanged | stays keyword (binds nothing) |
| Typestate | `state Name { … }` inside a type (D-STATE-DECL) | 1 | unchanged | unchanged | unchanged |
| Extern block | `extern rust "crate@v" { fn f(x) T = "path" }` (S50, L3473) | 23 blocks, 23 fns | unchanged | unchanged | inner `f :: fn(x) -> T = "path"` |
| Test | `#Test("name") { }` (S43, L3357) | 1,988 | unchanged | unchanged | unchanged |
| Import | `use math.[sin, cos as c]` (S16, L1787) | 4,946 | unchanged | unchanged | unchanged |
| Local binding | `name :: expr`, `name := expr` (S2, L186) | 49,756 / 27,621 | unchanged | unchanged | unchanged |
| Local named callable | `helper :: n -> n * 2` (D-SIG-AFTER1 note) | — | unchanged | unchanged | unchanged |

Observations from the table:

- Jet is **already a hybrid**. Name-first today: constants, distinct types, module instantiation, prep constants, locals, local lambdas (≈2,336 module-level `::` lines). Keyword-first: everything with a body (≈39,000 headers). D-ALIAS-BIND2 and D-VIS-REEXPORT1 (both open, both rec A) move aliases and re-exports to name-first.
- Under C, about **7,470 lines still cannot be bindings** because they bind nothing: `impl` 474, `#Test` 1,988, `use` 4,946, `derive` 24, `migration` 11 and `extern` blocks 23 (2,520 without `use`). "Everywhere" is therefore never literally everywhere. Odin has none of these forms (no methods, impls, traits or tests in its grammar).
- `migration User { }` is the sharpest case. Under C it would read `User :: migration { … }`, a second definition of the name `User`.

## 2. Measured criteria

### 2.1 Method

- **Reference** = a comment- and string-aware, brace-scope-aware scanner per language (`measure.mjs`). It classifies each statement-start line as an item, member (field, method, variant) or code (local) by the kind of the enclosing brace. It is not a full parser. Known limits: one-line nested bodies, and Jet `"""` text containing braces (handled) or interpolated quotes (rare).
- **Naive** = the regex a person types into `rg` or GitHub code search, run on raw text that includes comments and strings, as grep does.
- **Name-first Jet corpus** = the real corpus mechanically rewritten header-by-header (`struct P` → `P :: struct`, `fn f(` → `f :: fn(`, …). Comments were not rewritten, so name-first queries look cleaner than they would be in a real name-first codebase: commented-out examples (`// struct Point { … }`) are the main keyword-first false positives (41 of 44 struct false positives, 451 of 651 fn false positives). **Read the keyword-first vs name-first kind-search numbers as a tie.**
- Precision = matches that are real declarations ÷ all matches; recall = declarations found ÷ all declarations. "Micro" precision pools every declared name.

### 2.2 Grep, ripgrep and code search

**Jet, 7,468 files (Core 115, Compiler 311, Examples 1,257, Jetpack 633, tests 4,525, Tools 591).**

| Query | Keyword-first (today) | Name-first rewrite |
|---|---|---|
| All structs: `struct [A-Z]\w*` vs `[A-Z]\w* :: struct` | P 0.989, R 0.9998 | P 1.000, R 0.9998 |
| All enums | P 0.982, R 1.0 | P 1.000, R 1.0 |
| All functions incl. methods: `fn [\w.]+\(` vs `[\w.]+ :: (prep )?fn\(` | P 0.980, R 0.991 | P 1.000, R 0.991 |
| Top-level functions anchored at column 0 | P 0.992, R 0.971 | P 1.000, R 0.971 |
| Definition of function NAME, kind known: `fn NAME` vs `NAME :: fn` | P 0.985 (99.4 % of names noise-free) | P 1.000 |
| Definition of function NAME, kind unknown: `NAME ::` | — | **P 0.561** (95.5 % of names noise-free; the misses are common words) |
| Same, anchored at column 0: `^NAME ::` | — | P 0.994, **R 0.70** (methods and module bodies are indented) |
| Definition of type NAME, kind unknown: `NAME ::` | needs `(struct\|enum\|trait\|…) NAME\|NAME ::` | **P 0.986** |
| Definition of any NAME, any kind | P 0.598 (keyword alternation plus `NAME ::`) | P 0.617 |

The noisiest function names under `NAME ::` are `value` (37 definitions, 1,105 local bindings of the same name), `result`, `root`, `name`, `path`, `text`, `key`, `code`, `source` and `bytes`. Under `fn NAME`, the noisiest is `run` (143 extra hits from comment examples).

**Interpretation.** Kind-specific search is a tie. The name-first "one pattern finds every definition" promise holds in Jet **only for types**, because PascalCase never appears as a local binding (0 indented PascalCase `Name ::` lines in 7,468 files). For functions it fails: `::` is Jet's default local binding.

**Odin core, 1,288 files (native name-first).**

| Query | Result |
|---|---|
| `NAME ::` to find procedure NAME | P 0.976 (locals mostly use `:=`; local `::` is 18 % of local bindings vs Jet's 64 %) |
| All structs `\w+ :: struct` | P 0.888, R 1.0 (false positives: procedure-local types and comment text) |
| Strict `\w+ :: struct \{` | R 0.851: 253 of 1,702 struct headers carry something between `struct` and `{`, such as `#packed`, `#raw_union` or `($T: typeid)` |
| All procedures `\w+ :: proc` | **R 0.31**: 28,991 headers read `:: #force_inline proc` |
| Directive-tolerant `\w+ :: (#\w+ )*proc` | R 1.0, P 0.873: 3,363 overload groups `:: proc{a, b}` and `#type` aliases also match |
| `#type` uses | 93 (`::` + `#type proc(…)` declares a procedure *type*) |

Lesson for Jet: in a name-first form, keep the kind word **immediately after `::`**. Odin puts modifiers between the two and loses naive recall. Jet's markers go before the name (`#Codable Point :: struct`), so recommendation A keeps `:: struct` adjacent by construction.

**Zig std 0.16, 550 files (types by const binding, functions by keyword).**

| Query | Result |
|---|---|
| All structs `const \w+ = (extern\|packed )?struct` | P 0.933, R 1.0 |
| Definition of type NAME `const NAME =` | **P 0.598**: Zig binds types locally (`const T = …`, `const Self = @This()` in 234 files) |
| Definition of function `fn NAME(` | P 0.955 |
| Container literals | 4,035 bound by name; 180 returned from type functions; 1,499 elsewhere (field types, arguments, tests) |
| Type-returning functions (generics) | 253 container-level `fn X(…) type` |

Zig shows both the upside and a trap. Its function search stays precise because functions keep `fn`. Its type search degrades because types may be bound inside functions and given synthetic names (the langref's "Struct Naming" rule produces `filename.funcname__struct_ID`). Jet avoids both by D-ALIAS-BIND2's rule that a type on the right inside a function is an error. Recommendation A also requires that a `struct`/`enum`/`trait` expression appears only as a module-level binding's whole right side or as a recipe's result.

**Go (11,389 functions, `src/{net/http,strings,bytes,go/*,encoding/json,fmt,os,sort,sync,time}`).** `type X struct`: P 0.993. 33 % of functions are methods written `func (r T) Name(`, so `func Name(` misses every method: a receiver between the keyword and the name breaks keyword-first grep. Jet's `fn Type.method(` keeps the keyword adjacent.

**Rust (273 files from 10 crates).** Raw `struct X` P 0.847, `fn x` P 0.940, `enum X` P 0.672: rustdoc examples in `///` comments are false positives. Jet's equivalents are 0.989 and 0.980, because Jet comment examples are rarer.

**GitHub.** Code navigation (go-to-definition, symbols pane) runs on tree-sitter for a fixed list that includes Go, Rust, Swift and TypeScript but **not** Zig or Odin ([GitHub docs](https://docs.github.com/en/repositories/working-with-files/using-files/navigating-code-on-github)). A new language such as Jet will rely on plain-text and regex code search for years. That is why the text-level numbers above matter. Both forms are regex-friendly; only the kind-unknown function query separates them.

### 2.3 IDE and tooling

- **Outline, go-to-definition, references, rename** come from the parser and semantic index, not the surface. Jet's Zed outline (`Tools/editors/zed/languages/jet/outline.scm`) keys on node types (`struct_def`, `function_def`, …). Either style yields the same nodes once `Tools/editors/tree-sitter/grammar.js` is updated. A touches the struct/enum/trait rules; C touches nearly every item rule, plus field-versus-method and slot-versus-type conflicts.
- **ctags / tree-sitter `tags.scm`** are node-based too. Their quality depends on the grammar distinguishing kinds. That is easy when a kind word follows `::` (A and C) and hard for bodiless function signatures in C.
- **Formatter and alignment.** Name-first puts names at column 0 and aligns them; keyword-first puts the kind at column 0 and lets names float by keyword length (`fn ` 3, `enum ` 5, `trait ` 6, `struct ` 7). In A, type names align and function names keep the `fn ` offset.

### 2.4 Editing, refactoring, review noise

| Edit | Keyword-first | Name-first |
|---|---|---|
| Rename | one token at the definition | same |
| struct → enum | `struct Shape {` → `enum Shape {` (one token) | `Shape :: struct {` → `Shape :: enum {` (one token) |
| Add generics (D-GENERIC-TYPEFN1) | `struct Pair {…}` → `prep fn Pair(T: Type) -> Type { struct {…} }`: header rewritten, the name leaves the struct header | `Pair :: struct {…}` → `prep fn Pair(T: Type) -> Type { struct {…} }`: the `struct {…}` text moves unchanged |
| Local helper → top-level function | `helper :: n -> n * 2` → `fn helper(n: Int) -> Int { n * 2 }` | C does not help, because Jet lambdas are not `fn(…) { }` literals (Jai's main argument needs that literal) |
| Header width | — | +3 bytes per header; the full C rewrite adds 117 KB over 38,998 headers |

Review noise is equal per edit. A cutover diff is the one large difference: about 5.4k type headers for A versus 39k headers in 7,095 of 7,468 files for C.

### 2.5 Parse complexity, ambiguity, error recovery, diagnostics

- **Dispatch.** Keyword-first is LL(1) on the first token. Name-first is `Ident ::` and then a dispatch on the right side (LL(3)). Both are trivial. Jet already parses module-level `NAME :: value`, `Name :: distinct T` and `module x :: y(…)` this way (`Modules.rs:1188-1198`, `visibility_items.rs:59-69`).
- **Ambiguities name-first must resolve.** Jet already accepts one, by resolution: `Name :: path` is an alias, a re-export or a constant (D-ALIAS-BIND2/D-VIS-REEXPORT1 ordered rule). C adds three more:
  - (a) a bodiless `name :: fn(…) -> T` is a trait slot, an extern function or a function-type alias. Odin needs `---` and `#type`; GingerBill: "a procedure without a body cannot be distinguished from a procedure type" ([2018 article](https://www.gingerbill.org/article/2018/03/12/on-the-aesthetics-of-the-syntax-of-declarations/)).
  - (b) `area: fn(Shape) -> Float` (a field) versus `area :: fn(self) -> Float { }` (a method), one character apart in the same body.
  - (c) `int_cache :: cache(Int, 32)` is a module instance, a type recipe application or a constant call, decided by name resolution.
  
  A adds none of these: functions keep `fn`, and `Name :: struct {` has a reserved word on the right.
- **Error recovery.** `sync_top` resyncs on `fn | pub | struct | enum | impl | const | comptime` (`mod.rs:909-925`). Under C, a stray brace leaves the parser unable to tell `name ::` declarations (45 %) from locals (55 %) by tokens. Under A, `fn` remains, and `PascalIdent ::` at line start is a declaration in 100 % of the measured corpus.
- **Typo diagnostics.**
  - Keyword-first `strcut Point { x: Int }` is a parse error at `strcut`; E0003 lists the item forms, and a did-you-mean is a small add.
  - Name-first `Point :: strcut { x: Int }` *parses* as a typed construction `strcut{ x: Int }` (S29 `Type{ field: expr }`), so the error surfaces in name resolution ("unknown name `strcut`"). A must special-case "identifier within edit distance 2 of `struct`/`enum`/`trait` followed by `{` after a module-level `Name ::`" to teach `struct`.
  - `point :: struct {}` gets the name-case error that names the kind (D-ALIAS-BIND2 E0357 path).
  - `Point := struct {}` gets "a type is fixed; use `::`".
  - `struct Point {}` (the retired form) gets an error whose `jet fix` edit rewrites it. The parser keeps this only as a recovery path, as D-SIG-AFTER1 did.

### 2.6 Learnability

- **No controlled study compares name-first with kind-first declarations.** None was found; this report does not invent one. Stefik & Siebert ("An Empirical Investigation into Programming Language Syntax", ACM TOCE 13(4), 2013) show that syntax choices measurably change novice accuracy. They do not test declaration order.
- **Transfer.** Functions are keyword-first in Rust `fn`, Go `func`, Swift `func`, Kotlin `fun`, Python `def`, JavaScript/TypeScript `function`, Zig `fn`, Ruby/Elixir/Scala `def`. They are name-first only in Odin, Jai and Haskell equations. Types are split: keyword-first in Rust, Swift, Kotlin and Python; `type X struct` in Go; binding-like in TypeScript `type X = {…}`, Zig `const X = struct`, Odin and Jai. A keeps the near-universal function form and uses the binding form where several mainstream languages already do.
- **Jet beginners** learn `x :: 5` on day one. A extends that one rule to types ("a type is a value with a name"). C extends it to functions and then needs exceptions: methods look like constants, slots need a no-body mark, and `run := fn` must be refused. D-SIG-AFTER1's C why-not made this point.
- **Confusable pairs.** Jet retired `++`/`--` on atoms-of-confusion evidence (D-INCR1). C would create a one-character confusable, `name:` field versus `name ::` method, in the place beginners write most (type bodies).

### 2.7 Consistency with ratified and pending law

| Law | A | B | C |
|---|---|---|---|
| S2 `::` binds a name; D-ALIAS-OP1=B "every new name arrives through `::`" | types join constants | types stay keyword | everything joins |
| D-ALIAS-BIND2 (open, rec A) `Rows :: [[String]]` | same shape for every type | aliases differ from structs | same |
| D-VIS-REEXPORT1 (open, rec A) `clamp :: core.math.clamp` | same | same | same |
| Card #4723: `priv module` only, no `pub` | `priv module` unchanged | unchanged | needs a new place for `priv` on `name :: module` |
| D-GENERIC-TYPEFN1=A: `struct { }` is an expression | one struct grammar | **two struct grammars** (`struct P {}` and `struct {}`) or an arbitrary ban on `P :: struct {}` | one struct grammar |
| D-SIG-AFTER1=A (2026-09-18) rejected `double :: fn(…)` | kept | kept | **reverses it** |
| D-CALLABLE-ONE1: one anonymous callable shape | kept | kept | strained: `fn(…) { }` becomes an expression or a binding-only form |
| D-MARKER-DECL1: `marker` declares everything applied with `#` | kept | kept | `X :: marker(…)` |
| "Say it once" / one mechanism (philosophy priority 4) | one spelling per concept | duplicate struct spelling | one spelling, but two declaration shapes remain (bindings vs `impl`/`#Test`/`use`/…) |

### 2.8 Compile-time programming

D-GENERIC-TYPEFN1=A follows Zig: types are build-time values made by recipes, and `Pair(Int)` is a type expression. Under A every type expression is named the same way: `IntPair :: Pair(Int)`, `Rows :: [[String]]` and `Point :: struct { … }` share one form. That matches Zig's `const Point = struct {}` and `const IntList = std.ArrayList(i32)`. Functions are not build-time values in Jet (D-META-GATE1 still refuses functions as compile-time objects; only type recipes were amended), so binding them buys nothing semantic. That was exactly D-SIG-AFTER1's C why-not.

Nominal identity under A: a `struct`/`enum`/`trait` expression mints a new nominal type, named by the binding that holds it or by the recipe and its arguments. Odin documents the same rule: "Aggregate types (struct, enum, union) will always be distinct even when named", with `#assert(Foo != struct{})` ([overview](https://odin-lang.org/docs/overview/)). `Alias :: Point` is a transparent alias. `Usd :: distinct Point` is a new type over it.

### 2.9 Docs generators

Doc output is extracted by the compiler's parser and semantic index, not by text patterns, so the style does not change what is extracted. A gives docs one heading shape for every type (`Name :: kind`) and keeps `fn name(…)` signatures, which most readers recognise.

### 2.10 Migration cost (shared work; the owner has ruled that migration does not rank options)

| Surface | A (types) | C (everything) |
|---|---:|---:|
| `.jet` declaration headers | ≈5,437 (4,073 struct, 1,276 enum, 76 trait, 5 protocol stay, 7 alias via D-ALIAS-BIND2) | 38,998 in 7,095 files, +117 KB |
| Markdown ```jet fences (47 files) | subset of 500 header lines | 500 |
| Rust string literals holding Jet code (309 files) | subset of ≈2,559 lines | ≈2,559 |
| UI snapshot text (284 files) | subset of 446 lines | 446 |
| Grammars | tree-sitter `grammar.js`, TextMate, Zed `highlights.scm`/`outline.scm` | same, larger |
| Compiler | parser item dispatch (+1 RHS arm per kind), `sync_top`, formatter `Formatter/Items.rs`, E0003 hints, new did-you-mean | all item rules, field/method and slot/type disambiguation, module/marker/fact/effect binding forms |

The rewrite is mechanical: `measure.mjs` rewrote every header line-locally, using only the sanitized column positions. A real cutover is a `jet fix` rule plus the rewrite script that card #4723 already plans for re-runnable migrations.

## 3. Six real Jet programs, three ways

Programs are verbatim from the repository at `277033b7c`. Options remove `pub` per card #4723 and are otherwise literal rewrites. "Today" keeps `pub` where the file has it. None of the A/C spellings run today.

### P1 — `Examples/features/traits/display_read_receiver.jet` (struct, impl, method in a body)

```jet
// Today / B
struct Label {
    text: String
}

impl Label.Display {
    fn display(self) -> String { "label:{self.text}" }
}

struct Envelope {
    text: String
    impl Display {
        fn display(self) -> String { "envelope:{self.text}" }
    }
}

fn show(value: Label) {
    print(value)
    print("interpolation:{value}")
}
```

```jet
// A — types by binding
Label :: struct {
    text: String
}

impl Label.Display {
    fn display(self) -> String { "label:{self.text}" }
}

Envelope :: struct {
    text: String
    impl Display {
        fn display(self) -> String { "envelope:{self.text}" }
    }
}

fn show(value: Label) {
    print(value)
    print("interpolation:{value}")
}
```

```jet
// C — everything by binding
Label :: struct {
    text: String
}

impl Label.Display {
    display :: fn(self) -> String { "label:{self.text}" }
}

Envelope :: struct {
    text: String                 // a field: one colon
    impl Display {
        display :: fn(self) -> String { "envelope:{self.text}" }   // a method: two colons
    }
}

show :: fn(value: Label) {
    print(value)
    print("interpolation:{value}")
}
```

### P2 — `Examples/features/modules/member_import_types/types.jet` (struct, enum, trait with a slot, function)

```jet
// Today
pub struct Span {
    pub start: Int
    pub end: Int
}

pub enum Kind { Alpha Beta(Int) }

pub trait Named {
    fn label(self) -> String
}

pub fn make_span(start: Int, end: Int) -> Span { Span{start: start, end: end} }
```

```jet
// A
Span :: struct {
    start: Int
    end: Int
}

Kind :: enum { Alpha Beta(Int) }

Named :: trait {
    fn label(self) -> String
}

fn make_span(start: Int, end: Int) -> Span { Span{start: start, end: end} }
```

```jet
// C
Span :: struct {
    start: Int
    end: Int
}

Kind :: enum { Alpha Beta(Int) }

Named :: trait {
    label :: fn(self) -> String      // slot, or an alias of a function type? needs a rule or a mark
}

make_span :: fn(start: Int, end: Int) -> Span { Span{start: start, end: end} }
```

### P3 — `Examples/features/tooling/checked_text_type.jet` (enum, distinct, impl with an associated type)

```jet
// Today / B
#Error
enum TextError { Empty }

Token :: distinct String

impl Token.CheckedText {
    type Error = TextError

    fn check(text: String) TextError! -[]> {
        if text == "" { return Err(TextError.Empty) }
        return Ok(())
    }
}

fn show(value: Token) { … }
```

```jet
// A
#Error
TextError :: enum { Empty }

Token :: distinct String

impl Token.CheckedText {
    Error :: TextError             // associated type: a type, so a binding

    fn check(text: String) TextError! -[]> {
        if text == "" { return Err(TextError.Empty) }
        return Ok(())
    }
}

fn show(value: Token) { … }
```

```jet
// C
#Error
TextError :: enum { Empty }

Token :: distinct String

impl Token.CheckedText {
    Error :: TextError

    check :: fn(text: String) TextError! -[]> {
        if text == "" { return Err(TextError.Empty) }
        return Ok(())
    }
}

show :: fn(value: Token) { … }
```

(P3's `fn check(text: String) TextError! -[]>` is the file's current text, which predates D-SIG-AFTER1's cutover. It is kept verbatim.)

### P4 — `Examples/features/types/associated_types.jet` (trait with an associated type and slots)

```jet
// Today / B
trait Indexed {
    type Elem
    fn at(self, i: Int) -> Elem
    fn count(self) -> Int
}

struct Nums {
    vals: [Int]
}

impl Nums.Indexed {
    type Elem = Int
    fn at(self, i: Int) -> Int { self.vals[i] }
    fn count(self) -> Int { self.vals.len() }
}

fn run() {
    n :: Nums{vals: [10, 20, 30]}
    print(n.at(0))
}
```

```jet
// A
Indexed :: trait {
    type Elem                       // a slot: binds nothing yet, keeps its word
    fn at(self, i: Int) -> Elem
    fn count(self) -> Int
}

Nums :: struct {
    vals: [Int]
}

impl Nums.Indexed {
    Elem :: Int
    fn at(self, i: Int) -> Int { self.vals[i] }
    fn count(self) -> Int { self.vals.len() }
}

fn run() {
    n :: Nums{vals: [10, 20, 30]}
    print(n.at(0))
}
```

```jet
// C
Indexed :: trait {
    type Elem                       // still a keyword: a slot has no value to bind
    at :: fn(self, i: Int) -> Elem
    count :: fn(self) -> Int
}

Nums :: struct {
    vals: [Int]
}

impl Nums.Indexed {
    Elem :: Int
    at :: fn(self, i: Int) -> Int { self.vals[i] }
    count :: fn(self) -> Int { self.vals.len() }
}

run :: fn() {
    n :: Nums{vals: [10, 20, 30]}   // local binding: same shape as the declarations above
    print(n.at(0))
}
```

### P5 — `Examples/features/comptime/prep_struct_rows.jet` (struct, enum, build-time constants)

```jet
// Today / B
struct Row {
    name: String
    bits: Int
}

enum Mode {
    Fast
    Slow
}

fn make(name: String, bits: Int) -> Row Never! { Row{name: name, bits: bits} }

ROW :: prep { make("a", 1) }
MODE :: prep { mode("fast") }
```

```jet
// A
Row :: struct {
    name: String
    bits: Int
}

Mode :: enum {
    Fast
    Slow
}

fn make(name: String, bits: Int) -> Row Never! { Row{name: name, bits: bits} }

ROW :: prep { make("a", 1) }
MODE :: prep { mode("fast") }
```

```jet
// C
Row :: struct {
    name: String
    bits: Int
}

Mode :: enum {
    Fast
    Slow
}

make :: fn(name: String, bits: Int) -> Row Never! { Row{name: name, bits: bits} }

ROW :: prep { make("a", 1) }
MODE :: prep { mode("fast") }
```

### P6 — `Examples/features/modules/inline_module.jet` (module with a type and functions)

```jet
// Today
module arith {
    pub struct Tally {
        total: Int
    }

    pub fn double(n: Int) -> Int { n * 2 }

    pub fn tally(a: Int, b: Int) -> Tally { return Tally{total: add(a, b)} }
}

fn describe(t: arith.Tally) -> String { "total {t.total}" }
```

```jet
// A
module arith {
    Tally :: struct {
        total: Int
    }

    fn double(n: Int) -> Int { n * 2 }

    fn tally(a: Int, b: Int) -> Tally { return Tally{total: add(a, b)} }
}

fn describe(t: arith.Tally) -> String { "total {t.total}" }
```

```jet
// C
arith :: module {
    Tally :: struct {
        total: Int
    }

    double :: fn(n: Int) -> Int { n * 2 }

    tally :: fn(a: Int, b: Int) -> Tally { return Tally{total: add(a, b)} }
}

describe :: fn(t: arith.Tally) -> String { "total {t.total}" }
```

### P7 — generics under D-GENERIC-TYPEFN1 (ratified; not implemented yet)

```jet
// Today's ratified spelling (B and A are identical: a recipe is a function)
prep fn Pair(T: Type) -> Type { struct { first: T, second: T } }
IntPair :: Pair(Int)                 // A, and B if D-ALIAS-BIND2=A
fn larger(prep T: Comparable, a: T, b: T) -> T { if a > b -> a else -> b }

// C
Pair :: prep fn(T: Type) -> Type { struct { first: T, second: T } }
IntPair :: Pair(Int)
larger :: fn(prep T: Comparable, a: T, b: T) -> T { if a > b -> a else -> b }
```

## 4. Evidence from the wild

- **Odin** (name-first everywhere, `proc`). Its designer, writing in 2018, preferred name-first for elegance but recorded doubts. He switched the whole syntax to Go-like qualifier-first twice for two weeks, and needed `---` because "a procedure without a body cannot be distinguished from a procedure type" ([gingerBill, "On the Aesthetics of the Syntax of Declarations"](https://www.gingerbill.org/article/2018/03/12/on-the-aesthetics-of-the-syntax-of-declarations/)). The overview documents `---` for foreign procedures and `#type` "for telling someone reading the code that the expression is a type … a procedure signature without a body is a type and not just missing its body" ([odin-lang.org/docs/overview](https://odin-lang.org/docs/overview/)). Measured: 93 `#type` uses and 28,991 `#force_inline` headers in core (§2.2). Odin has no methods, traits, impls or test blocks, so it never meets Jet's field/method and slot cases.
- **Jai** (name-first, `f :: (…) -> T { }`). The stated rationale is refactoring. "The strength of Jai's function syntax is that it doesn't change whether the function is an anonymous function, a local function … or a global function", with a "code maturation process" from block to captured block to lambda to named local to named global ([Jai Primer](https://github.com/BSVino/JaiPrimer/blob/master/JaiPrimer.md)). The payoff requires the lambda and the function to share one syntax. Jet's lambdas are `x -> …` and its named functions are `fn`, by ratified law (D-SIG-AFTER1, D-LAMBDA-IFACE2, D-CALLABLE-ONE1), so the payoff does not transfer.
- **Zig** (types `const X = struct {}`, functions `fn`). Every struct is anonymous and named by its binding, by its returning function plus serialized arguments, or by a synthetic `filename.funcname__struct_ID` ([langref, Struct Naming](https://ziglang.org/documentation/0.15.2/#Struct-Naming)). Proposal #1717 ("make function definitions expressions", `const f = fn(…) {}`) was accepted in 2019 for consistency. Andrew Kelley rejected it on 2023-07-09: "there is a good reason for there to *not* be syntax consistency between function declarations and other declarations … functions have symbol names. These symbol names show up in stack traces, performance measurements, debugging tools … This is different from constants and types, which may exist ephemerally and be unnamed. So, I think the syntax inconsistency appropriately models reality" ([#1717](https://github.com/ziglang/zig/issues/1717)). An early comment in the same thread flagged that `const A = fn(i32) void;` (type) and `const B = fn(x: i32) void {};` (function) are "a little too close". Zig 0.15 also removed `usingnamespace` because it sent readers "on a wild goose chase" for definitions (cited in D-VIS-REEXPORT1).
- **Go** (`func f()`, `type X struct`). Keyword, then name, then kind. Type aliases (`type X = Y`) arrived in Go 1.9; generalized aliases for every kind were rejected for 1.8 because they complicated other declaration forms and crashed `vet`/`doc` (golang/proposal 18130, cited in D-VIS-REEXPORT1). Methods put the receiver between `func` and the name (§2.2: 33 % of functions).
- **Rust** (`fn`, `struct`, `enum`, `trait`, `impl`, `type X = Y;`). Keyword-first items throughout; type aliases use `type`. Rust's own pain point is doc-comment noise in grep (§2.2), not declaration order.
- **Swift** (`func`, `struct`, `enum`, `protocol`, `typealias`) and **Kotlin** (`fun`, `class`, `typealias`). Keyword-first throughout, with a separate alias keyword.
- **TypeScript** has both forms for functions and shows how teams choose. `function f() {}` and `const f = () => {}` coexist; Google's TypeScript style guide says "Prefer function declarations over arrow functions or function expressions when defining named functions" ([Google TS style guide](https://google.github.io/styleguide/tsguide.html)). Type aliases are binding-shaped: `type Point = { x: number }`.
- **Haskell.** Types are keyword-first (`data`, `newtype`, `type`, `class`). Functions are name-first equations with an optional `f :: Int -> Int` signature, where `::` means "has type", not "is bound to".
- **Python.** gingerBill's article calls it the canonical hybrid: name-first for variables, keyword-first for `def`/`class`/`import`.

The pattern across mature languages: **keyword-first functions are near-universal.** Binding-shaped type names appear wherever types are values or aliases (Zig, Odin, Jai, TypeScript `type`). The only language that tried to move functions to bindings for consistency (Zig) reversed itself after four years.

## 5. Options for the owner

- **A — Types by binding, code by keyword (recommended).** Every type and constant is `Name :: …`; `fn`, `prep fn`, `impl`, `module`, `marker`, `fact`, `effect`, `migration`, `derive`, `extern`, `#Test`, `use` keep their words. Retires `struct Name {}`, `enum Name {}` and `trait Name {}` (with a `jet fix` edit) and folds `type Elem = Int` in impls into `Elem :: Int`. Subsumes D-ALIAS-BIND2=A's alias spelling.
- **B — Keyword-first for every definition with a body.** `struct Point { }`, `enum`, `trait`, `fn` stay. `::` names expressions only: constants, distinct, aliases, re-exports. To keep one spelling, B must forbid `Point :: struct { … }` outside recipe results, so the anonymous struct expression exists only inside `prep fn` bodies.
- **C — Odin/Jai everywhere.** Every named declaration is `name :: kind …`: functions, methods, slots, modules, markers, facts, effects. `impl`, `#Test`, `use`, `derive`, `migration` and `extern` blocks stay keywords because they bind nothing. Needs a no-body mark for slots and extern functions, a rule separating `name:` fields from `name ::` methods, a placement for `priv` on modules, and an explicit reversal of D-SIG-AFTER1=A.

## 6. Recommendation

### 6.1 Decisive reasons for A

1. **One spelling per concept, given ratified law.** `struct { }` is already an expression (D-GENERIC-TYPEFN1=A), so `Point :: struct { }` is the natural and only necessary spelling. B keeps a second one or bans a natural one. Functions have no expression form, so `fn name` is already single, and C would add a binding-only pseudo-expression.
2. **It delivers name-first's real benefit where it is real.** `Name ::` finds every type of every kind at 98.6 % precision in this corpus. It would find functions at only 56 %, because `::` is Jet's everyday local binding (64 % of locals; Odin 18 %).
3. **It follows the strongest field evidence.** Zig's 2023 reversal: functions are named machine artifacts, types and constants are values. Jet ships `jet perf`, stack traces and a debugger that show function symbols.
4. **It adds no new ambiguity.** C needs Odin's `---`/`#type` style marks for 122 bodiless signatures, a one-colon field/method distinction across 3,249 methods and 14,132 fields, and resolution-time module-versus-type-versus-call decisions.
5. **It respects a three-week-old ratification.** D-SIG-AFTER1=A (2026-09-18) settled `fn name(…) -> T { }` and rejected `double :: fn(…)` as "the most text for the least gain". Nothing since then changes that for functions; D-GENERIC-TYPEFN1 changed it for types only.

### 6.2 Why B and C lose

- **B** keeps the most familiar look for Rust, Swift and Kotlin readers. But it leaves types split across two shapes (`struct Point {}` vs `Rows :: [[String]]`, `Usd :: distinct Decimal`, `IntPair :: Pair(Int)`) and a duplicate struct grammar beside the recipe form. Its "find any type" query needs a keyword alternation.
- **C** gives one visual rule for named things, Odin's look and column-0 names. But it measurably weakens function search (56 %). It reintroduces the ambiguities Odin patches with `---` and `#type`, creates a one-colon field/method confusable, reverses D-SIG-AFTER1, and still keeps about 7,400 keyword lines (`impl`, `#Test`, `use`, …), so it is not "everywhere" either.

### 6.3 The real costs of A, with mitigations

1. **Type declarations no longer begin with their kind word.** A reader scanning for "struct" finds it after the name, and Rust/Swift/Go readers meet an unfamiliar type form. *Mitigation:* the formatter keeps the kind word directly after `::` with nothing between (Odin's `#force_inline` lesson). Markers stay before the name. `rg ':: struct'` then finds every struct (P 1.0, R 0.9998 on the rewritten corpus), and the editor outline shows the kind icon.
2. **Two declaration shapes remain: bindings for types and values, keywords for code.** *Mitigation:* the rule fits one sentence the compiler teaches: "A type is a value, so it gets a name with `::`; code keeps `fn`." Each retired form (`struct Point {}`, `point :: fn(…)`) gets an error that names the rule and a `jet fix` edit. Zig, the language D-GENERIC-TYPEFN1 adopted, draws the same line.

Shared work for any change (not a loss): the `.jet` cutover (≈5.4k headers for A), Markdown fences, Rust string fixtures, UI snapshots, the tree-sitter/TextMate/Zed grammars, parser dispatch and recovery, the formatter, and one did-you-mean for `strcut`.

### 6.4 Rules A must state (for the ballot's technical field)

1. At module, file or type-body level, `Name :: struct { … }`, `Name :: enum { … }` and `Name :: trait { … }` mint a new nominal type named `Name`. Any other type on the right is a transparent alias (D-ALIAS-BIND2), `distinct` makes a separate type, and a value makes a constant.
2. A `struct`/`enum`/`trait` expression is legal only as the whole right side of such a binding or as a recipe's result. Anywhere else is an error that says "name this type at module level".
3. Inside a function, a type on the right is an error whose fix moves the line out (D-ALIAS-BIND2). `:=` with a type is an error.
4. Type names are PascalCase; a lowercase name gets the name-case error that names the kind. Recovery syncs on `fn`, `impl`, `module`, `marker`, `#Test`, `use` and `PascalIdent ::` at line start.
5. Markers precede the name (`#Error TextError :: enum { … }`), and the kind word immediately follows `::`.
6. Trait associated-type slots keep `type Elem`; impls bind them with `Elem :: Int` (replacing `type Elem = Int`).
7. `struct Name {}`, `enum Name {}` and `trait Name {}` are retired, each with an error and a `jet fix` edit to the binding form.

## 7. Limits of this report

- The scanners are heuristic. Full-parser ground truth was not available offline for Odin, Zig or Go, and the Jet compiler was not run (read-only brief). The scanner's own inventory adds up consistently, and the naive regexes were checked on samples (`dbg.mjs`).
- The name-first Jet corpus is synthetic. Comments were not rewritten, so kind-search precision favours name-first slightly (§2.1).
- GitHub code-search and adoption numbers were not collected; the GitHub claim is limited to the documented navigation language list.
- No experiment measured human reading speed for either form. Learnability claims are labelled as transfer or inference.

## 8. Ballot draft

`~/.cache/jet-dev/scratch/ballots/D-DECL-FORM1.json` is the full-profile draft: A recommended, `draft: true`, `cardId` empty. `ballot.mjs` beside this report generates it and runs Tower's own `addDecision` gate in a throwaway store. The draft passes; the ready shape also passes, using placeholder reader receipts that exist only in the throwaway store. Still owed before submission: a card id, a fresh beginner (rli5) pass, a fresh adversarial pass, then repairs.
