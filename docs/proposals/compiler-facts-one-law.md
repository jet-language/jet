# Compiler-held facts: one law, one ledger

Status: proposal, 2026-08-07. Owner decisions: seven ballots on the companion card.
Scope: the capstone over four ratified rethinks — type system v2 (knowledge), authority
(rights), concurrency (state, duty, reach), and memory v5 (ownership) — plus every
smaller fact the compiler holds: flow narrowing, uninit tracking, purity, attribution,
config facts, marker rows, provenance, maturity.

## Executive summary

**The finding.** In the last month Jet ratified four big rethinks. Each one found the
same shape: the compiler holds facts about the program, folds them through the code,
and erases them before codegen. Type v2 named the facts about values "knowledge" and
gave them a conservation law. Authority named the facts about scopes "rights" and gave
them an attenuation law. Concurrency named three facts about work — state, duty, reach.
Memory v5 gave places read, write, and take windows. Four planes, four laws, four
vocabularies. The proposals even started to merge on their own: D-META-REG1 already
ratified one registration table behind markers, planes, rights, and build facts, and
D-CONC-UNIT1 already put work facts on the type machinery. The convergence happened.
Nobody wrote it down as one thing.

**The idea.** All of it is one mechanism: **the compiler holds facts, and every fact
obeys one law — it may move toward safety silently, and every move away from safety is
one written word, recorded where an auditor can find it.** Knowledge conservation,
rights attenuation, policy tightening, flow narrowing, duty discharge, and the
ownership sigils are the same law with the safe direction pointed the right way for
each plane. The escape hatches — `#Unsafe("...")`, `#Scrub(Tag)`, `approx(x)`,
`.Force`, `.drop("reason")`, `detach` — were always one family: the release lever on
one ratchet.

**Why now.** Implementation cards #1517–#1579 are about to build these planes. Today
they would build four law engines, six flow-fact stores, and five vocabularies. Two of
the six stores already have dead join code (`State.rs:712`, `Sema/mod.rs:44`) — the
fragmentation is not a style problem, it is unsound branches waiting to be found.
Stating the one law now means the cards build one substrate once.

**The payoffs, concretely.**
- One sentence teaches the whole safety story: "the compiler learns facts for you,
  never forgets one behind your back, and every exception is one written word."
- One audit: `jet inspect gates` lists every place the program stepped off the safe
  path — unsafe blocks, taint scrubs, precision drops, forced settings, dependency
  grants — instead of four commands and a diff.
- One read surface: every registered fact is a typed value you can read; the
  string-smuggled reflection fallback and the phantom-type census (about eighteen
  enums that appear in errors but can never be written or built) die.
- One flow engine: moved, uninit, state, narrowing, and taint facts get one store and
  one join rule, so branch merges stop being five hand-written near-copies, two of
  them dead.
- One vocabulary: "tighten", "attenuate", "narrow", "conserve", and "erase" collapse
  into one word in diagnostics and docs.

**What the ballots ask.** Seven direction-level choices: ratify the one-way law
(LAW1); one vocabulary (WORD1); one gate ledger (GATE1); one fact-read surface
(READ1); home the orphan facts (HOME1); ratify the ownership wall (OWN1); one
flow-fact engine (FLOW1). Each stands alone.

**What does not change.** Every ratified spelling stays: `&`/`^`/`~`, `=[Net]=>`,
`tag`/`state`/`effect`, `distinct Int(0..10)`, `#Unsafe("reason")`, `$build.*`,
`task` / `task.all` / `task.race`. Every wall stays: no macros, no dependent types, no HKT, no top
type, comptime never creates types (S26), facts classify and erase and never dispatch
(D-FACTMODEL1), zero cost, I1–I9. This proposal deletes vocabulary, dead code, phantom
types, and duplicate engines. It adds no keyword and no annotation to any common case.

## Glossary

- **Fact** — one thing the compiler has proved about the program: a value's range, a
  scope's rights, a task's duty, a place's write window, a build setting.
- **Subject** — what a fact is about. Four kinds: a value, a place (a name plus its
  projection), a scope, and the build.
