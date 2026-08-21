# One callable, one arrow — reading first

**Status:** proposal. Every not-yet-ratified spelling below is marked *proposed*. Nothing here is law until its ballot is ratified.

## Executive summary

The owner set a new priority order for Jet's surface: the ability to **reason** about code comes first, the ability to **read** it comes second, and the ability to **write** it comes third. This audit swept the lambda, function, and control-flow surface against that order, probed the running binary, and swept the rest of the surface for the same trade. The finding: Jet's *choice* surface (arm tables, one-line guards, loops) already serves reading well — the owner's instinct to collapse `else if` chains into tables is confirmed by the evidence and stays. The damage is concentrated in the **callable** surface and in the **arrow glyph** itself.

One idea unifies the fixes: **the arrow means "yields", and every callable is one shape.** A function is a named lambda. Its interface — name, inputs, output, error, effects — sits complete and visible before the arrow. The body sits after the arrow and carries no interface facts. Today that one shape is split into seven costumes: a braced function has no arrow, a one-line function has one, a function with an effect row hides the arrow inside `:[IO]>`, a lambda always has one, a function type has none, a code argument has one, and a task block has none. A reader must learn seven rules for one concept. The proposal collapses them to one rule with one exception a beginner never meets.

The glyph itself is on the table. `:>` was picked for typing comfort; on the page it reads as noise, and the ratified ballot that unified it admitted losing the at-a-glance result cue. The slate offers `->` (lighter, standard) and `=>` (the `=` carries "outputs/binds", and it already rhymes with the ratified `impl Source => Target` conversion rail) as full respells, worked side by side so the choice is visual. The effect row unfuses from the arrow in every option: `:[IO]` becomes a standalone interface fact, which also fixes the false arrow that today dangles in type position (`f: fn(Int) Int :[]>`).

Three smaller reasoning hazards ride along, each probed live in the binary: a dispatch arm head that mixes distributed atoms with a guard (`48 | 45 && ready`) desugars invisibly and silently routes values to `else`; the subject shorthand accepts method-head chains in loops but rejects them in call slots (`.map(.len())` is two errors while `loop words if .len() > 1` runs); and parameter defaults use a shape (`name: Type = value`) that never appears in local bindings. The first two get tightening ballots. The third turns out to be lawful under the already-ratified "`::` defines, `=` fills" rule — the proposal teaches it and offers the respell anyway so the owner decides with the full picture.

What the ballots ask: pick the arrow glyph (D-ARROW-RESPELL1), adopt the one-callable shape (D-CALLABLE-ONE1), unfuse the effect row (D-EFFECT-ROW2), give lambdas full interface parity (D-LAMBDA-IFACE1), fence the mixed arm head (D-ARMHEAD-PAREN1), reconcile the subject shorthand (D-SUBJECT-COHERE1), and settle the default-value shape (D-DEFAULT-SHAPE1). Each ballot stands alone; any subset can be adopted. What does not change: arm tables, one-line forms, `::`/`:=` bindings, the memory sigils, `?`/`!`/`??`, trailing-value blocks, and L0507.

## The problem, briefly

Jet says "a callable" once in its semantics and seven times in its syntax. The same concept wears a different costume depending on body form, effect row, and position. Each row below is real, current, running syntax.

