# Jet Syntax Delta Proposal v5

Status: proposal artifact only. Not ratified. Not implemented.

v5 delta over v4: redraws the `#`/`@` line on a principled test — `@` states
a checkable contract on the declaration it precedes; `#` is a compiler
directive (what compiles, when it runs, what is legal, what is generated,
compile-time values). The v4 "modes vs facts" line was compiler taxonomy; the
v5 line is checkable by eye. `~` becomes reserved. Visibility becomes
per-item `@pub` with private default. Reflection queries move to
`#proc()`/`#caller()`. Adds lifecycle entry points: `jet <verb>` resolves to
`fn <verb>()`, and the entry point is `fn run()` — no `fn main()`.

Source basis: `docs/spec/philosophy.md`, `docs/spec/syntax-decisions.md`,
`docs/spec/spec.md`, `docs/reference/syntax-surface.jet`, `examples/canon.jet`,
selected `examples/features/*.jet`, and `crates/jet-foundation/src/Syntax.rs`.

This file is delta-first. Each feature gets one code block. If syntax should not
change, the block starts with `KEEP`. If syntax should change, the block starts
with `CHANGE` and shows `CURRENT` next to `PROPOSED`.

Domain used throughout: PulseOps, a real incident-response service: webhooks,
database writes, paging, CLI import, UI, typed config, protocol exchange, and
one expert low-level slice.

## Executive Summary

The current language has good raw material but inconsistent visual law:

- `#` means marker, fixed length, version pin, and ordinary immutable binding.
- `if subject == {}` is useful, but needs a strict table grammar so `if` does
  not become arbitrary dispatch syntax.
- `~` is too physically awkward for common write access. Keep sigils, but move
  the common write grant to postfix `!` on values and prefix `!T` in types.
- `#Pure fn`, `#[Codable]`, `#Layout(c)`, `#Test("x")`, `#Bench "x"` mix marker
  shapes and mix two different jobs: contracts about declarations and
  instructions to the compiler.
- The v4 `#`/`@` split drew its line at "compiler modes vs attached facts" —
  compiler taxonomy a reader cannot check by eye. The line must be a test the
  reader can apply: does this state a contract about the declaration below
  (`@`), or does it change what/when/how code compiles (`#`)?
- Custom dev/build behavior has no home in Jet source; other ecosystems bolt
  on config DSLs and second build languages.
- Trait names in value position can hide dynamic dispatch and allocation.
- `ok`/`err`/`value`/`null` are special constructors instead of using the same
  enum/dot machinery as the rest of Jet.
- Protocols generate method names (`Hello`, `recv_Ack`) instead of exposing one
  obvious send/receive shape.

Recommended profile:

```jet
// CHANGE: v5 visual law
// 1. `::` = bind a new fixed name.
// 2. `:=` = bind a new editable name.
// 3. Access capability stays sigil-based: `!`, `^`, `&`.
// 4. `if subject == {}` owns subject dispatch/pattern tables.
// 5. Optional/result constructors become dotted variants.
// 6. `@` states a contract on the declaration it precedes. `#` is a
//    compiler directive: what compiles, when it runs, what is legal, what
//    is generated, and compile-time values. `$` splices inside generated
//    Jet. `~` stays reserved.
// 7. Dynamic dispatch uses `dyn Trait` in public/expert syntax.
// 8. `jet <verb>` maps to `fn <verb>()`: run -> run, dev -> dev,
//    build -> build. No `main`; no exceptions. Absent verb fn ->
//    batteries-included default.

incident_id :: "inc-2026-071"       // fixed local binding
attempts := 0                       // editable local binding
attempts += 1

fn deposit(account: !Account, cents: Int) {
    account.balance += cents
}
deposit(account!, 500)

fn close(file: ^File) -> Receipt { ... }
fn cache(report: &Report) { ... }

result :: if parse_event(raw) == {
    .Ok(event) -> event
    .Err(e) -> return .Err(e)
}
```

Hard challenge: this rejects the current "mostly keep everything" path. The
proposal spends breaking syntax budget where it buys a smaller mental model:
binding has two grepable operators, access authority has three sigils,
meta speech has two sigils with an eye-checkable line between them, subject
dispatch gets one table form, and every sum-like thing uses one
dot-constructor story.

## Sigil Law

```jet
// CHANGE
// CURRENT:
//   #  marker, fixed-size count, version pin, immutable binding `#=`
//   ~  reserved for rare expert views; do not spend it on common writes
//   ?  optional/fallible/propagate/fallback
//   .  member/module/method/variant/construction/fan-out
//   @  labels, debug interpolation, os host selectors
//
// PROPOSED: four planes. The first character of a token tells you which
// plane you are in.
//
//   VALUE PLANE — runtime meaning, read left to right:
//   :: bind a new fixed name
//   := bind a new editable name
//   !T, x!  grant exclusive write access
//   ^T, ^x  move ownership
//   &T, &x  grant retained/shared access
//   *  raw pointer only: `*T`, `*x`, `p.*`, gated by `#unsafe`
//   ?  uncertainty/failure only
//   .  member of something: module, field, method, variant, typed construction
//
//   ATTRIBUTE PLANE — `@` states a checkable contract on the declaration
//   it precedes. Never inside a type or expression.
//   @fact fn/struct/field    one contract: @pure, @pub, @must_use, @redact
//   @[fact, fact(arg)]       several contracts: @[codable, rename_all(camel)]
//
//   DIRECTIVE PLANE — `#` speaks to the compiler, a la C directives:
//   what compiles, when it runs, what is legal, what is generated, and
//   compile-time values.
//   #mode("why") { }         scoped mode: #unsafe, #comptime, #context
//   #unsafe("why") fn        item-scoped mode (changes what is legal inside)
//   #test "name" { }         compiler-owned items: #test, #bench, #derive,
//                            #ffi, #unit, #target, #explicit, #suppress
//   #(Fs, Net)               effect set on a function type
//   [T#N], pkg#1.2.3         compile-time quantity attached to a name
//   #proc(), #caller()       compile-time value in expression position
//
//   GENERATION PLANE — inside emitted Jet only:
//   $T, $name, $(expr)       splice a compile-time value into generated source
//
//   RESERVED — deliberately unassigned: `~`. A language meant to live
//   decades keeps a sigil in reserve.
```

Challenge: if a sigil cannot be explained in one sentence and one grep command,
it is not mature enough for a language trying to replace C, Go, Rust, Swift, and
TypeScript. The one-sentence tests: `@` — "this states a contract about the
declaration below." `#` — "this instructs the compiler." `$` — "this splices a
compile-time value into generated source." Every value sigil carries exactly
one authority fact.

The `@`/`#` line, stated so a reader can check it by eye:

- `@` only ever directly precedes a declaration, and only states contracts the
  compiler checks or fulfills at that declaration's boundary. It never changes
  what code is legal inside a body, never controls compilation, and never
  appears inside a type or expression.
- `#` never states a contract about a declaration. It changes compilation —
  phase (`#comptime`), legality (`#unsafe`, `#explicit`, `#suppress`),
  inclusion (`#test`, `#target`, `#ffi`), generation (`#derive`, `#unit`) —
  or supplies a compile-time value (`#(Fs)`, `[T#N]`, `pkg#1.2.3`,
  `#caller()`).

This fixes the v4 arbitrariness. v4 drew the line at "modes vs facts" —
compiler taxonomy invisible in source. The v5 line resolves every hard case
mechanically: `@must_use` is a contract on callers, `#suppress` changes
legality in a scope; `@codable` is a capability contract (the derivation is
implementation), `#derive` is generation machinery the user writes;
`#unsafe fn` stays a directive because it changes what is legal inside the
body. And the split keeps `#` rare and loud: `#unsafe` and `#comptime` should
scream, not drown in a sea of visibility and encoding markers.