- **Plane** — one family of facts with its own combination rules (type v2's word,
  kept). Ranges are a plane; rights are a plane; duty is a plane.
- **Safe direction** — the way a fact can move without making any claim less true:
  learning more about a value, giving up a right, discharging a duty, closing a write
  window.
- **Gate** — one written word that moves a fact the other way, at the site, on the
  record. `#Unsafe("...")` is the oldest gate.
- **Ledger** — the one place every gate lands, so an auditor reads one list.
- **Prover** — an engine that establishes facts: the interval prover, the effect
  fixpoint, the borrow checker. Provers are internal; planes are the product.

## The one idea

**The compiler holds facts about values, places, scopes, and the build. One law
governs all of them: a fact moves toward safety silently; every move away from safety
is one written word, recorded for audit. At runtime, no fact remains.**

For a beginner this is invisible. Jet infers the facts — what a function uses, what a
value holds, when a task must be joined — and the defaults are the safe ones. The
beginner meets the law only as magic: exact math, no data races, no forgotten task,
one build line that says what the program may do.

For an expert every fact is a first-class citizen: nameable, readable as a typed
value, declared in readable prelude source, and every escape they write lands in one
ledger they can hand to a reviewer. "What does the compiler know here, and where is
every exception" is one question with one answer.

## Evidence: one law wearing five coats

Every ratified rule below is the same law. The left column is the ruling; the right
column is what the law calls it.

| Ratified rule | The law, in that plane |
|---|---|
| "Knowledge is never lost silently" — spelled demotions `approx`, `from_*_rounded`, `.raw()` (D-TYPE2-EXACT1) | value facts: losing certainty is the away-move |
| "Rights only shrink as scope nests; every re-widening is a written, audited gate" (D-AUTHORITY-MODEL1) | scope facts: gaining power is the away-move |
| "Package policy may only tighten safety" (D-PACKAGE-POLICY-SCOPE1) | same law at package scope |
| "Safety facts only tighten, at every scope and layer"; `.Force` is the audited exception (D-CONF-MERGE1) | same law on the build |
| `x != None` narrows `T?` to `T`, silently and free (D-FLOWTYPE1) | value facts: gaining certainty is the free move |
| A bound task handle must be joined or detached; dropping it is an error (D-CONC-JOIN1) | duty facts: discharge is the free move, abandonment needs the word |
| `#SingleUse` values need `.drop("reason")` to die unused | same duty law, older plane |
| Bare access reads; `&` opens a write window; `^` takes; `~` copies (D-MEM1, D-SHAPE-PLACE1) | place facts: each grade of power costs one written sigil |
| "No failure is lost, reworded, or rerouted silently; a route changes only at a spelled boundary" (D-FAIL-MODEL1) | report facts: same law |
| Taint spreads automatically; only `#Scrub(Tag)` removes it (D-TAG-SURFACE1) | value facts: the compiler gains suspicion free, you spell its removal |

The corpus never says this once. It says it ten times in five vocabularies: *tighten*
(policy, config), *attenuate* (rights), *narrow* (flow), *conserve* (knowledge),
*erase* (facts). Grep proof: "monotone" appears nowhere in the specs and
proposals; the words never meet.

And the machinery census (file:line from the code sweep):