| # | The same concept | Today's spelling | Where | Arrow? | Defect |
|---|---|---|---|---|---|
| 1 | Function, one-expression body | `fn double(n: Int) Int :> n * 2` | `examples/features/basics/signature_shape.jet:2` | yes | — (baseline) |
| 2 | Function, braced body | `fn scale(self, factor: Float, clamp: Bool = false) Rect { … }` | `examples/features/basics/named_args.jet:15` | **no** | arrow vanishes when braces appear |
| 3 | Function, braced body + effects | `fn bump(n: Int) Int :[IO]> { … }` | `examples/features/basics/signature_shape.jet:4` | **fused** | arrow returns, but hidden inside the effect row |
| 4 | Lambda, expression body | `n :> n * 2` | `examples/features/functions/lambda_inference.jet:7` | yes | cannot state a return type at all (AST has no slot: `crates/jet-foundation/src/AST/expressions.rs:256-336`) |
| 5 | Code argument | `twice(() :> { print("HI") })` | `examples/features/syntax/trailing_block.jet:8` | yes | — |
| 6 | Function type | `f: fn(Int) Int :[]>` | `examples/features/effects/effect_levers.jet:13` | **dangling** | a body arrow in type position, where no body can follow |
| 7 | Task block | `task { frozen.value }` | `examples/features/concurrency/freeze_capture.jet:13` | **no** | zero-param lambda in yet another costume |

Seven costumes, three arrow states (present, absent, fused), and one capability hole (row 4). The reader pays the bill: the arrow's presence tells them nothing reliable about the construct, so they re-derive the rule at every site.

The glyph compounds it. `:>` sits in a four-member colon family — `::` (define), `:=` (mutable define), `:>` (body), `:[IO]>` (body with effects) — plus `:` for types, labels, and interpolation selectors. The brevity sweep rated this colon rail the densest context-switch cluster on the surface. The ratified arrow ballot (`c0ma0xb6`, D-ARROW-UNIFY1=B) chose one arrow and accepted "loses the at-a-glance result cue" as a cost; the owner now reports the glyph also reads as sloppy. Both costs are real and both are fixable by a pure respell — the unification itself was right and stays.

Three adjacent hazards, each verified against the running binary today:

```jet
// PROBE 1 — ran today. What does grade(46, false) return?
fn grade(n: Int, ready: Bool) Int {
    if n < {
        16 :> 0
        48 | 45 && ready :> 2      // desugars to ((n < 48 || n < 45) && ready)
        else :> 15
    }
}
// grade(46, true) = 2, grade(46, false) = 15 — the value silently falls to `else`.
// The arm text is not the predicate that runs (D-IFDIST1=A).
```

```jet
// PROBE 2 — ran today. The same shorthand, two verdicts.
active :: loop words if .len() > 1 :> .len()   // runs: implicit subject accepts a method head
lens :: ["ab", "cde"].map(.len())              // E0302 + E0104: subject-call demands a member head
```

```jet
// PROBE 3 — the two declaration shapes a beginner must learn.
version :: Float{0.1}                          // local: type rides the value (D-BIND-BARE1=A)
fn connect(host: String, /, *, timeout seconds: Int = 30) String { … }
                                               // parameter: annotation + `=` fill (D-APILABEL1=A)
```

## The proposal

### The reading law

One test governs every element below: **all facts a reader needs to reason about a call sit visibly at the interface, before the arrow; the arrow always means "yields"; the body never smuggles interface facts.** Reason first, read second, write third. Where a short form survives, it survives because it *aids* reasoning (one responsibility per line, no zoom-in/zoom-out), never because it saves keystrokes.

### One callable shape *(proposed — D-CALLABLE-ONE1)*

One production covers every callable. Square brackets mark what each rung may omit:

```
[fn name] ( params ) [Return] [! Error] [:[Effects]] -> body
body = expression | { statements … trailing-value }
```

A function is this shape with `fn name` in front and interface types written. A lambda is this shape with the front matter omitted and types recovered from the call slot. The arrow appears exactly when the callable yields something — which is why the beginner's first function needs none:

```jet
// Rung 0 — beginner types nothing extra. Unit function, no result, no arrow. UNCHANGED.
fn greet() {
    print("hello")
}

// Rung 1 — a result appears, so the arrow appears. One-liners: today vs proposed.
fn double(n: Int) Int :> n * 2        // today
fn double(n: Int) Int -> n * 2        // proposed

// Rung 2 — braced body with a result. Today the arrow vanishes; proposed it stays.
fn scale(r: Rect, f: Float) Rect {    // today: `Rect {` — type? literal? the space decides
    Rect{width: r.width * f, height: r.height * f}
}
fn scale(r: Rect, f: Float) Rect -> { // proposed: the arrow ends the interface, then the body
    Rect{width: r.width * f, height: r.height * f}
}

// Rung 3 — full interface stack: result, error, effects, in one fixed order.
fn poll(url: String) Response ! NetError :[IO] -> {   // proposed
    fetch(url)? "polling {url}"
}
fn poll(url: String) Response ! NetError :[IO]> {     // today: effects swallow the arrow
    fetch(url)? "polling {url}"
}
```

The same shape *is* the lambda. Nothing to relearn — remove the name, drop the types the slot already knows:

```jet
nums.map(n -> n * 2)                        // proposed lambda: slot supplies the types
nums.reduce(0, (acc, n) -> acc + n)         // two parameters
retry(3, () -> fetch())                     // code argument (D-TRAILBLOCK2=A shape, respelled)
sorted :: words.sort_by((w: String) -> w.len())   // annotated when the author wants it visible
```

And the function type is the same shape with the body omitted — no body, no arrow, no dangling glyph:

```jet
f: fn(Int) Int          // unchanged
g: fn(Int) Int :[]      // proposed: explicit empty effect row, no arrow in type position
h: fn(*, force: Bool) Int    // zones stay (D-APILABEL1=A)
```

Why this beats today: the reader gets one invariant — *see an arrow, something is yielded; see the arrow's left, that is everything the compiler guarantees; see its right, that is how.* The `Rect {`-vs-`Rect{` squint at rung 2 dies. Row 3's fused arrow dies. Row 6's dangling type arrow dies. The costume count drops from seven to one shape plus one beginner exception (unit functions), and `task { … }` is documented as exactly this shape with an empty interface.

Expert exits: `jet fmt` performs the whole migration mechanically (arrow insertion is syntax-directed, zero judgment); `jet explain E0068` teaches the shape at the exact error site; no project switch is offered because two callable grammars is precisely the state this ballot deletes (I8).

### The arrow itself *(proposed — D-ARROW-RESPELL1, amends D-ARROW-UNIFY1=B spelling)*

D-ARROW-UNIFY1=B was right that one arrow beats three. The respell keeps the unification and changes only the ink. The same program in all three candidates — the choice is visual:

```jet
// Candidate A: `->`  (lighter line, the standard "maps to" of Rust/Swift/Haskell)
fn label(n: Int) String -> {
    if n == {
        1 -> "one"
        2 | 3 -> "a few"
        else -> "many"
    }
}
names :: loop u, users if u.active -> u.name
m :: if a > b -> a else -> b
if ready -> run() else -> wait()
```

```jet
// Candidate B: `=>`  (the `=` says "outputs/binds"; rhymes with impl Source => Target)
fn label(n: Int) String => {
    if n == {
        1 => "one"
        2 | 3 => "a few"
        else => "many"
    }
}
names :: loop u, users if u.active => u.name
m :: if a > b => a else => b
if ready => run() else => wait()
```

```jet
// Candidate C: `:>` (status quo)
fn label(n: Int) String :> {
    if n == {
        1 :> "one"
        2 | 3 :> "a few"
        else :> "many"
    }
}
names :: loop u, users if u.active :> u.name
m :: if a > b :> a else :> b
if ready :> run() else :> wait()
```

The honest tradeoffs, stated once:

| | `->` | `=>` | `:>` |
|---|---|---|---|
| Visual weight | lightest | heavier, but `=` reads "outputs" | reads as a typo of `:` + `>` |
| Prior-art muscle memory | Rust fn/Haskell/Swift/Python return hints | JS/C#/Scala lambdas, Rust match arms | none |
| Collision with live Jet | none found | none — and it *matches* the ratified `impl Source => Target` rail (D-FAIL-CONV1=A) | n/a |
| Colon-family crowding | leaves `::`/`:=`/`:` | leaves `::`/`:=`/`:` | stays inside the crowd |
| Follow-up if chosen | respell `impl Source => Target` to `impl Source -> Target` for one-arrow purity, or accept the rhyme break | none — conversion rail already agrees | spec/comment drift cleanup only |

In every candidate the effect row unfuses (next element), so no option needs a `-[IO]->` / `=[IO]=>` monster — the exact form the original ballot called unreadable.

### The effect row stands alone *(proposed — D-EFFECT-ROW2, amends D-SHAPE8=A)*

Effects are an interface fact, so they sit with the other interface facts — between the error and the arrow — instead of deforming the arrow:

```jet
fn bump(n: Int) Int :[IO]> { … }        // today: one token, two jobs
fn bump(n: Int) Int :[IO] -> { … }      // proposed: fact, then arrow

fn run() :[IO]> { … }                   // today: unit fn forced to carry an arrow by its effects
fn run() :[IO] { … }                    // proposed: unit fn stays arrowless, effects still visible

pure_add: fn(Int, Int) Int :[]          // proposed: type position carries the fact, no fake body arrow
```

This is the piece that makes the unit-function exception clean: today `fn run() :[IO]> { … }` *must* write an arrow only because the effect row physically contains one. Unfused, the rule "arrow iff something is yielded" holds with no asterisk.

### Lambdas get the whole interface *(proposed — D-LAMBDA-IFACE1)*

Rust's evidence holds here: closures elide types because the slot supplies them; interfaces stay explicit. Jet keeps that default. The change is capability, not ceremony: today a lambda physically cannot state a return type or effect row (`crates/jet-foundation/src/AST/expressions.rs:256-336` has no slot). Under the one shape it can, because it is the same grammar:

```jet
nums.map(n -> n * 2)                             // rung 0: slot knows everything — unchanged behavior
parse :: (raw: String) Int ! ParseError -> {     // rung 2: stored lambda, no slot, full interface
    raw.trim().to_int()? "parsing {raw}"
}
```

Rule: annotations are *allowed* everywhere, *required* only where no expected type exists (exactly today's inference law, D-LAMBDA-INFER1). No upper rung changes what the lowest rung does.

### Choice is already right — keep it, fence one trap *(D-ARMHEAD-PAREN1 proposed; everything else stays)*

The owner's instinct — a table beats an `else if` ladder — is confirmed from three directions: the surface-frequency audit (branching is 45–95% of real code; tables keep arms scannable), the nesting study (275 participants, less nesting = measurably faster comprehension), and Go's one-construct philosophy. L0507, the one-line forms, guard tables, and dispatch tables all stay.

One arm-head form fails the reading law, per PROBE 1: mixing distributed atoms with boolean guards hides the real predicate. The fence is minimal — parentheses become required exactly when atoms and `&&`/`||` mix in one head:

```jet
48 | 45 && ready :> 2          // today: legal, desugars to ((n < 48 || n < 45) && ready)
(48 | 45) && ready -> 2        // proposed: the grouping the compiler sees is the grouping you see
```

Pure atom arms (`301 | 302`), pure predicate arms (`code >= 500`), and pattern+guard arms (`.Err(e) && e.fatal`) are untouched — each is already honest on the page.

### Subject shorthand: one rule, not two *(proposed — D-SUBJECT-COHERE1)*

PROBE 2 shows the false rhyme: the loop's implicit subject accepts `.len() > 1` (method head), while the call-slot shorthand accepts only member heads, so `.map(.len())` fails with two errors that never name the real rule. Two sub-decisions:

- **Widen or narrow.** Either method-head chains become legal in call slots too (`.map(.len())` works — one rule everywhere), or loops narrow to member heads (consistent but removes a working form). Widening is recommended: it deletes a rule instead of adding one, and E0302/E0104 stop firing on a form every reader expects to work.
- **Nesting fence.** Kotlin's style law is the tested boundary: implicit subjects in *nested* shorthand scopes stop being obvious. A lint (not an error) when `.member` shorthand appears inside another shorthand scope, naming the explicit-binding rewrite.

### Defaults, defines, and fills *(D-DEFAULT-SHAPE1 — decision, with an honest keep option)*

The owner flagged the shape split of PROBE 3. The sweep found it is not lawless — it is the ratified fill law wearing two contexts (D-CHOOSE-FNBODY1=A: *`::`/`:=` define a new thing; `=` fills a slot that already exists*):

```jet
version :: Float{0.1}       // defines a name; the type rides the value
n := 100                    // defines a mutable name
timeout seconds: Int = 30   // parameter: the slot exists (declared by `name: Type`), `=` fills its absent-case
port: Int = 3000            // field: same fill
n = 200                     // statement: refills an existing := slot
```

Read this way, the split carries information: *see `::`, something new exists; see `=`, an existing slot gets a value.* An interface must show its types (`name: Type`), a local must not repeat what its value shows (`Float{0.1}`) — so the two contexts genuinely differ, and the current spelling marks the difference. That is the keep case, and it is strong.

The unify options, priced honestly:

| Option | Shape | Cost |
|---|---|---|
| A — keep, and teach the fill law | `timeout seconds: Int = 30` | zero migration; the law gets one glossary line in the book and in `jet explain` |
| B — defaults adopt the binding glyph | `timeout seconds: Int := 30` *(proposed)* | `:=` stops meaning "mutable define" uniformly; a parameter default is neither mutable nor a local define — the rhyme would lie |
| C — locals adopt annotation | `port: Int :: 3000` *(proposed)* | reopens ratified D-BIND-BARE1=A, which retired exactly this; re-adds a second local shape (I8 pressure) |

Recommendation: A. This is the one place the audit found the brevity-era design *already* reasoning-first; the inconsistency dissolves once the law is stated where beginners meet it.

## The final vision

The same small program, complete, today and under the recommended slate (shape ballot + `->` candidate; swap `=>` mentally via the candidate block above — the structure is identical):

```jet
// ── TODAY ────────────────────────────────────────────────────────────
fn parse_age(raw: String) Int ! ParseError {
    raw.trim().to_int()? "parsing {raw}"
}

fn describe(age: Int) String :> {
    if age < {
        13 :> "kid"
        18 | 16 :> "teen"
        else :> "adult"
    }
}

fn audit(users: [User]) :[IO]> {
    names :: loop u, users if u.active :> u.name
    labels :: names.map(n :> n.to_upper())
    loop label, labels :> print(label)
}

fn run() {
    audit(load())
}
```

```jet
// ── PROPOSED ─────────────────────────────────────────────────────────
fn parse_age(raw: String) Int ! ParseError -> {
    raw.trim().to_int()? "parsing {raw}"
}

fn describe(age: Int) String -> {
    if age < {
        13 -> "kid"
        (18 | 16) -> "teen"
        else -> "adult"
    }
}

fn audit(users: [User]) :[IO] {
    names :: loop u, users if u.active -> u.name
    labels :: names.map(.to_upper())        // subject shorthand, now one rule
    loop label, labels -> print(label)
}

fn run() {
    audit(load())
}
```

The grammar the reader carries after this slate, whole, in one table:

| See | Know |
|---|---|
| `fn name(…)` | a named callable; its interface is everything left of the arrow |
| `(…) … -> body` without `fn` | the same callable, anonymous; missing types live in the slot |
| `->` | something is yielded; left = guarantee, right = how |
| no arrow, braces | statements run for effect; nothing is yielded |
| `! E` | it can fail with `E` |
| `:[…]` | its effect ceiling, in types and declarations alike |
| `::` / `:=` | a new name exists (immutable / mutable) |
| `=` | an existing slot gets a value (default, refill) |
| `if … { head -> body }` | an ordered table; first true head wins; parens in a head group exactly as the compiler groups |

## What this unlocks

- **Agents.** One callable grammar collapses the repair space: E0068/E0335-class mistakes now admit one obvious fix, and the arrow-presence rule is checkable per token instead of per context (repair determinism, quantity *e*; context economy, quantity *d* — annotated lambdas stop forcing named-function rewrites mid-loop).
- **Teaching.** The book's callable chapter becomes one page: one shape, three rungs. The seven-costume table above is today's page count.
- **Tooling.** `jet fmt` migration is fully mechanical; signature help, inlay hints, and `jet doc` render one shape everywhere including stored lambdas with full interfaces.
- **Extremes.** Trivial one-liners keep their one line (`fn double(n: Int) Int -> n * 2`); critical simulation code gains stored lambdas with declared error and effect rows — auditable callbacks with no named-function detour.

## What stays

- **Arm tables, guard tables, one-line `if`/`loop` forms, L0507** — confirmed by frequency and nesting evidence; the owner's table instinct is the ratified design.
- **`::` / `:=` / bare bindings (D-BIND-BARE1=A)** — locals not repeating slot-known types is the right economy; prior art (Rust closures) agrees.
- **Memory sigils `^ & ~` (D-MEM1)** — visible ownership at both ends is reasoning-first brevity; the sweep's strongest keep.
- **`?` / `!` / `??` failure surface (D-ERRSIGIL1=A, D-ERR-DECON1=A)** — recently ratified, reasoning-honest; only its docs gain the context rules the sweep flagged.
- **Trailing-value blocks (D-BODY-LAST1=B)** — the signature states the type, so the tail is checked, not guessed.
- **Explicit code arguments (D-TRAILBLOCK2=A), no general pipe (D-SHAPE-PIPE1=C), no `match` keyword (I8)** — all reasoning-first walls, kept on purpose.

## Decisions for the owner

| Ballot | Question | Options (recommended first) | Amends |
|---|---|---|---|
| D-ARROW-RESPELL1 | Which glyph is the one arrow? | `->` / `=>` / keep `:>` | D-ARROW-UNIFY1=B (spelling only), D-LOOP-STMT-ARROW1=C, D-SIG-SHAPE1=B spellings |
| D-CALLABLE-ONE1 | One callable shape, arrow iff yield? | adopt / arrow-always (uniform, unit fns pay) / keep split | D-SIG-SHAPE1=B, D-BODY-ARROW1=B |
| D-EFFECT-ROW2 | Unfuse `:[E]` from the arrow? | unfuse / keep fused | D-SHAPE8=A |
| D-LAMBDA-IFACE1 | Lambdas may write return type + error + effects? | adopt / params-only status quo | S46/S47 |
| D-ARMHEAD-PAREN1 | Parens required when atoms mix with `&&`/`||`? | require / keep + document | D-IFDIST1=A |
| D-SUBJECT-COHERE1 | Method-head chains in call slots + nesting lint? | widen + lint / narrow loops / keep split | D-SUBJECT-CALL1=A, D-LOOP-SUBJECT1=A |
| D-DEFAULT-SHAPE1 | Default-value shape | keep + teach fill law / `:=` defaults / annotated locals | D-APILABEL1=A or D-BIND-BARE1=A |

## Implementation shape

- **A — internal, no surface change.** Add the lambda result/error/effect slots to the AST and sema behind the current grammar; land the drift cleanup card (spec prose, `explain` text, example comments, and fixtures still showing `->`/`=>`/`:> Int ::` from before D-ARROW-UNIFY1=B — that cleanup is owed under every outcome including "keep `:>`").
- **B — ratified spellings land once.** Arrow respell + shape rule + effect unfuse ship as one `jet fmt`-driven migration of the whole corpus (examples, tests, snapshots, docs), one coherent change, replaced forms deleted (greenfield law).
- **C — surface deltas.** Subject-shorthand widening, arm-head parens (with its E-code and fixit), and any defaults respell each land as their own vertical slice with I9 parity and fresh snapshots.