Tradition backs both assignments: C's `#include`/`#pragma` trained readers
that `#` lines instruct the compiler; Java/Python/C# trained readers that `@`
annotates the declaration below. Jet uses both instincts as-is.

Grep law: `rg '@\w|@\['` is every stated contract; `rg '#\w'` is every
compiler instruction. Two commands audit the whole meta surface.

## Reasoning Laws

```text
Law 1: Syntax must expose semantic boundaries.
       Allocation, dynamic dispatch, mutation, ownership transfer, effects,
       generated code, and ambient context must be visible or expandable.

Law 2: Same semantic fact gets one surface family.
       Sum variants use dot variants. Declaration contracts use `@`.
       Compiler instructions use `#`. Bindings use binding operators.
       Access authority uses value sigils.

Law 3: Hidden beginner magic must have an expert view in Jet source.
       Generated Rust is backend artifact, not the language truth.

Law 4: Reasoning beats typing speed, but source shape still matters.
       `::`/`:=` and `!`/`^`/`&` are compact because they carry one stable
       semantic fact each. Do not replace stable sigils with prose keywords.

Law 5: Tradition earns respect by encoding real experience.
       Keep `fn`, braces, `?`, modules, and named fields where
       they carry decades of learned value. Change only when Jet has a clearer
       invariant than tradition offers.

Law 6: Fewer lines are a quality feature when semantics stay visible.
       Jet should beat Rust line count by deleting ceremony, unifying concepts,
       deriving boring code, and shipping batteries. It should not win by hiding
       mutation, allocation, dynamic dispatch, effects, or generated behavior.

Law 7: The sigil names the plane.
       No meta sigil = runtime value code. `@` = contract on the declaration
       below. `#` = instruction to the compiler. `$` = spliced into generated
       source. A reader knows the plane of any token from its first character.
```

This pass rejects the v3 word-capability detour. Jet should use sigils when a
symbol has a single stable semantic job and gives reviewers a strong scan line.

## Line Count Budget

```jet
// CHANGE: Jet should win line count by deleting ceremony, not by hiding facts.
//
// RUST-SHAPE CEREMONY:
// - request body extraction
// - serde derive/import glue
// - explicit Result constructors
// - error conversion glue
// - handler trait/wrapper boilerplate
// - selection arms naming every enum path
// - runtime wiring outside the handler
//
// PROPOSED JET TARGET:
@[codable, rename_all(camel)]
struct Intake {
    incident_id: String
    title: String
    severity: Int
}

enum Route {
    Store
    Page
    Drop
}

fn intake(req: http.Request) -> http.Response ? http.Error #(Net, Db, Log) {
    text :: req.body_text()?
    item: Intake :: json.decode<Intake>(text)?

    route :: if item.severity == {
        0..1 -> .Store
        2..10 -> .Page
        else -> .Drop
    }

    db.insert("incidents", item)?
    log.info("stored {item.incident_id} as {route:debug}")
    return .Ok(http.Response.json(.{ status: "stored" }))
}

// EXPERT VIEW:
//   jet expand src/intake.jet --facts types,effects,derives,allocs
// emits/opens expanded Jet for Decode/Encode, effect use, allocations,
// drops, inferred local types, and generated DB mapping.
```

The target is not "short at any cost." The target is fewer places for humans to
invent glue. Batteries, derives, field punning, dotted variants, `?`, `??`,
typed effects, typed config, tests, and generated expanded Jet should remove
entire files of ceremony while preserving audit points.

## 0A. Files, Entry, Comments, Layout

```jet
// KEEP.
// Reason: this is tradition working. It is boring, searchable, teachable, and
// scales from one-file scripts to packages.

// file: intake.jet
// line comments use `//`
/* block comments nest
   /* inner */
*/

fn run() {
    print("PulseOps ready")
}
```

Keep `.jet`, curly braces, no visible statement separators, `//`, and nested
`/* */`. Do not add top-level statements; they make the one-file to package
upgrade worse. The entry point is `fn run()`, not `fn main()` — see 36A; the
command/function verb map has no exceptions.

## 0B. Strings And Text

```jet
// KEEP.
incident_id :: "inc-2026-071"
summary :: "incident {incident_id}: {{literal brace}}"
body :: """
    Customer: {customer.name}
    SLA: {customer.sla_minutes} minutes
    """

chars :: "route".chars()
bytes :: "route".bytes()
```

Keep interpolation as the one string composition path. Keep `String` as text,
`Char` as Unicode scalar, and `[U8]` for bytes. Do not add string `+`.

## 0C. Numbers, Conversions, Overflow

```jet
// KEEP most numeric surface.
status: Int :: 200
latency: Float :: 18.5
byte: U8 :: 255
exact: Decimal :: Decimal("19.99")
huge: BigInt :: BigInt("100000000000000000000")

wide: I64 :: byte.to_i64()
small: U8 :: wide.to_u8()?        // narrowing is fallible

wrapped :: wrapping(byte + 1)
clamped :: saturating(byte + 1)
checked_sum :: checked(byte + 1) ?? .None
```

Keep explicit numeric widths, checked integer overflow, named conversions, and
per-operation overflow escape hatches. This is exactly the right safety/control
split.

## 0D. Increment And Decrement

```jet
// CHANGE: keep `n++`/`n--` only as statements. Reject value-producing prefix
// and postfix forms.
// CURRENT:
old #= i++
new #= ++i

// PROPOSED:
i++             // ok, statement only
i--             // ok, statement only
i += 1          // still ok

// rejected:
// old :: i++   // too much C footgun
// new :: ++i   // same
// ++i          // no prefix form
```

Why: C-style value-producing increment is compact but bad for reasoning. Jet can
keep the ergonomic counter statement without inheriting prefix/postfix value
rules.

## 0E. Logic, Comparison, No `Any`

```jet
// KEEP.
ready :: queue.len() > 0 && !maintenance_mode

// KEEP: no general top type.
enum Input {
    Webhook(Intake)
    CsvRow([String])
}

fn handle<T: Renderable>(x: T) -> String { return x.render() }
dynamic: Data :: json.parse(raw)?
maybe: User? :: .None
```

Keep `&&`, `||`, `!`, standard comparisons, no comparison distribution, and no
general `Any`. Closed enums, generics, traits, `T?`, `T ? E`, and `Data` cover
the real cases with better reasoning.

## 1. Binding Sites

```jet
// CHANGE: use the Odin/Jai-style split because it is the cleanest scan law.
// ORIGINAL INTENT:
//   Make "new fixed name" and "new editable name" visually distinct.
//   Keep reassignment grepable. Avoid hidden local mutability.
//   That goal is right. `::`/`:=` does it without overloading `#`.
//
// CURRENT:
incident_id #= "inc-2026-071"
severity: Int #= 2
attempts := 0
attempts += 1

// OPTION A - keep current:
incident_id #= "inc-2026-071"      // fixed
retry_count := 0                   // editable
// Verdict: high visual split, but `#` means too many things and `#=` is alien.

// PROPOSED:
incident_id :: "inc-2026-071"      // fixed
severity: Int :: 2                  // fixed typed binding
attempts := 0                       // editable binding
attempts += 1

attempts = attempts + 1             // reassignment
// incident_id = "inc-2026-999"     // error: fixed binding