| # | Shadow piece | Home | Defect |
|---|---|---|---|
| 1 | Two narrowing lattices | the `Tighten` combine in `Policy.rs:47`, `effect_covers` in `Effects.rs:277` | one narrowing rule, written twice (authority already claims this) |
| 2 | Unjoined-task check | `CheckerOwnership.rs:4141` vs `:4173` | a comment admits it "mirrors the `#SingleUse` check"; one warns, one errors |
| 3 | Six flow-fact stores | `moved` (`mod.rs:1253`), `uninit` (`:1375`), `StateCtx` (`State.rs:246`), `TaintCtx` (`Taint.rs:33`), narrow-by-shadowing (`switches.rs:144`), `ViewFactGraph` (`mod.rs:882`) | six shapes for "per-binding fact, updated by flow" |
| 4 | Dead joins | `State.rs:712` `join_after`, `mod.rs:44` `UninitState::merge_paths` | both `#[allow(dead_code)]` — typestate and partial-init have **no live branch merge** |
| 5 | Phantom fact enums | `Capability`, `State`, `TaskGroup`, `PolicySetting`, ~14 more (type-unification audit :117–129) | named in errors, accepted in signatures, impossible to build — `fn f(x: Capability)` compiles to a dead end |
| 6 | Send-safety | stray `sendable` flag on `LocalInfo`, `mod.rs:756` | a plane-shaped fact outside every registry (concurrency proposal flags it) |
| 7 | Twelve hand-added fact columns | `LocalInfo`, `mod.rs:746-772` | per-binding facts grown one field at a time (row 6 is one of them) |
| 8 | Reflection string fallback | `Reflect.rs:151` | facts leave the compiler as strings |
| 9 | Fact-like orphans | uninit, exhaustiveness, `#Track` provenance (D-PROVENANCE1), view provenance (D-MEMPROVENANCE3), unit-scale provenance, maturity, attribution | each held, checked, and erased — none registered, most unreadable |
| 10 | Four audit sources | `jet unsafe`, `git diff jet.lock`, `~/.jet/trust`, shell history | the authority proposal unifies its four; scrubs, demotions, and `.Force` still live outside |

The four rethinks each cleaned their quadrant. Rows 2–9 are the seams between the
quadrants — exactly where a capstone has to act before cards #1517–#1579 pour
concrete into them.

## The model

**One relation.** `fact(plane, subject, value)`. Subjects: value, place, scope, build.
The registry of planes is D-META-REG1's one table — already ratified; this proposal
adds nothing to it, it finishes moving everyone in.

**One order, per plane.** Every plane orders its facts by safety. More certainty about
a value is safer. Fewer rights in a scope is safer. A discharged duty is safer. A
closed write window is safer. The per-plane *operation* rules stay per-plane — units
multiply by adding exponents, intervals add by interval arithmetic, rights meet by
subset — because that is what a plane is. The shared part is the order and the law.

**The law.**
1. **Toward safety: silent and free.** Inference, narrowing, attenuation, discharge,
   taint spread, exact conversion. The compiler does this for you; it never asks.
2. **Away from safety: one written word.** `approx(x)`, `.raw()`, a call through a
   `#Scrub(Tag)` sanitizer, `#Unsafe("reason")`, `#Impure("reason")`, `.Force`,
   `.drop("reason")`, `detach`, `wrapping(...)`. The word is at the site. Reasons
   stay exactly where ratified law puts them today; no new reason ceremony. The
   memory sigils `&`, `^`, `~` obey the same spirit — each grade of power costs one
   written mark — but they are the ownership prover's surface, not gates, and they
   never enter the ledger (see the wall below).
3. **On the record.** Every gate is auditable from one ledger. (Which gates the ledger
   lists is ballot GATE1.)
4. **At runtime, nothing.** All facts erase; only carriers run (I9-safe: facts live
   entirely in sema).

**The "ohhh" connections, spelled out.**

1. Conservation and attenuation are mirror images. Knowledge is certainty — losing it
   is dangerous. Rights are power — gaining it is dangerous. Point the safe direction
   correctly and both are one ratchet. That is why both rethinks independently
   invented "silent one way, spelled the other".
2. `#Unsafe`, `#Scrub`, `approx`, `.Force`, `.drop("reason")`, `detach` were always
   one family — the release lever. No document names them together; the ledger is
   just the family finally getting a surname.
3. Four registries were minted in one 48-hour window, each citing the others; then
   D-META-REG1 ratified them into one table. The model announced itself before anyone
   proposed it.
4. The unjoined-task check *is* the `#SingleUse` check — the code comment says so.
   Duty was always one plane; D-CONC-UNIT1 already ratified the merge.
5. The ownership sigils were the law's first implementation in spirit: bare read is
   the free direction; `&`, `^`, `~` are each one written word for one grade of
   power. Memory v5 obeyed the law three weeks before the law had a name. The
   sigils stay on the prover's surface, outside the gate family and the ledger.