// GREP:
//   rg '::' Source examples docs
//       fixed binding sites
//   rg ':=' Source examples docs
//       editable local bindings
//   rg '[^<>:]=' Source examples      reassignment sites, approximate
```

Why: `::` visually reads as "definition/fact" because it is doubled, stable, and
non-assigning. `:=` visually reads as "binding cell" because it keeps the colon
from definition but ends with assignment. `=` remains change-existing-storage.

Adversarial check: this spends two binding operators. Worth it: fixed vs
editable binding is a semantic boundary that reviewers must see without type
inference or compiler expansion.

## 2. Local Mutability, No Hidden Mutation

```jet
// KEEP semantic split; CHANGE fixed spelling from `#=` to `::`.
// CURRENT:
total := 0
total += incident.cost

// BAD PROPOSAL FROM V1 FILE:
total :: 0            // compiler infers editable later, bad for grep

// PROPOSED:
total := 0            // author says editable now
total += incident.cost

limit :: 100          // fixed binding
// limit += 1         // compile error
```

Challenge: "magic mutability" would be beginner-friendly but expert-hostile.
Mutability is a reasoning boundary. Keep it explicit and grepable.

## 3. No Const Keyword; Comptime Is Phase

```jet
// CHANGE: delete `const`; `::` already means fixed binding.
// CURRENT:
const DEFAULT_REGION = "us-east"
comptime RETRY_LIMIT = 3
comptime {
    comptime ratio = RETRY_LIMIT / 3
}

// PROPOSED:
DEFAULT_REGION :: "us-east"
RETRY_LIMIT :: 3

#comptime {
    ratio :: RETRY_LIMIT / 3
    table :: build_lookup_table()
}

buffer: [U8#RETRY_LIMIT]          // use site demands compile-time value
```

Challenge: a separate `const` keyword creates a fake distinction. Jet needs two
facts: "can this binding change?" and "which phase produced this value?" `::`
answers the first. `#comptime` and use-site requirements answer the second.
`jet expand --facts phase` should show whether each `::` was runtime,
compile-time, or emitted by a derive.

## 4. Write Access Sigil

```jet
// CHANGE: replace common `~` writes with physically cheap postfix `!`.
// CURRENT:
fn deposit(account: ~Account, cents: Int) {
    account.balance += cents
}
deposit(~account, 500)

impl Account {
    fn freeze(~self) {
        self.locked = true
    }
}
account.freeze()

// OPTION A:
fn deposit(account: ~Account, cents: Int)
deposit(~account, 500)
// Verdict: visually good, physically bad for common code.

// OPTION B:
fn deposit(account: %Account, cents: Int)
deposit(%account, 500)
// Verdict: typeable enough, but `%` already suggests modulo/formatting.

// OPTION C:
fn deposit(account: !Account, cents: Int)
deposit(account!, 500)
// Verdict: best common-write candidate. Type side is prefix, value side is
// postfix, avoiding conflict with boolean `!flag`.

// PROPOSED:
fn deposit(account: !Account, cents: Int) {
    account.balance += cents
}
deposit(account!, 500)

impl Account {
    fn freeze(!self) {
        self.locked = true
    }
}
account!.freeze()

// Receiver form expands to the ordinary boundary call:
//   Account.freeze(account!)
```

Why: write access is common. `~` is visually nice but physically bad. `!` is
cheap, visible, and already reads as "pay attention." Prefix `!T` is type
position; postfix `x!` is value position. Prefix `!flag` remains boolean not,
so value-write grant is postfix only.

Adversarial check: `!` carries danger baggage from other languages. That is not
fatal here because Jet gives it one source-level authority meaning outside
boolean not: exclusive write access. No unwrap, no panic, no macro bang.

## 5. Move And Share Sigils

```jet
// KEEP capability sigils.
// CURRENT:
fn close(file: ^File) -> Receipt { ... }
fn cache(report: &Report) { ... }

receipt #= close(^file)
cache(&report)

// PROPOSED:
fn close(file: ^File) -> Receipt { ... }
fn cache(report: &Report) { ... }

receipt :: close(^file)
cache(&report)
```

Why: `!`, `^`, and `&` form a tight access family. They are short, grepable,
and visible at both type and call sites.

Challenge: sigils can become line noise if they are overloaded. Do not overload
them. `!` writes, `^` moves ownership, `&` shares. Nothing else.

## 6. Optional Capability Composition

```jet
// KEEP current capability composition.
// CURRENT:
fn fill(owner: ~User?, fallback: User) { ... }
fill(~maybe_owner, fallback)

// PROPOSED:
fn fill(owner: !User?, fallback: User) { ... }
fill(maybe_owner!, fallback)
```

Challenge: `!User?` is visually dense, but it is honest: optional value plus
exclusive write access. Formatter/LSP can lens it; source stays sigil.

## 7. Raw Pointer Tier

```jet
// CHANGE: item-level unsafe scopes remove repetitive local unsafe blocks.
use core.mem as mem

#unsafe("MMIO register address is from board manual section 4.2")
fn read_status(addr: Int) -> U32 {
    p: *U32 :: *addr
    return p.*
}

#unsafe("packet buffer is owned by the NIC ring for this callback")
module nic.rx {
    fn bytes(ptr: *U8, len: Int) -> [U8] {
        return mem.copy(ptr, len)
    }
}

// Build policy:
//   unsafe: deny | allow-listed | audit | allow
//   unsafe_allow: ["nic.rx", "board.mmio.read_status"]
```

Keep `*T`, prefix `*x`, postfix `p.*`. Do not mix this with write access.
Experts can mark an unsafe function/module once, then write raw-pointer code
without block spam. `jet audit unsafe` must still list every raw op, reason,
scope, and call path.

## 8. If Tables

```jet
// KEEP short `if subject == {}` table; tighten the grammar.
// CURRENT:
label #= if status == {
    200 -> "ok"
    400 | 404 -> "client"
    500..599 -> "server"
    else -> "other"
}

// PROPOSED:
label :: if status == {
    200 -> "ok"
    400 | 404 -> "client"
    500..599 -> "server"
    else -> "other"
}
```

Why: the short table form was chosen to push users away from repeated
`if`/`else if` ladders. Keep it. The rule must be strict: `if cond {}` is a
boolean branch; `if subject == { arms }` is a pattern/equality table. No other
`if expr op {}` forms.

## 9. If Table Guards

```jet
// CHANGE: guards become explicit `if` after the pattern.
// CURRENT:
action #= if event == {
    .Metric(name, value) && name == "disk" && value > 90 -> Action.Page
    .Metric(name, _) && name.starts_with("debug.") -> Action.Ignore
    else -> Action.Store
}

// PROPOSED:
action :: if event == {
    .Metric(name, value) if name == "disk" && value > 90 -> .Page
    .Metric(name, _) if name.starts_with("debug.") -> .Ignore
    else -> .Store
}
```

Why: pattern and condition are different jobs. `if` guard reads as "matched,
but only if."

Adversarial check: current "arms are Bool expressions" is uniform internally but
muddy in source. Keep the short table, but do not make arms arbitrary boolean
expressions.

## 10. Option And Result Constructors

```jet
// CHANGE: make Option/Result use the same dotted variant story as enums.
// CURRENT:
fn parse_event(raw: String) -> Incident ? ParseError {
    if raw == "" { return err(ParseError.Empty) }
    return ok(Incident.{ id: "inc-1", title: raw })
}

owner: User? #= value(user)
none: User? #= null

// PROPOSED:
fn parse_event(raw: String) -> Incident ? ParseError {
    if raw == "" { return .Err(.Empty) }
    return .Ok(Incident.{ id: "inc-1", title: raw })
}

owner: User? :: .Some(user)
none: User? :: .None
```

Why: Jet already has leading-dot variants. Use them. `ok`, `err`, `value`, and
`null` are four special words for the same sum-type idea.

Challenge: `null` is safe in Jet, but the word still drags a lifetime of wrong
instincts from other languages. `.None` is honest.

## 11. Propagation And Fallback Stay

```jet
// KEEP with constructor rename.
incident :: parse_event(raw)?
fallback :: parse_event(raw) ?? Incident.blank()
owner_email :: incident.owner?.email ?? "unassigned"

loop line in lines {
    event :: parse_event(line) ?? continue
    store(event) ?? break
}
```

Keep `?`, `??`, `?.`, `?? return`, `?? break`, `?? continue`. This family is
coherent. Only the success/failure constructor names need unification.

## 12. Struct Construction

```jet
// KEEP: dot construction is a good Jet-ism.
incident :: Incident.{
    id,
    title,
    severity: 2,
    owner: .None,
}

copy: Incident :: .{
    id,
    title,
    severity: 3,
    owner: .Some(owner),
}
```

Keep `Type.{}` and inferred `.{}`. It separates construction from function
calls and composes with enum named payloads.

## 13. Enum Construction And Patterns

```jet
// CHANGE only through `.Some/.None` and `if` tables.
// CURRENT:
state #= IncidentState.Acknowledged.{ user: owner }
if state == {
    .Open -> print("needs owner")
    .Acknowledged(user) -> print("owned by {user.name}")
    .Resolved(at, by) -> print("closed by {by.name} at {at}")
}

// PROPOSED:
state :: IncidentState.Acknowledged.{ user: owner }
if state == {
    .Open -> print("needs owner")
    .Acknowledged(user) -> print("owned by {user.name}")
    .Resolved(at, by) -> print("closed by {by.name} at {at}")
}
```

Keep leading-dot variants where expected. Make every sum-like thing use this.

## 14. Lists, Maps, Tuples, Fixed Lists

```jet
// KEEP with binding delta.
latency :: (p50: 18, p95: 92)
ids: [String] :: []
by_owner: [String, Int] :: ["alice": 3, "bob": 1]

top3: [Incident#3] :: pick_top.[a, b, c]
[first, second, third] :: top3

loop entry in by_owner {
    print("{entry.key}: {entry.value}")
}
```

Keep named-only tuples, `[T]`, `[K, V]`, `[T#N]`, map entries as `.key/.value`,
and `f.[...]`. These are compact and internally consistent.

## 14A. Custom Iteration And Indexing

```jet
// KEEP dual-tier hooks.
// Beginner API:
items := IncidentStore.open()
items.each((x: Incident) => print(x.id))
maybe :: items.get("inc-1")
items!.set("inc-1", incident)

// Expert API:
impl IncidentStore.Iterable {
    fn iter(self) -> IncidentIterator { ... }
}

impl IncidentStore.Index {
    fn get(self, id: String) -> Incident? { ... }
}

impl IncidentStore.IndexMut {
    fn set(!self, id: String, value: Incident) { ... }
}

store := IncidentStore.open()
loop incident in store {
    print(incident.id)
}
stored :: store["inc-1"]
store["inc-1"] = incident
```

Keep the dual-tier model. Beginners get method names; experts can opt into
syntax hooks. This respects tradition (`[]`, iteration) while making the
mechanism explicit and reviewable.

## 15. Slices And Ranges

```jet
// KEEP but document cost aggressively.
loop minute in 0..60 step 5 {
    poll(minute)
}

head :: incidents[0..9]          // inclusive copy, not a view
window :: text.slice(0..79)      // char positions, inclusive
```

Challenge: inclusive slicing is unusual and copy-by-default can hide cost.
Still keep it because one inclusive range law is beginner-friendly. Expert view
must show "copy" on slices in loops.

## 16. Loops

```jet
// KEEP.
loop {
    break
}

loop queue.has_work() {
    process(queue.next()?)
}

loop incident in queue.pending() {
    process(incident)
}

loop i := 0; i < retries.len(); i++ {
    run(retries[i])
}

outer :: loop customer in customers {
    loop incident in customer.incidents {
        if incident.id == target {
            break outer
        }
    }
}
```

Keep one `loop` keyword. Semicolons only inside counted-loop headers are ugly
but contained. The counted-loop index is explicitly editable because `i++`
changes it. `name :: loop` makes labels use the same fixed-binding visual law
as other named facts. Issue: it looks like binding a loop value. Rule: `name ::
loop` is a label form only; `loop` remains a statement, not a value. Do not add
`for`/`while`.

## 17. Functions, Labels, Defaults, Variadics

```jet
// KEEP.
fn notify(customer: Customer, message: String, urgent: Bool = false) -> Bool {
    return urgent
}

notify(customer, "disk almost full", urgent: true)

fn tag_all(prefix: String, tags: ...String) -> [String] {
    return tags.map((t: String) => "{prefix}:{t}")
}

tag_all("ops", ..."pager,db".split(","))
```

Keep checked labels without reorder. Good compromise: readable at call site,
one call order mechanically.

## 17A. Procedure Context And Reflection

```jet
// CHANGE: support an Odin/Jai-style procedure context, but make it inspectable.
// CURRENT:
fn parse(raw: String) -> Incident ? ParseError {
    return Incident.from_json(raw)
}

// PROPOSED:
fn parse(raw: String) -> Incident ? ParseError {
    trace_id :: context.trace_id
    proc :: #proc()
    caller :: #caller()
    return Incident.from_json(raw)
}

#context(allocator: arena, trace_id: incident_id) {
    incident :: parse(raw)?
}

// EXPANDED JET:
fn parse(raw: String, context: Context) -> Incident ? ParseError {
    trace_id :: context.trace_id
    proc :: ProcInfo.{ module: "pulseops.intake", name: "parse" }
    caller :: CallerInfo.{ file: "src/intake.jet", line: 42 }
    return Incident.from_json(raw)
}
```

Yes, support it. It is useful for allocators, logging, tracing, build/test
harnesses, caller location, and metaprogramming. The constraint: context is not
a hidden dependency swamp. It is a typed implicit parameter with a fixed name,
visible through `jet expand --facts context`, and scoped overrides use
`#context(...) {}`. `#proc()` and `#caller()` are compile-time-supplied values
in expression position — same plane as `#comptime`, same sigil.

## 18. Lambdas And Captures

```jet
// KEEP, with capture sigils if capture crosses an ownership boundary.
open :: incidents.filter((i: Incident) => i.state == .Open)
sender_task :: ^queue () => queue.flush()

fn apply(f: fn(Incident) -> Bool, item: Incident) -> Bool {
    return f(item)
}
```

Challenge: capture lists everywhere would be C++ noise. Inference is fine if
`jet expand --facts captures` shows the capture set and clone/move decisions.

## 19. Dynamic Trait Values

```jet
// CHANGE: dynamic dispatch must be visible in expert/public syntax.
// CURRENT:
fn render_all(items: [Renderable]) -> String {
    return items.map((x: Renderable) => x.render()).join("\n")
}

// PROPOSED:
fn render_all(items: [dyn Renderable]) -> String {
    return items.map((x: dyn Renderable) => x.render()).join("\n")
}

// Static dispatch stays generic:
fn render_one<T: Renderable>(x: T) -> String {
    return x.render()
}
```

Why: the important user-facing fact is dynamic dispatch, not Rust's `Box`
implementation detail. `dyn Renderable` means "value carries a vtable/runtime
type." If heap allocation is needed, `jet expand --facts allocs,dyn` shows it.

Adversarial check: this spends one keyword but buys honest dispatch. Do not use
plain `Renderable` for dynamic values; that hides cost and behavior.

## 20. Traits, Impl, Methods

```jet
// KEEP.
trait Renderable {
    fn render(self) -> String
}

struct Incident {
    id: String
    title: String

    fn render(self) -> String {
        return "{self.id}: {self.title}"
    }
}

fn Incident.url(self) -> String {
    return "/incidents/{self.id}"
}

impl Incident.Renderable {
    fn render(self) -> String {
        return "{self.id}: {self.title}"
    }
}
```

Keep `.` for external methods and `impl Type.Trait`. Do not reintroduce `::` for
paths; Jet already has one path/member operator and should not spend another.

## 20A. Type Aliases

```jet
// KEEP.
alias Fallible<T> = T ? Error
alias Rows<T> = [T]
alias Handler = fn(Request) -> Response ? Error

// Still rejected:
// alias CustomerId = String       // transparent alias, not a real domain type
type CustomerId :: String          // nominal: not assignable to plain String
```

Keep aliases transparent and generic/function-shaped. Do not let aliases become
fake nominal types; that breaks reasoning. Use `type Name :: Base` when the
domain needs a real type with the same representation.

## 21. Visibility

```jet
// CHANGE: visibility is a per-item compile-time fact; default is private.
// CURRENT:
#PubFile
struct IncidentDTO {
    id: String
    priv internal_score: Int
}

// PROPOSED:
@pub
struct IncidentDTO {
    id: String
    @priv internal_score: Int
}

fn handle(req: Request) -> Response { ... }        // private by default

@pub
fn intake(req: Request) -> Response { ... }
```

Why: visibility is a contract at the declaration's boundary — who may see it —
so it is an attribute. Attach it to the item; the answer is readable at the
item, with no ambient fence state and no `pub {}` block reindentation. Default
private means the export surface is exactly the `@pub` set: `rg '@pub'` is the
API audit.

Beginner rule: a `@pub` struct exports its fields; hide one with `@priv`. The
common DTO case costs one marker, not one per field.

Challenge: per-item markers repeat on large API surfaces. Accepted — an API
surface is the one place repetition is a feature: every exported item says so
where it stands, and a diff that changes visibility touches the item it
changes.

## 22. Declaration Markers

```jet
// CHANGE: contracts about a declaration use `@`; directives that change the
// compiler's behavior use `#`. Lowercase names, single contract bare,
// several bracketed.
// CURRENT:
#Pure fn score(i: Incident) -> Int { ... }
#Unsafe("reason") fn mmio(addr: Int) -> U32 { ... }
#State(Open) fn send(c: ~Connection, msg: String) { ... }
#[Codable, RenameAll(camel)]
struct Intake { ... }

// PROPOSED:
@pure fn score(i: Incident) -> Int { ... }

#unsafe("reason") fn mmio(addr: Int) -> U32 { ... }

@state(Open) fn send(c: !Connection, msg: String) { ... }

@[codable, rename_all(camel)]
struct Intake { ... }
```

Why: `@pure`, `@state(Open)`, and `@codable` state contracts the compiler
checks or fulfills at the declaration's boundary — call sites, encode/decode
capability, purity of the body against its signature. `#unsafe` is not a
contract; it changes what is legal inside the body. That is the eye-checkable
line: `@` promises, `#` permits/controls. Shape carries the rest: bare `@fact`
for one contract, `@[a, b(x)]` to group several.

## 23. Block Markers

```jet
// CHANGE marker family, keep scoped-block shape.
#unsafe("p points to a live Int") {
    print(p.*)
}

#caps(Fs, Io) {
    text :: fs.read("incidents.json") ?? ""
    print(text)
}

#grant(Fs) { caps ->
    write_report(caps)
}

#transact(order) {
    saved :: db.insert(incident)?
    order.on_rollback(() => db.delete(saved.id).drop("rollback cleanup"))
}

#bench "route lookup" {
    route(sample_incident())
}
```

Block markers are directives with scope: `#name(...) {}` changes compiler or
runtime behavior for the region — legality (`#unsafe`, `#suppress`), granted
capabilities (`#caps`, `#grant`), execution discipline (`#transact`,
`#bench`). None of them state a contract about a declaration, so none are
`@`.

## 24. Effects Position

```jet
// CHANGE: put effects after return type so signature reads left to right.
// CURRENT:
fn load(path: String) #(Fs) -> String ? IOError {
    return fs.read(path)
}

// PROPOSED:
fn load(path: String) -> String ? IOError #(Fs) {
    return fs.read(path)
}

@pure fn score(i: Incident) -> Int {
    return i.severity * 10
}
```

Why: params -> return -> effects is scan order. Current `#(Fs)` between params
and return interrupts the function type.

Adversarial check: this is a parser churn cost, but source readability wins.

## 25. Tests And Benchmarks

```jet
// CHANGE: tests/benchmarks are compiler/tool items under `#`.
// CURRENT:
#Test("routing escalates critical incidents") {
    incident #= Incident.{ id: "inc-1", title: "db", severity: 3, owner: null }
    require_eq(route(incident), Route.Page)
}