6. Phantom types are facts that were never given their name. Law zero — registered,
   nameable, reflectable — is not just marker hygiene; applied to every plane it
   closes the whole phantom census.
7. Six flow stores, one shape. Two dead join functions are the price of writing the
   same store six times: somebody always forgets to finish one.

**What the model refuses to absorb (the walls, kept on purpose).**

- **The borrow checker is a prover, not a plane.** Alias and flow analysis over
  places is real program analysis; it cannot be a fold of per-operation algebra
  rules, and pretending otherwise would wreck it. The *facts it publishes* —
  sendability, view provenance, moved-ness — register like any others. The engine
  stays its own. (Ballot OWN1 makes this a wall instead of an accident.)
- **Availability is not a fact about safety.** Runtime-layer ceilings and target
  partitions answer "can this exist here"; they keep their own mechanism (authority
  already ruled this).
- **Facts classify and erase; they never dispatch** (D-FACTMODEL1). The law adds
  nothing at runtime, so it can never become a dynamic capability system.
- **S26 stands.** Facts are values, never types; comptime reads facts and never mints
  a type from one.

## The surface

The surface is the product. Five changes, each a before/after pair. Nothing here
touches the common case; every change is on the expert/audit side or deletes a trap.

**1. One audit command** (GATE1; generalizes D-AUTHORITY-GATE1's ledger):

```sh
# before — the exceptions live in four places, assembled by hand
jet unsafe && git diff jet.lock && cat ~/.jet/trust && history | grep allow
# ...and precision drops, scrubs, and .Force pins are in no list at all

# after (proposed) — one ledger of every step off the safe path
$ jet inspect gates
src/dma.jet:41    #Unsafe "MMIO: board manual §4.2"     (obligations: discharged)
src/etl.jet:88    call of #Scrub(Pii) fn hash_email     (taint cleared)
src/sim.jet:12    approx(reading)                        (exact → Float)
pkg.jet           grants image-codec: FS.Read            (lockfile: recorded)
build             stamp.at forced via .Force             (profile: release)
```

**2. One way to read a fact** (READ1; extends D-CONF-READ1's `$build.*` and
D-META-STAGE1's `$` mark to every registered plane):

```jet
// before — each plane has its own partial story; some facts are strings,
// some are unreadable, states are not even nameable (E0107)
info :: Order.reflect()          // state names arrive as "\0state:..." strings

// after (proposed) — reading what the compiler knows is one act, one sigil
Order.$states                    // [.Draft, .Confirmed, .Shipped] — typed values
Severity.$range                  // 0..10
send_report.$effects             // [Net, DB.Read]
$build.profile                   // ratified today — the same act, build subject
```

**3. Phantom types die honestly** (HOME1):

```jet
// before — compiles, then every call is E0112; no value can ever exist
fn audit(c: Capability) { ... }

// after (proposed) — refused at the signature, with the real path named
fn audit(c: Capability) { ... }
// error: `Capability` is a fact menu, not a type.
// fix: take `Authority` (the rights value) or a rights list `[Right]`.
```

**4. One error voice** (WORD1; the D-AUTHORITY-WORD1 move, finished):

```text
before: "package policy may only tighten"        (config)
        "rights may only attenuate"              (authority)
        "narrowing does not cross this boundary" (flow)
        "this conversion would lose knowledge"   (types)
after:  one shape everywhere (word pair balloted in WORD1; recommended: tighten/loosen):
        "<fact> only tightens here. To loosen it, write <gate>."
e.g.    "this call would loosen the range 0..10. To allow it, write approx(x)."
```

**5. One duty voice** (rides HOME1; the E0140 family and `#SingleUse` speak as one):

```text
before: "unjoined task" (warning) / "single-use value dropped" (error) — two voices
after:  "this value still owes `join` — join it, or write `detach`"
        "this value still owes `send` — send it, or write `.drop("reason")`"
```

Deleted by this surface: the four-command audit crawl, the string reflection
fallback, the phantom-type signature trap, two duplicated error vocabularies, and
five law-words in docs and diagnostics. Added: zero new annotations, zero new
keywords, one CLI noun.

## Beginner magic, expert control

The ladder, bottom to top. Every rung is opt-in, and no upper rung changes what a
lower rung's code does.

**Rung 0 — type nothing.** The compiler holds the facts alone. All lines below are
today's ratified behavior, kept:

```jet
fn main() {
    total :: 19.99 * 3            // exact by default (D-TYPE2-DEFAULT1)
    data :: fetch(url)            // effects inferred; build prints one line
    h :: task compute()           // bound handle: join it, or dropping it
    print(h.join() ?? 0)          // is an error (D-CONC-JOIN1); ? rail
}
```

The defaults are refusable (state the fact yourself, rung 1), visible (read it,
rung 2), and project-switchable (the config plane, D-CONF-*, owns every default).

**Rung 1 — state a fact inline.** Ratified spellings, unchanged:

```jet
fn set_brightness(level: Int(0..100))     // knowledge, inline (D-TYPE2-SPELL1)
fn handle(r: Request) =[Net, DB.Read]=>   // rights bound
    Response { ... }
altitude :: 100meter                       // unit fact from the literal
```

**Rung 2 — read the facts.** `$` on any registered plane (proposed, READ1);
`jet explain` answers "what is known about this value and where was it learned".

**Rung 3 — gate a fact.** One written word per away-move, exactly where ratified law
already puts it: `approx(x)`, `#Scrub(Pii)`, `wrapping(sum + b)`, `.Force`,
`#Unsafe("reason")`. No gate gets new ceremony; the gates only gain a shared ledger.

**Rung 4 — declare new facts.** Ratified today, one row each in readable prelude
source: `tag`, `effect FS.Read`, `state`, `#UnitFamily`, typed settings. Post-E3
user markers ride the same rows (marker-plane :318) — the extension point is the
registry row, never a new compiler mechanism.

**Rung 5 — own the audit.** `jet inspect gates` is the reviewer's one list; CI can
ratchet it ("no new gates without sign-off") because it is one list.

Ceremony check: rung 0 gains no marker, no word, no build step. Magic check: every
default above has a refusal (write the fact), a viewer (rung 2), and a project
switch (the config plane).

## What it looks like

One program, all four planes, the law visible only at the gates. Lines marked
*proposed* are this slate; everything else is ratified.

```jet
tag Pii { deny: [Log, Net] }                     // a fact kind: one registry row

#Scrub(Pii)                                      // the taint gate lives on the
fn hash_email(email: String) => String { ... }   // sanitizer's declaration (D-TAG-SURFACE1)

fn ingest(rows: [Row]) =[DB.Read, Log]=> Report {
    stats :: shared Stats.{ seen: 0 }            // place facts: lock story inferred

    task.group g(limit: 4) {                     // work facts: state, duty, reach
        loop row, rows {
            task process(row, stats)             // duty: g joins every child
        }
    }

    email :: rows[0].email                       // taint: Pii spreads silently
    key :: hash_email(email)                     // the gate fires here, on the record
    log.info("first key: {key}")                 // fine — suspicion was spelled away

    reading :: sensor_avg(rows)                  // exact by default
    wire: F32 = approx(reading)                  // the gate: precision loss, spelled

    return Report.{ key, wire, count: stats.seen }
}

// The expert's audit, after the fact:            (proposed)
// $ jet inspect gates
//   src/ingest.jet:16  call of #Scrub(Pii) fn hash_email   (taint cleared)
//   src/ingest.jet:20  approx(reading)                     (exact → F32)
```

And the same model read back (proposed, READ1):

```jet
Report.$fields.wire.$exactness    // .Approximate(bits: 53)
ingest.$effects                   // [DB.Read, Log]
Stats.$reach                      // .Sendable — the fact five checkers ask about today
```

The through-line: a beginner reads this program top to bottom and sees two odd words
— `#Scrub` and `approx` — exactly the two places the program stepped off the safe
path. That is the law working: the diff *is* the audit.

## What this unlocks

- **Certification and security review** — flight software, medical, fintech: one
  ledger is the difference between "grep the repo for a week" and "read one list".
  The Epoch-8 sandbox proof verifies one law instead of five.
- **Teaching** — one sentence covers types, effects, ownership, and concurrency
  safety. Today that story takes four chapters with four vocabularies.
- **Tooling and agents** — `jet explain` and `$`-reads give IDEs, doc generators,
  and AI agents the same typed model the checker uses; no string parsing.
- **The unbuilt cards** — #1517–#1579 (config, failure, meta, concurrency, authority,
  corelib) all manipulate facts; one substrate means each card builds its feature,
  not its own fact plumbing.
- **Soundness** — one flow engine with one join rule closes the dead-join holes
  (typestate and partial-init across branches) instead of patching them twice.
- **Trivial one-liners** — unchanged: zero annotations, and now zero gate noise too,
  because a program that never leaves the safe path has an empty ledger.

## What stays

Only what wins on merit; nothing stays because it shipped.

- Every ratified spelling: the memory sigils, effect rows, `tag`/`state`/`effect`,
  inline refinements, unit literals, `$build.*`, the concurrency surface, every gate
  word. This proposal renames nothing a user types.
- Per-plane algebras. Units are a group; intervals are arithmetic; rights are sets;
  states are automata. One algebra for all of them would be a false unification —
  the shared thing is the order and the law, and that is enough.
- The borrow checker as its own prover (ballot OWN1 makes it a wall).
- All walls: no macros, no dependent types, no HKT, no top type (D-ANY-JAI1), S26,
  D-FACTMODEL1's never-dispatch rule, zero cost, effect erasure, I1–I9.
- All four rethink proposals as ratified. This capstone amends none of their
  decisions; it adds the roof they each drew one corner of.

## Decisions for the owner

Direction-level; each stands alone; worked examples in the sections above.

| ID | Question | Recommendation |
|---|---|---|
| D-FACT-LAW1 | Ratify the one-way law as one spec law that EXACT1, AUTHORITY-MODEL1, CONF-MERGE1, POLICY-SCOPE1, FLOWTYPE1, and the duty rules instantiate | adopt with the guarded registry (option B) |
| D-FACT-WORD1 | One law vocabulary in diagnostics and docs: facts tighten, a gate loosens; "attenuate/conserve" retire as law-words (flow narrowing keeps its operation name) | adopt tighten/loosen (option A) |
| D-FACT-GATE1 | One ledger + `jet inspect gates` for every gate (generalizes D-AUTHORITY-GATE1); choose full ledger vs security-gates-only | full ledger |
| D-FACT-READ1 | `$` reads every registered plane (`T.$range`, `f.$effects`, `x.$state`); extends D-CONF-READ1 / D-META-STAGE1; kills string reflection | adopt |
| D-FACT-HOME1 | Home the user-facing orphans: attribution, `#Track`, view/unit provenance, maturity, send-safety become registry rows; prover internals (uninit, exhaustiveness) stay engine-side; phantom fact enums rejected at signatures with fix-its | adopt |
| D-FACT-OWN1 | Ratify the wall: the borrow checker is a prover, never a plane; its published facts register; the sigil surface is closed | adopt |
| D-FACT-FLOW1 | One flow-fact store and one join contract for moved/uninit/state/narrow/taint (machinery; fixes the dead joins) | adopt |

## Implementation shape

Effort is expendable; the sequence is what matters.

- **Phase A — internal re-founding, no surface change.** One fact store on the
  checker (replacing the six flow stores and the twelve `LocalInfo` columns), one
  join contract, the D-META-REG1 table as the single plane registry. Every test
  stays green; generated Rust byte-identical.
- **Phase B — land the owed cards on the substrate.** #1517–#1579 build their
  features as plane instances; the concurrency facts (UNIT1/CROSS1) and authority
  substrate (MODEL1) land once, here.
- **Phase C — the balloted surface.** The ledger command, the `$` fact reads, the
  phantom rejections, the one error voice — each a coherent greenfield migration
  that deletes the replaced form.