#Test fn score_is_non_negative(sev: Int) {
    require(score_for(sev) >= 0)
}

#Bench("route lookup") {
    route(sample_incident())
}

// PROPOSED:
#test "routing escalates critical incidents" {
    incident :: Incident.{ id: "inc-1", title: "db", severity: 3, owner: .None }
    require_eq(route(incident), .Page)
}

#test score_is_non_negative(sev: Int) {
    require(score_for(sev) >= 0)
}

#bench "route lookup" {
    route(sample_incident())
}
```

Why: tests and benches are not production declarations. `#test` and `#bench`
put them in the compiler/tooling plane without inventing more keywords.

Challenge: tests are daily syntax, so `#test` must remain formatter/LSP-native,
not a generic attribute bolted onto an anonymous block.

## 26. Docs And Doctests

```jet
// KEEP.
/// Calculates severity score.
/// ```jet
/// score_for(3) // => 30
/// ```
fn score_for(sev: Int) -> Int {
    return sev * 10
}
```

Keep `///` and fenced doctests. No syntax churn.

## 27. Display, Debug, Redaction

```jet
// KEEP.
struct Customer {
    id: String
    @redact token: String

    fn display(self) -> String {
        return self.id
    }
}

print("{customer}")
print("{customer:debug}")
```

Keep no-trait display hook. Debug redaction belongs on fields. Use `:debug`
inside interpolation; no sigil spent.

## 28. Nominal Types And Units

```jet
// CHANGE: replace unclear `distinct` with `type`; make units compiler-meta.
type CustomerId :: String

@numeric type Millis :: Int

#unit currency { usd, eur, gbp }

fn bill(id: CustomerId, amount: Usd) -> Invoice {
    return Invoice.{ customer: id.raw(), amount: amount.raw() }
}
```

`type CustomerId :: String` means nominal new type with the same representation
as `String`, not a transparent alias. `#unit` is a compiler item because it
generates a family of checked types, conversions, formatting, and dimensional
rules.

## 29. Tags, Taint, Typestate

```jet
// KEEP core model; contracts use `@`.
tag Reviewed

@tainted
raw_input :: req.body_text()?

@sanitizer
fn sanitize(s: String) -> String {
    return s.trim()
}

state Connection { Closed, Open }

@transition(Closed -> Open)
fn open(c: ^Connection) -> Connection { ... }

@state(Open)
fn send(c: !Connection, msg: String) { ... }
```

Challenge: tags, taint, typestate, and single-use are all declaration
contracts. One sigil, one shape, whether the contract rides a binding, a
function, or a type.

## 30. Single-Use, Must-Use, Discard

```jet
// KEEP model; contracts use `@`.
@single_use
struct Ticket { id: Int }

@must_use
fn persist(i: Incident) -> Receipt ? IOError { ... }

persist(incident).drop("best-effort local cache during webhook replay")

#suppress(MustUse) {
    persist(incident)
}
```

Keep `.drop("reason")`. Do not add `_ = expr` as a silent escape.

## 31. Published Schemas And Migrations

```jet
// KEEP model; contract uses `@`.
@published_schema
struct CustomerRecord {
    id: String
    display_name: String
    plan: String
}

migration CustomerRecord {
    rename name -> display_name
    add plan: String = "standard"
    remove legacy_id
    change cents: Int -> Usd via { (c) => Usd(c.to_float() / 100.0) }
}
```

Keep migration verbs. Schema evolution reads better as domain language than
sigil language.

## 32. Protocols

```jet
// CHANGE: generated API should be uniform send/recv over message variants.
// CURRENT:
protocol Payment {
    client -> server: Hello(version: Int)
    server -> client: Ack(session: Int)
    client -> server: Charge(cents: Int)
    server -> client: Receipt(id: Int)
}

fn drive(h: ^Payment.Client) {
    h1 #= h.Hello(1)?
    h2 #= h1.recv_Ack()?
    h3 #= h2.Charge(500)?
    h3.recv_Receipt()
}

// PROPOSED:
protocol Payment {
    client -> server: Hello(version: Int)
    server -> client: Ack(session: Int)
    client -> server: Charge(cents: Int)
    server -> client: Receipt(id: Int)
}

fn drive(h: ^Payment.Client) {
    h1 :: ^h.send(.Hello.{ version: 1 })?
    ack :: ^h1.recv(.Ack)?
    h2 :: ack.next
    h3 :: ^h2.send(.Charge.{ cents: 500 })?
    receipt :: ^h3.recv(.Receipt)?
}
```

Why: message names should be values, not generated method names. This makes
protocols feel like typed channels with typestate, not RPC codegen magic.

Adversarial check: generated methods are convenient but hide machinery. Uniform
`^h.send(.Msg)` / `^h.recv(.Msg)` is louder, but it exposes the linear state
transition. Expanded Jet can show the free-call form and next-state type.

## 33. Concurrency

```jet
// KEEP, with binding/access deltas.
use core.tasks as tasks

taskgroup g {
    fetch_incidents :: g.task { api.fetch_incidents()? }
    fetch_customers :: g.task { api.fetch_customers()? }
    all :: g.all([fetch_incidents, fetch_customers])
}

winner :: g.select().recv(alerts).after(time.ms(500)).wait()?

background :: tasks.spawn(^queue () => queue.flush())
^background.detach()
```

Keep structured taskgroups. Detached tasks stay visible escape.

## 34. Modules, Imports, Core

```jet
// KEEP.
use "./routing" as routing
use core.http.server as http
use core.encoding.json as json
use core.db as db

module scoring {
    @pub fn priority(i: Incident) -> Int {
        return i.severity * 10
    }
}
```

Keep quoted path vs bare module. Keep `core.*`. Do not add magic auto-imports
beyond `print`/`input`; LSP can write imports.

## 35. Generic Modules

```jet
// KEEP.
module Cache<K, capacity: Int> {
    pub fn capacity() -> Int {
        return capacity
    }
}

module IncidentCache = Cache<String, 1024>
```

Keep. This is the right Jai-like upgrade path: ordinary Jet source becomes a
template without becoming a macro language.

## 36. Package And Workspace Files

```jet
// CHALLENGE: current package file is too object-literal-ish.
// CURRENT:
payload: {
    name: "pulseops"
    version: "0.4.0"
    edition: "2026"
}

packages: {
    pulseops: { targets: [library, executable { entry: "src/main.jet" }] }
}

// PROPOSED:
package pulseops {
    version: "0.4.0"
    edition: "2026"

    targets: [
        library,
        executable { entry: "src/main.jet" },
    ]
}
```

Why: `package pulseops {}` is a named declaration. Current `payload:` plus
`packages:` makes package identity feel like data fields floating at top level.

Adversarial check: this is a bigger ecosystem syntax change. Worth considering
before package config ossifies.

## 36A. Lifecycle Entry Points

```jet
// NEW: `jet <verb>` resolves to `fn <verb>()` in the package entry when
// defined. Absent verb fn -> batteries-included default. Customization is
// plain Jet, in one language, organized however the user wants.

fn run() {                         // jet run, and the built binary
    http.serve(routes())
}

fn dev() -> Unit ? Error #(Fs, Net, Db) {   // jet dev
    db.seed("fixtures/incidents.json")?
    http.serve(routes(), port: 3000)
}

fn build(b: !Build) {              // jet build
    b.embed("assets/")
    b.target(.Wasm, modules: [compute])
}
```

Rules:

- `jet run` -> `fn run()`. `jet dev` -> `fn dev()`, falling back to `run()`
  under watch/hot-reload defaults. `jet build` -> `fn build(b: !Build)`,
  falling back to the default pipeline. New verbs only by ratification.
- There is no `fn main()`. The verb map has zero exceptions: the command IS
  the function name. `main` was C's crt0 linking convention; Jet's front end
  owns entry resolution, so the constraint that named it is gone. Writing
  `fn main` gets a teaching error with an auto-fix to `fn run`.
- `jet test` / `jet bench` stay on `#test` / `#bench` items. Tests are
  compiler-owned items scattered where the code lives, not an entry point.
- Verb functions are ordinary Jet: effects checked, `@pub` not required,
  `jet expand` and `jet audit` see them like any other code. No config DSL,
  no second build language, no untyped script hook.

Why: every ecosystem eventually grows dev servers, asset steps, codegen, and
deploy glue. Make/npm-scripts/build.zig all prove the demand; only Zig proved
the answer — the build language should be the language. Jet goes further:
zero ceremony until the user writes the verb function, then full language
power with the same safety, effects, and audit story as production code.

Adversarial check: `fn build` executing at build time is compile-phase user
code — it inherits the `#comptime(impure, ...)` reproducibility policy and
build-policy allowances. That is a feature: the build script is auditable by
the same machinery as everything else.

## 37. Dependencies, Refs, Build Profiles

```jet
// CHANGE: provider refs use call-like forms; no sigil spent on them.
deps: {
    textkit: "1.2.0"
    helpers: path("../helpers")
    parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" }
    raylib: c("system")
}

build: {
    release: Build.{ optimize: full, panic: unwind }
    ci: Build.{ optimize: basic, debug_info: true }
}
```

Keep `#` version pins — a version is a compile-time quantity attached to a
name, same law as `[T#N]`. Provider refs (`path(...)`, `git`, `c(...)`) are
ordinary call-shaped data; no sigil spent, and `@` never appears inside an
expression.

## 38. Encoding, Data, Codable

```jet
// CHANGE constructors, binding law, and marker family.
use core.encoding.json as json

@[codable, rename_all(camel), deny_unknown_fields]
struct Intake {
    @rename("incident_id") id: String
    @default("standard") tier: String
}

item: Intake :: json.decode<Intake>(text)?
tree: Data :: json.parse(text)?

if tree == {
    .Object(fields) -> print("fields {fields.len()}")
    else -> print("not object")
}
```

Keep `Codable`/`Encode`/`Decode`, `Data`, typed decode, and `core.encoding.*`.
Remove stale `Serialize` wording everywhere.

## 39. HTTP, DB, Crypto, Batteries

```jet
// KEEP: batteries are library APIs, not syntax.
use core.http.server as http
use core.db as db
use core.crypto as crypto
use core.log as log

fn intake(req: http.Request) -> http.Response ? http.Error #(Net, Db, Log) {
    text :: req.body_text()?
    item: Intake :: json.decode<Intake>(text)?
    db.exec("insert into incidents values (?, ?, ?)", [
        DbValue.Text(item.id),
        DbValue.Text(item.title),
        DbValue.Int(item.severity),
    ])?
    log.info("stored {item.id}")
    return .Ok(http.Response.json(.{ status: "stored" }))
}
```

Keep core breadth. Syntax should not grow for every domain.

## 40. CLI Args

```jet
// KEEP.
use core.args as args
use core.io as io

spec :: args.spec()
    .flag("verbose", "print extra detail")
    .option("output", "write report to FILE", "FILE")
    .positional("input", "incident CSV")

parsed :: spec.parse(io.args()) ?? panic(spec.help())
```

Builder API is fine. No CLI DSL.

## 41. Terminal Input

```jet
// REOPEN: `live` may belong in the compiler/tooling plane, not core syntax.
use core.term as term

// CURRENT:
live {
    key :: term.read_key()
    if key == {
        .Enter -> break
        .Char(c) -> print("typed {c}")
        else -> print("control key")
    }
}

// PROPOSED CANDIDATE:
#live {
    key :: term.read_key()
    if key == {
        .Enter -> break
        .Char(c) -> print("typed {c}")
        else -> print("control key")
    }
}
```

Do not ratify yet. If `live` is a tool/runtime mode, `#live` fits the
compiler-mode family. If it is ordinary control flow, keep `live`. The deciding
question: can it be implemented as library/runtime protocol without compiler
semantics?

## 42. Reactive UI

```jet
// KEEP typed values. Do not invent JSX.
use core.reactive as reactive
use core.ui as ui

enum View {
    Text(text: String)
    Box(children: [View])
}

tree :: reactive.signal(.Box.{ children: [.Text.{ text: "open incidents" }] })
leaves :: reactive.derived(() => flatten(tree.get()))

style :: Style.{
    color: Color.{ r: 255, g: 255, b: 255 },
    width: Length.{ value: 320.0, unit: .Px },
    height: Length.{ value: 48.0, unit: .Px },
}
```

Challenge: JSX would be marketable but wrong for Jet. Typed constructors give
autocomplete, refactoring, encode/decode symmetry, and no second language.

## 43. SIMD, Linalg, Measurement, Sketches

```jet
// CHANGE: open existing operators to user-defined types through fixed traits.
lane :: F32x4.splat(1.0) + F32x4(2.0, 3.0, 4.0, 5.0)
total :: lane.sum()

v :: Vec3(1.0, 2.0, 3.0)
xy :: v.xy
unit :: v.normalize()

latency :: Measurement(120.0, uncertainty: 2.5)
unique :: hll.new().add("inc-1").add("inc-2").estimate()

type Usd :: Decimal

impl Usd.Add {
    fn +(self, rhs: Usd) -> Usd {
        return Usd(self.raw() + rhs.raw())
    }
}

invoice_total :: Usd("19.99") + Usd("4.50")
```

Open operator overloading, but narrowly. Users may implement existing operator
traits for user-defined types. They may not invent operators, change precedence,
overload `=`, `::`, `:=`, `?`, `&&`, `||`, field access, or control-flow
operators. Effects and allocation in overloaded operators must be explicit in
the operator function signature.

## 44. Web Targets

```jet
// KEEP.
#target(Wasm)
module compute {
    @[wasm_export, pure]
    fn score(severity: Int) -> Int {
        return severity * 10
    }
}

#target(Js)
module browser {
    fn mount() -> Unit #(Browser) {
        render_app()
    }
}
```

Keep explicit target markers plus effects. Browser partitioning should never be
silent.

## 45. Rust And C FFI

```jet
// CHANGE: one extensible FFI item shape.
// CURRENT:
extern rust "base64@0.22" {
    fn encode(s: String) -> String = "base64::encode"
}

module c.raylib {
    fn draw_text(text: String, x: Int, y: Int, size: Int, color: Color) = "DrawText"
}

// PROPOSED:
#ffi rust "base64#0.22" as base64 {
    fn encode(s: String) -> String = "base64::encode"
}

#ffi c "raylib#system" as raylib {
    fn draw_text(text: String, x: Int, y: Int, size: Int, color: Color) = "DrawText"
}

use ffi.raylib as rl
```

Why: FFI should not have one Rust syntax and one C syntax. `#ffi <backend>
"ref" as <module> { ... }` can extend to C++, Swift, Zig, platform SDKs, or
generated bindings without multiplying surface forms. Source-level Rust interop
remains rejected; imported functions get Jet signatures and Jet diagnostics.

## 46. Comptime And Derive Generators

```jet
// CHANGE: stop emitting generated code as raw strings.
// CURRENT:
derive Label for T {
    info #= T.reflect()
    name #= info.name
    emit("impl $name { fn label(self) -> String { return \"$name\" } }")
}

// PROPOSED:
#derive T.Label {
    info :: T.reflect()
    name :: info.name
    emit jet {
        fn $T.label(self) -> String {
            return $name
        }
    }
}
```

What derive is: a compile-time generator that implements a known capability for
a type by emitting checked Jet source. `T.Label` matches `impl Type.Trait` and
avoids another `for` grammar. `$T` and `$name` are splices only inside generated
Jet.

Why: string-based codegen is the wrong long-term surface for a language that
owns diagnostics. `emit jet {}` is still a typed source fragment that re-enters
lexer -> parser -> sema; it is not a token macro.

Adversarial check: this is where Jai inspiration should matter. Metaprogramming
must show its work without making syntax user-definable.

## 47. Comptime Impurity

```jet
// CHANGE: impurity is a mode of comptime, not a separate wrapper.
// CURRENT:
#Impure("reads CI build number from environment at compile time") {
    comptime {
        build_number :: env.get("BUILD_NUMBER") ?? "dev"
    }
}

// PROPOSED:
#comptime(impure, "reads CI build number from environment at compile time") {
    build_number :: env.get("BUILD_NUMBER") ?? "dev"
}
```

Challenge: build-time I/O is a reproducibility hazard. Keep one `#comptime`
surface, make impurity an explicit mode, and require CLI/build-policy allowance
for impure compile-time execution.

## 48. Embedded, Layout, Uninit, Ref Fields, GC

```jet
// KEEP model; contracts use `@`.
use core.mem as mem
use core.gc as gc

@layout(c)
struct CHeader {
    len: U32
}

struct Owner {
    name: String
    @ref(owner) primary: Incident
}

fn buffer() {
    @uninit bytes: [U8#1024]
    bytes[0] = 1
}

root :: gc.Gc.new(Node.empty())
```

Keep opt-in GC and low-level memory gates. One contract shape covers items,
fields, and bindings alike.

## 49. Arenas And Regions

```jet
// KEEP with access/binding deltas.
use core.mem as mem

fn arena_parse(raw: String) {
    arena :: mem.Arena.new(capacity: 4096)
    node :: arena.alloc(parse_node(raw))

    region scratch {
        tmp :: arena.alloc(parse_node("scratch"))
        print(tmp.kind)
    }
}
```

Keep implicit regions plus explicit `region name` for expert cases.

## 50. Build And Tool Commands

```bash
# KEEP command surface, add transparency commands.
jet run src/main.jet
jet dev                 # fn dev() if defined, else run() under watch/reload
jet build               # fn build() if defined, else default pipeline
jet test .
jet bench .
jet debug src/main.jet
jet schema status
jet vendor --vendor-dir vendor
jet audit
jet build --sbom

# NEW:
jet expand src/main.jet --facts types,caps,effects,dyn,clones,drops,allocs
jet expand src/main.jet --stage derive --trigger Intake
jet fmt src/main.jet --materialize types,caps,effects
jet check src/main.jet --explicit dyn,clones,effects,caps
```

This is where "magic for beginners, machinery for experts" becomes real.
Generated Rust is not the right expert view; expanded Jet is.

## 51. LSP Fact Lenses

```jet
// NEW TOOLING VIEW, not source syntax.
fn intake(req: http.Request) -> http.Response ? http.Error #(Net, Db, Log) {
    text :: req.body_text()?
    item: Intake :: json.decode<Intake>(text)?
    return .Ok(http.Response.json(.{ status: "stored" }))
}

// LSP lens:
// effects: Net, Db, Log
// caps: req read
// hidden dyn dispatch: 0
// hidden clones: 0
// allocations: body String, JSON decode, response body
// generated: Intake.Decode, Intake.Encode
// drops: item, text
```

Challenge: without this, "beginner magic" is just hidden complexity. The LSP
must make compiler decisions inspectable at point of use.

## 52. Explicit Mode

```jet
// NEW: enterprise/audit source gate.
#explicit(Types, Caps, Effects, Dyn, Clones, Drops, Derives, Allocators)
module audit_intake {
    fn intake(req: http.Request) -> http.Response ? http.Error #(Net, Db, Log) {
        text: String :: req.body_text()?
        item: Intake :: json.decode<Intake>(text)?
        // generated derives and allocation facts must be materialized or accepted.
        return .Ok(http.Response.json(.{ status: "stored" }))
    }
}
```

`#explicit(...)` is not a second dialect. It is a refusal to compile hidden
facts in selected categories unless source or generated expanded Jet shows them.

## 53. Single Real Slice

```jet
// CHANGE: final proposed feel.
use core.http.server as http
use core.encoding.json as json
use core.db as db
use core.log as log

@[codable, rename_all(camel)]
struct Intake {
    incident_id: String
    title: String
    severity: Int
}

enum Route {
    Store
    Page(user: String)
    Drop(reason: String)
}

fn classify(i: Intake) -> Route {
    return if i.severity == {
        0..1 -> .Store
        2 -> .Page.{ user: "oncall-primary" }
        3..10 -> .Page.{ user: "incident-commander" }
        else -> .Drop.{ reason: "bad severity" }
    }
}

fn intake(req: http.Request) -> http.Response ? http.Error #(Net, Db, Log) {
    text :: req.body_text()?
    item: Intake :: json.decode<Intake>(text)?

    if classify(item) == {
        .Store -> {
            db.exec("insert into incidents values (?, ?, ?)", [
                DbValue.Text(item.incident_id),
                DbValue.Text(item.title),
                DbValue.Int(item.severity),
            ])?
            return .Ok(http.Response.json(.{ status: "stored" }))
        }

        .Page(user) -> {
            log.warn("paging {user} for {item.incident_id}")
            db.exec("insert into incidents values (?, ?, ?)", [
                DbValue.Text(item.incident_id),
                DbValue.Text(item.title),
                DbValue.Int(item.severity),
            ])?
            return .Ok(http.Response.json(.{ status: "paged" }))
        }

        .Drop(reason) -> {
            return .Ok(http.Response.bad_request(reason))
        }
    }
}
```

This is the target: visually grepable, type-directed, lower ceremony, honest
about dispatch, and still compact.

## 54. Diagnostics And Expanded Jet

```text
// KEEP invariant, CHANGE expert attachments.
// CURRENT CONTRACT:
//   rustc never speaks to users.
//   Jet diagnostics own what/why/fix.
//
// PROPOSED EXPERT DIAGNOSTIC ATTACHMENTS:
error[JET-BIND-FIXED]
  what: cannot assign to fixed binding `incident_id`
  why: `incident_id` was introduced with `::`, not `:=`
  fix: use `incident_id := ...` if later edits are part of the design

  facts:
    binding: fixed
    introduced: src/intake.jet:18
    attempted mutation: src/intake.jet:27

  expand:
    jet expand src/intake.jet --facts bindings,drops

// DERIVE FAILURE ATTACHMENT:
error[JET-DERIVE-DECODE]
  what: generated Decode for `Intake` cannot read field `severity`
  why: input key is named `sev`, but Jet expected `severity`
  fix: add `@rename("sev") severity: Int`

  generated:
    .jet/expanded/src/intake.Intake.Decode.jet:14
```

Diagnostics must stay beginner-readable by default, but experts should get
facts, expansion paths, and exact compiler decisions without reading generated
Rust. The expanded file is Jet source because Jet owns semantics.

## 55. Highest-Value Decision Cards

```text
1. D-BIND-REOPEN
   Choose `name :: value` fixed and `name := value` editable.
   Retire `#=`.

2. D-CAP-ERGONOMICS
   Keep source capability sigils:
   `!T`/`x!`, `^T`/`^x`, and `&T`/`&x`.

3. D-INCR-STATEMENT
   Keep `n++`/`n--` only as statements. Reject prefix forms and any use as a
   value.

4. D-IF-TABLE
   Keep `if subject == {}` dispatch tables.
   Add explicit `if` guards in arms.

5. D-SUM-DOT
   Replace `ok/err/value/null` constructors with `.Ok/.Err/.Some/.None`.

6. D-META-PLANE
   `@` states contracts on the declaration below (`@pure`, `@pub`,
   `@[codable, rename_all(camel)]`); never in a type or expression.
   `#` instructs the compiler: modes (`#unsafe`, `#comptime`), items
   (`#test`, `#ffi`, `#derive`), effects (`#(Fs)`), quantities
   (`[T#N]`, `pkg#1.2.3`), queries (`#caller()`). `$` splices in
   generated Jet. `~` reserved.

7. D-EFFECT-POSITION
   Move effect bounds after return type: `fn f() -> T #(Fs)`.

8. D-DYNAMIC-DISPATCH
   Public/expert dynamic dispatch uses `dyn Trait`.

9. D-TEST-BENCH-ITEMS
   Top-level `#test` and `#bench` items replace `#Test`/`#Bench`.

10. D-EMIT-JET
   `#derive T.Trait` emits typed Jet blocks, not raw strings.

11. D-TRANSPARENCY
    Add `jet expand`, `fmt --materialize`, `check --explicit`,
    LSP fact lenses, and `#explicit(...)`.

12. D-DIAGNOSTIC-FACTS
    Diagnostics can attach fact tables and expanded-Jet locations.
    They still never expose rustc as user-facing truth.

13. D-CONTEXT
    Support typed implicit `context`, scoped `#context(...) {}`,
    and `#proc()` / `#caller()` compile-time reflection values.

14. D-FFI-SHAPE
    Use one extensible `#ffi <backend> "ref" as <module> {}` shape.

15. D-UNSAFE-SCOPE
    Allow `#unsafe` on functions/modules plus build-policy audit modes.

16. D-OPERATORS
    Allow existing operator traits for user-defined types only.

17. D-NO-CONST
    Remove `const`; use `::` for fixed bindings and `#comptime` for phase.

18. D-WRITE-BANG
    Replace common `~T` / `~x` write access with `!T` / `x!`.
    Reserve `~` for rarer expert views.

19. D-PUB-ITEM
    Replace `#PubFile` / `pub {}` with per-item `@pub`; default private;
    a `@pub` struct exports fields, `@priv` hides one.

20. D-UNIT-META
    Use `#unit family { ... }` for compiler-generated unit families.

21. D-LIVE-REOPEN
    Decide whether live terminal mode is `live {}` control flow or `#live {}`
    compiler/runtime mode.

22. D-LIFECYCLE-VERBS
    `jet <verb>` resolves to `fn <verb>()`: run -> run, dev -> dev,
    build -> build. Entry point is `fn run()`; `fn main` is a teaching
    error with auto-fix. Absent verb fn -> batteries default. `jet test` /
    `jet bench` stay on `#test` / `#bench` items. New verbs by ratification.
```

These are real syntax moves. They are not polish. They are the difference
between "nice Rust/Go hybrid" and a language with its own coherent surface.
