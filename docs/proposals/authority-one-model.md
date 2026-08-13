# Authority: one model

Proposal, 2026-08-06. Full ballot profiles: the seven original
D-AUTHORITY-* ballots plus the ratified SCOPE1 and MEM2 follow-ups. All nine
outcomes ratified 2026-08-06. The delivery map is recorded in
`docs/spec/syntax-decisions.md` (card #1500). The forms below are ratified
targets; implementation cards still own migration to shipped code.

## Executive summary

Jet answers one question in at least nine different places: **who may do what,
in what scope, granted how**. The effect system answers it for functions. The
`#Caps` and `#Grant` markers answer it for blocks. The `#Policy` ladder answers
it for non-memory policy and unsafe floors; memory denials ride the effect row.
The package effect budget answers it for
dependencies. The build-effect gate answers it for build plans. Comptime
purity answers it for build-time code. The REPL authorizer answers it for
sessions. The plugin sandbox answers it for extensions. The `jet trust` store
answers it for the local user. And the ratified agent-executor design is about
to answer it for child processes — a tenth copy, currently unbuilt.

Each answer re-invents the same three parts: a closed vocabulary of actions, an
allow/deny/grant record, and a rule that inner scopes may only tighten what
outer scopes allow. The code shows the cost of nine copies. The spec promises
ten effect roots; the code ships twenty-eight. The word "Capability" now has
six unrelated meanings, and the `Capability` type is accepted everywhere a type
name is legal but can never be constructed. Four separate allow/deny schemas
live under one `policy:` key in `pkg.jet` with no shared implementation. Two
purity checkers enforce the same rule with different error codes.

The finding: these are not nine features. They are one relation — **a scope
holds a set of rights** — and one law — **rights only shrink as scope nests;
every re-widening is a written, audited gate**. Every ratified rule in this
area is already an instance of that law. The proposal names the relation, builds
it once, re-founds all nine mechanisms on it, and redesigns the surface to
match. Every surface break is shown as a before/after pair and decided by a
ballot. Every authority right is kept. Several ratified-but-unbuilt decisions
(D-AGENT-EXEC2, D-JOS-INSTALLTRUST1, D-JPK-SANDBOX2) land on the new substrate
instead of adding a tenth copy.

Why now: the drift is already user-visible. `jet explain E1221` tells users the
effect vocabulary has ten roots while the compiler accepts twenty-eight.
`=[Java]=>` compiles today with no spec text behind it. The `Capability`
phantom regressed from an honest "unknown type" error to a dead-end type error.
The agent executor and jetos install-trust work are ratified and about to be
built — building them on a shared substrate is one-time work; building them as
copies ten and eleven is permanent debt.

The original slate asked eight direction-level questions: adopt the model; heal
the root table; fold the memory floors in; name the authority value; merge the
two scope markers; unify the manifest schema; adopt the audit law; retire the
word "capability". Ratified D-AUTHORITY-MEM2 then closes the bounded-arena
argument left by MEM1.
What does not change: effect inference, erasure, every diagnostic families'
meaning, the beginner's zero-annotation experience, and every frozen wall.

The score, in order. **Deleted:** the `BuildEffect` fork, the second
tighten-only lattice, the duplicate purity checker, one of two scope markers,
three of four manifest schemas, the dead-end phantom types, and five of six
meanings of one word.
**Kept:** every authority right, every ratified spelling not amended by this slate,
every diagnostic meaning, zero-cost erasure. **Gained:** a nameable authority value for processes,
plugins, and sessions; one audit ledger; the build-time hole closed; one
vocabulary at every checkpoint.

## Glossary

- **Right** — permission to perform one kind of action, named as a path in one
  tree: `Net`, `FS.Read`, `Mem.Alloc`. Today called an effect root or leaf.
- **Scope** — a region of the program or system that holds rights: a block, a
  function, a module, a package, a build plan, a process, a plugin, a REPL
  session, a machine user.
- **Holds** — the set of rights a scope may use. Today spelled many ways:
  an effect bound, a `#Caps` list, a `#Grant` list, a `policy:` floor, an
  `effects: { allow: … }` budget, an `--allow-*` flag, a trust grant.
- **Uses** — the set of rights code inside a scope actually exercises. Today:
  the inferred effect row.
- **Attenuation** — narrowing: an inner scope holding a subset of its outer
  scope's rights. The opposite — widening — is gaining a right the outer scope
  did not hold.
- **Gate** — a written, audited widening or use of a guarded right:
  `#Unsafe("reason")`, a `grants:` entry, an `--allow-*` flag, a `jet trust
  grant`.
- **Reified authority** — a rights set carried as a runtime value across a
  boundary the compiler cannot see past: into a spawned process, a plugin
  instance, or a REPL session.

## The one idea

**A scope holds a set of rights; nesting only shrinks the set; every
re-widening is a written, audited gate.**

For the beginner this is invisible. They write no annotations. Jet infers what
each function uses, checks it against what each scope holds, and prints one
line at build time: `effects: FS, Net (across root + 2 dependencies)`. The
default is the safe default, and the least-authority answer is the one Jet
writes for them — the secure policy is the zero-effort policy.

For the expert every part is nameable and explicit. They can bound a function
(`=[Net, DB.Read]=>`), narrow a block (`#Caps(Net)`), delegate to a dependency
(`grants:`), hand a measured rights set to a child process (D-AGENT-EXEC2's
`ProcessAuthority`), forbid a right for a whole package (`policy:`), and read
the complete audit ledger of every gate (`jet inspect`). One relation, checked
statically by sema, enforced at build for the dependency graph, and reified as
a value only at true trust boundaries.

## Evidence: the shadow systems

Fourteen shadow mechanisms; among them six closed rights vocabularies
(`Effect`, `BuildEffect`, `PolicyKey`, the trust authority kinds, the registry
identity kinds, the plugin capabilities), plus smaller mode and decision enums,
and four manifest schemas:

| # | Mechanism | Home | Vocabulary | Defect |
|---|-----------|------|-----------|--------|
| 1 | Effect rows on functions | `crates/jet-sema/src/Sema/Effects.rs` | `Effect`, 28 variants | Spec (D-EFF4/5, `syntax-decisions.md:1865`) says ten roots; `Facts.rs:24-28` ships 28; module comment says "ten… twelve" |
| 2 | `#Caps` / `#Grant` blocks | `Sema/CheckerCore/statements.rs:2530-2595` | same 28 | Handle type `Capability` unnameable; second scoping mechanism over the same vocabulary |
| 3 | `#Policy` ladder | `crates/jet-foundation/src/Policy.rs:6-136` | `PolicyKey`, 6 variants | Second tighten-only ladder, own lattice, own error enum (E0355), independent of `effect_covers` |
| 4 | `#Unsafe` obligations | `Sema/UnsafeObligations.rs` | `ObligationMode`, 6 variants | Rides ladder #3, not plane #1; reason string checked only for presence |
| 5 | Package effect budget | `crates/jet-pkg-model/src/EffectBudget.rs` | same 28 | Own allow/deny/grants schema (E1220/E1221); E1221 explain-text still says "ten-effect vocabulary" |
| 6 | Build effects | `crates/jet-foundation/src/BuildEffects.rs` | `BuildEffect`, 10 variants | Near-exact fork of #1's enum; own CLI + manifest + grant resolution (E3503/E3504) |
| 7 | Comptime purity | `jet-comptime/Comptime/Purity.rs` and `jet-sema/Sema/Purity.rs` | impure-builtin list | Two checkers, two diagnostic families (E0951 vs E3401-03), same rule |
| 8 | REPL authorizer | `jet-comptime/Comptime/Interpreter.rs:98-161` | same 28 + flags | Third enforcement point for facts sema already proved |
| 9 | Notebook render trust | `jet-repl/Notebook/kernel.rs` | own decisions | Fourth allow/deny surface, cell-scoped |
| 10 | jetpack trust store | `crates/jetpack/src/Trust.rs:29-46` | 9 authority kinds | Fifth closed vocabulary, disjoint from all others |
| 11 | Registry trust roots | `crates/jetpack/src/TrustRoot.rs` | 4 identity kinds | Sixth closed vocabulary |
| 12 | Manifest trust/provider/lint policy | `PackageManifest/mod.rs:110-134` | `TrustDecision` | Three more allow/deny schemas under the same `policy:` key as #3 and #5, no shared code |
| 13 | Plugin sandbox + compiler extensions | `Prelude/Plugin.rs`, `CompilerExtension.rs:42-200` | plugin `Capability`, 7 variants | Seventh vocabulary; own quota plane; deny-by-default done right but alone |
| 14 | Vetted-unsafe allowlist | `tests/golden.rs:793-841` | 7 region names | Invariant I1 for generated Rust enforced only by a test-side substring scan |

Confirmed defects that fall out of the fragmentation:

- **`Capability` phantom regression.** `core_type_known`
  (`Sema/CheckerCoreLib/core_types.rs:154-157`) accepts any marker-argument
  enum name as a general type, so `fn f(x: Capability)` now compiles to a
  dead end (E0112 at every call, no way to make a value). Same for
  `PolicySetting`, `TaintKind`, `ObligationMode`, `State`, `InlineMode`.
- **Spec-code drift shipped to users.** `jet explain E1221` asserts the
  ten-root vocabulary; the compiler accepts 28. D-FFI-PY1 promised a `Py`
  root that was never added.
- **Six meanings of "capability".** Grant handle; marker-argument enum;
  `&`/`^` diagnostic copy ("write capability"); product capability-claim
  manifest; capability ledger card #1499; remote-builder capability model
  card #422.

## The model

Three axes, one law.

**Axis 1 — the right.** One tree. Roots are closed and ratified; leaves are
declarable (`effect FS.Read`, D-EFFECT-DECL1) with ancestor subsumption
(D-EFFTREE1). The tree extends to cover every vocabulary in the evidence
table: the sixteen FFI-language roots become `FFI.*` leaves, and
D-AUTHORITY-MEM1/B puts memory floors on `Mem.*` leaves with the parameterized
denial from D-AUTHORITY-MEM2. Build actions and host operations reuse the same
names they already reuse informally. Unsafe does not become a right: it is a
gate — the audited way to widen — and gates are the other half of the law, not
entries in the holds-set.

**Axis 2 — the scope.** One nesting chain, already mostly ratified as the
D-MARK-SCOPE1 ladder: organization > package > module > function > block —
extended outward through the boundaries the compiler cannot see past:
dependency, build plan, process, plugin, session, machine user. Inner scopes
are checked statically; boundary scopes receive reified authority values.

**Axis 3 — the moment.** The same fact is checked at three moments: compile
(sema's effect fixpoint), build (the whole-graph budget), boundary (the reified
value a process, plugin, or session receives). One vocabulary, one subsumption
rule, one Prelude implementation (I9); the moments differ, the meaning does
not.

**The law — attenuation.** A scope's holds-set is at most its parent's.
Widening never happens silently; a widening or a guarded use is a gate, and a
gate is (a) written at the site and (b) recorded in the audit ledger.

The ratified record already states this law piecewise, which is the strongest
evidence the model is right:

- "Package policy may only tighten safety — it can forbid unsafe code but can
  never authorize an unsafe operation" (D-PACKAGE-POLICY-SCOPE1).
- Tighten-only combine on the policy ladder (D-MARK-SCOPE1).
- Declared effect bounds are checked as supersets of inferred use (E0740);
  `#Caps` overflow is an error (E0741); prohibition is transitive (E0749).
- A grant handle may not escape its scope (E0711).
- Plugins hold the empty set by construction (D-PLUGIN1); package sandboxes
  deny by default (D-JPK-SANDBOX2); installed apps "start with almost no
  power" and the user grants later (D-JOS-INSTALLTRUST1).
- "Every bypass is spelled at the site or on the command line, never in hidden
  config, and lands in the audit record" (D-LINTPOLICY1).
- The unsafe gate demands a written reason (E3112, D-UNSAFE-REASON1).

The "ohhh" connections, spelled out:

1. **Memory floors are `#Caps` minus `Mem.Alloc`.** D-AUTHORITY-MEM1=B moves
   memory floors out of `#Policy` and onto the rights ladder as denials; the
   bounded form is `Mem.Alloc(above: 65536)` under D-AUTHORITY-MEM2=A. The two
   tighten-only lattices in `Policy.rs` and `Effects.rs` are one lattice
   implemented twice.
2. **Effect inference is least-privilege policy authoring.** The reason Java
   SecurityManager and .NET CAS died — hand-writing fine-grained policy for a
   real dependency tree is intractable, so everyone grants everything — is a
   problem Jet already solved: the compiler writes the minimal policy, and
   humans only decide the exceptions.
3. **The package budget is `#Caps` at package scope.** `effects: { allow: … }`
   and `#Caps(…)` are the same narrowing at two rungs of one ladder;
   `grants:` and `#Grant` are the same delegation, and both already share
   `effect_covers`.
4. **`BuildEffect` is `Effect` with the compiler looking the other way.** The
   build.rs / npm-install-script hole is exactly a second authority system
   for build-time code; folding it in closes the hole cards #642-#649 name.
5. **`#Unsafe` is not a different kind of thing — it is the gate.** The only
   mechanism allowed to mint authority the scope does not hold, which is why
   it alone demands a reason and an audit trail.
6. **`ProcessAuthority` (ratified, unbuilt) is the rights set, reified.** The
   agent executor, the plugin linker, the REPL authorizer, and jetos install
   trust all need the same value; today each is designed as its own type.
7. **The trust store and the effect budget are the same record at different
   scopes.** `grants: { "dep": [FS] }` in `pkg.jet` and `grant:user:build:…`
   in `~/.jet/trust` both say: this named subject holds these rights, granted
   by this authority, auditable later.

What the model deliberately does **not** absorb (the walls):

- **Availability is not authority.** Runtime-layer ceilings (E1006), target
  profiles, and web/OS partitions answer "can this exist here", not "may this
  happen". They stay separate mechanisms.
- **Data taint is the dual, not an instance.** Tags (`Secret`, `Credential`)
  constrain what may be done *with a value*; rights constrain what may be done
  *by a scope*. Both are compile-time facts (D-FACTMODEL1) and the sink checks
  meet the rights tree (the `Secret` root), but tags stay tags.
- **Lint severity is not authority.** `policy.lints.deny` governs build
  reporting, not actions; it keeps its own small schema.
- **Registry identity (who signed a package) stays TUF-shaped.** It feeds
  trust decisions but is an identity question, not a rights question.

## The surface

The surface is the product. Three rules for spellings, from the model:

1. **One name per idea.** A right has the same name in a row, a marker, a
   manifest, a flag, and a trust grant.
2. **One marker per job.** Two markers that set the same relation merge.
3. **Kept spellings must win on merit.** Nothing stays because it shipped.

The redesign, as before/after pairs from real programs. Each pair names its
ballot and what it amends.

These are ratified target forms, not claims that the compiler has already
migrated. The implementation cards own the one-pass source, parser, sema,
grammar, diagnostic, and snapshot migration for each amended surface.

**Foreign calls get one root** (D-AUTHORITY-ROOTS1; amends D-EFF4/D-EFF5 and
the D-FFI-*1 effect clauses):

```jet
// before — sixteen flat roots; forbidding all of them takes sixteen words
fn score(row: Row) =[Java]=> Float { ... }
fn pure_math(x: Float)
  =[!Go, !Java, !DotNet, !Fortran, !Cobol, !Tcl, !Lua, !Ada,
    !Pascal, !Dart, !PowerShell, !Perl, !Ruby, !Php, !R, !Com]=> Float

// after (ratified; implementation #1567) — one root, languages as leaves
fn score(row: Row) =[FFI.Java]=> Float { ... }
fn pure_math(x: Float) =[!FFI]=> Float { ... }
```

**The two scope markers become one** (D-AUTHORITY-SCOPE1; amends D-EFF1's
`#Caps` region and D-SCAP1/D-ARROW-CONTROL1's `#Grant` head):

```jet
// before — two markers, one relation, two error families
#Caps(DB.Read) { db.query(id) }          // narrow: E0741 outside the list
#Grant(caps: FS, Net) { backup() }       // grant + handle: E0712 / E0711

// after (ratified; implementation #1573) — one marker; a name binds the handle when you want one
#Caps(DB.Read) { db.query(id) }
#Caps(g: FS, Net) { backup() }           // g is the Authority value
```

**The manifest reads top to bottom** (D-AUTHORITY-MANIFEST1; amends
D-EFFBUDGET1 keys and D-JPK-POLICYSURFACE1/D-JPK-GRANTSCHEMA1 placement):

```jet
// before — four schemas in four shapes
effects: { allow: [Net, DB.Read], deny: [Exec] }
grants: { "image-codec": [FS.Read] }
policy: .{ trust: { default: prompt }, providers: { nix: { ... } } }

// after (ratified; implementation #1570) — one block: what this package may do and delegate
authority: .{
  holds: { allow: [Net, DB.Read], deny: [Exec] },
  grants: { "image-codec": [FS.Read] },
  trust: { default: prompt },
  providers: { nix: { ... } },
}
```

**Boundaries take one value** (D-AUTHORITY-NAME1; amends D-AGENT-EXEC2
naming):

```jet
// before — a bespoke type per boundary, ratified or planned:
// ProcessAuthority for processes, plugin capability lists, REPL grant copies

// after (ratified; implementation #1569) — one value, one narrowing rule, three boundaries
a :: Authority.workspace()
process.run("jet build", a.with(FS.Write, "out/"))
plugin.load("lint", a.without(FS.Write))
```

**The audit is one command** (D-AUTHORITY-GATE1; generalizes D-LINTPOLICY1's
bypass law):

```sh
# before — four sources to assemble by hand
jet unsafe && git diff jet.lock && cat ~/.jet/trust && history | grep allow

# after (ratified; implementation #1571) — one ledger, one reader
jet inspect authority
```

**One word, one meaning** (D-AUTHORITY-WORD1):

```text
before: "the loop takes the write capability (&) for items"   <- borrowing
        "the Capability handle escapes the #Grant region"     <- authority
after:  "the loop takes write access (&) for items"
        "the Authority handle escapes its region"
```

The slate, item by item:

| Item | Status |
|------|--------|
| `=[Net, FS.Read]=>` rows, `#Unsafe("reason")`, `#Policy(…)` | ratified, kept on merit: rows read as contracts, the reason is the audit, and `#Policy` keeps its non-memory safety settings |
| `#Caps` absorbs `#Grant`; a `name:` head binds the handle | ratified A (D-AUTHORITY-SCOPE1; implementation #1573; amends D-EFF1, D-SCAP1, D-ARROW-CONTROL1) |
| `effect FS.Read` leaf declaration | ratified (D-EFFECT-DECL1), unchanged |
| Root table: thirteen closed roots — `Net FS IO DB Time Rand Env Exec Log GPU FFI Browser Secret`; FFI languages become `FFI.Go`, `FFI.Java`, … leaves | ratified A (D-AUTHORITY-ROOTS1; implementation #1567; amends D-EFF4/D-EFF5 and the effect clauses of the D-FFI-*1 ballots) |
| `Mem.Alloc`, `Mem.Rc`, `Mem.Gc` leaves; memory floors use effect denials, including `Mem.Alloc(above: N)` | ratified B/A (D-AUTHORITY-MEM1/MEM2; implementation #1568; MEM2 record-only, folds into criterion 4) |
| A nameable `Authority` value usable at boundaries: process spawn, plugin instantiation, session | ratified A (D-AUTHORITY-NAME1; implementation #1569; `ProcessAuthority` renamed) |
| One manifest authority schema shared by package bounds, dependency grants, trust and providers | ratified A (D-AUTHORITY-MANIFEST1; implementation #1570) |
| `jet inspect authority` — one ledger of every gate: unsafe reasons, dep grants, flags, trust grants | ratified A (D-AUTHORITY-GATE1; implementation #1571) |
| "capability" leaves user-facing surfaces; `&`/`^` diagnostic copy says "write access"; product claim ledger becomes feature claims | ratified A (D-AUTHORITY-WORD1; implementation #1572) |

## What it looks like

**Beginner — writes nothing, gets least privilege.** (Ratified behavior,
kept.)

```jet
fn fetch_scores(url: String) => String {
    net.get(url)                     // Jet infers: uses Net
}

fn save(path: String, body: String) {
    files.write(path, body)          // Jet infers: uses FS.Write
}
```

Build output (ratified, D-EFFBUDGET1):

```
effects: FS, Net (across root + 2 dependencies)
```

**Middle — bounds and scoped narrowing.** (Ratified target form, kept after
the implementation migration.)

```jet
fn handle(req: Request) =[Net, DB.Read]=> Response {
    #Caps(DB.Read) {
        // Net is out of reach here; a stray net.get is E0741
        db.query(req.id)
    }
    respond(req)
}
```

`pkg.jet`, today (ratified, D-EFFBUDGET1 / D-PACKAGE-POLICY-SCOPE1) and the
ratified D-AUTHORITY-MANIFEST1 form (implementation #1570):

```jet
// today
policy: .{ unsafe: .Forbid }
effects: { allow: [Net, DB.Read, Log], deny: [Exec] }
grants: { "image-codec": [FS.Read] }

// ratified form
policy: .{ unsafe: .Forbid }
authority: .{
  holds: { allow: [Net, DB.Read, Log], deny: [Exec] },
  grants: { "image-codec": [FS.Read] },
}
```

**Expert — boundaries and gates.** (Ratified target forms; implementations
#1569 and #1571.)

```jet
fn build_worker() =[Exec]=> ProcessReceipt {
    a :: Authority.workspace()           // D-AUTHORITY-NAME1 — implementation #1569
                                         // reified rights: read-only repo,
                                         // no Net, no Env, no secrets
    a2 :: a.with(FS.Write, "out/")       // attenuate-or-extend only
                                         // within what this scope holds
    process.run("cargo build", a2)       // ratified direction: D-AGENT-EXEC1/2
}

#Unsafe("MMIO: board manual §4.2, register is write-once") {
    mem.volatile_write(reg, 1)
    assert valid_ptr, aligned            // ratified: D-UNSAFE-OBLIG1
}
```

Audit read-back (ratified D-AUTHORITY-GATE1; implementation #1571):

```
$ jet inspect authority
gates:
  src/dma.jet:41  #Unsafe "MMIO: board manual §4.2…"  (obligations: discharged)
  pkg.jet         grants image-codec: FS.Read          (lockfile: recorded)
  session         --allow-net (2026-08-06, repl)
```

## What this unlocks

- **Agents and processes** (epoch 7): D-AGENT-EXEC1/2 land as instances — the
  workspace authority object is the reified rights set, receipts come from the
  one ledger. No second isolation vocabulary.
- **jetos** (epoch 9): D-JOS-INSTALLTRUST1's "apps start with almost no power"
  is the attenuation law at the OS scope; grant prompts reuse the same right
  names users saw in `jet build` output.
- **Supply chain** (epochs 4/8): comptime, build plans, and generators sit
  under the same tree — the cargo-build.rs / npm-install hole (#642-#649)
  closes structurally, not by case-work. The Epoch-8 sandbox proof (#398)
  verifies one model instead of five.
- **Embedded and critical work**: `Mem.*` on the same ladder means a flight
  controller package writes effect denials, including parameterized bounds, in
  one vocabulary and gets one audit ledger for certification.
- **Trivial one-liners**: unchanged — zero annotations, one build line.
- **Teaching**: one sentence covers the whole security story: "your code can
  only do what its scope holds, holds only shrink, and every exception is
  written down."

## What stays

Only what wins on merit; nothing stays because it shipped.

- Effect inference, erasure, and zero runtime cost (byte-identical Rust) —
  inference is the least-privilege magic, and erasure is the price of zero.
- `=[Net]=>` rows and `effect` leaf declarations — a row reads as a contract.
- `#Unsafe("reason")` — the written reason is the audit; no shorter form
  carries it.
- Memory floor words retire under D-AUTHORITY-MEM1=B; denials use
  `=[!Mem.Alloc]=>`, with `Mem.Alloc(above: N)` from D-AUTHORITY-MEM2=A.
- All diagnostic meanings; codes stay (E0740/E0741/E0711/E0712/E1220/E3112…).
- Frozen walls: no top type, no HKT, no macros, comptime never creates types,
  facts never select runtime types or dispatch (the reified authority value is
  ordinary data, never a dispatch input), I1-I9 (I1, I3, I9 are strengthened:
  one Prelude substrate, sema-only checking, one meaning on every tier).
- Availability mechanisms (runtime layers, target profiles, web partition)
  and data tags stay what they are.

## Ratified decisions and delivery

| Decision | Outcome | Card | Status |
|----------|---------|------|--------|
| D-AUTHORITY-MODEL1 | A — one authority substrate | #1566 | ratified |
| D-AUTHORITY-ROOTS1 | A — thirteen roots; FFI languages as leaves | #1567 | ratified |
| D-AUTHORITY-MEM1 | B — memory floors as effect denials | #1568 | ratified |
| D-AUTHORITY-MEM2 | A — parameterized denial row | record-only | ratified; acceptance in #1568 criterion 4 |
| D-AUTHORITY-NAME1 | A — `Authority` value | #1569 | ratified |
| D-AUTHORITY-MANIFEST1 | A — one `authority:` block | #1570 | ratified |
| D-AUTHORITY-GATE1 | A — one authority ledger | #1571 | ratified |
| D-AUTHORITY-WORD1 | A — retire `capability` from surfaces | #1572 | ratified |
| D-AUTHORITY-SCOPE1 | A — `#Caps` absorbs `#Grant` | #1573 | ratified |

Bugs found during research (cards, not ballots): **#1501** — the
`core_type_known` marker-argument regression. The read-only board check found
no cards for the remaining three findings: the E1221 ten-vs-28 explain text,
stale "ten/twelve roots" comments, and the D-FFI-PY1 promised-but-missing
`Py` root. The orchestrator must file those findings before this criterion can
pass.
Also recorded, not balloted here: the `#Policy(…)` spelling itself names two
unrelated ratified mechanisms — scope floors (D-POLICY-WORD1) and call
decorators (D-CALLPOLICY1) — a word collision of the same kind this proposal
documents for "capability".

## Implementation shape

- **Phase A — internal re-founding, no surface change.** One substrate in
  `jet-foundation`: the rights tree (one table), the holds relation, the
  attenuation lattice, the gate record. `Effects.rs`, `Policy.rs`,
  `EffectBudget.rs`, `BuildEffects.rs`, the two purity checkers, and the REPL
  authorizer become consumers. All tests stay green; generated Rust stays
  byte-identical.
- **Phase B — land ratified-but-unbuilt work on the substrate.**
  D-AGENT-EXEC1/2 (`ProcessAuthority` as the reified value),
  D-JOS-INSTALLTRUST1, and the Epoch-8 sandbox proof consuming one model.
  D-CALLPOLICY1 is untouched: its call decorators (`#Policy(retry(3))`) are a
  separate ratified mechanism, not authority; only the spelling collision is
  recorded above.
- **Phase C — ratified surface unifications**, each a coherent greenfield
  migration owned by #1567–#1573 that deletes the replaced form: the root-table
  heal, the memory fold, the authority value, the scope-marker merge, the
  manifest schema, the ledger, and the word. D-AUTHORITY-MEM2 is record-only
  and folds into #1568 criterion 4.

## Ballot-ready slate

The profiles below are the full owner-facing records for the seven original
ballots. SCOPE1 and MEM2 were added after the original slate; their full
profiles follow so the nine-decision record stays complete. Every profile uses
the `safety` decision group, `ballotMode: full`, and names its amendments. The canonical board is
the authority for current status; this section is the reviewable Markdown copy.

### D-AUTHORITY-MODEL1 — One authority model for effects, caps, policy, budgets, and trust

- **Group:** `safety`
- **Card:** #1500
- **Status:** `ratified` (2026-08-06)
- **Amends:** No prior surface ruling is amended. This is the substrate ruling: it applies I3 and I9 to the shared rights tree, holds relation, attenuation rule, and gate record.
- **Gist:** Whether Jet builds all authority checks on one shared model.

#### Lesson

Nine parts of Jet decide who may do what. Effect rows, #Caps and #Grant, the #Policy ladder, package budgets, build effects, comptime purity, the REPL gate, plugin sandboxes, and the trust store each carry their own vocabulary and rules. They already obey the same law: a scope may only shrink what its parent allows, and every exception is written down. This choice decides whether that law gets one implementation or stays spread across nine.

#### Story

Maya runs a small billing service with two dependencies and a deploy agent. Her auditor asks one question: what can this system touch? Today the compiler, the package manifest, the build runner, and the trust store answer in four vocabularies. She wants one answer she can hand to the auditor.

#### In the wild

```text
// pkg.jet — package scope speaks one vocabulary
effects: { allow: [Net, DB.Read, Log], deny: [Exec] }
grants: { "image-codec": [FS.Read] }

// src/report.jet — the compiler speaks the same names
fn render(rows: Table) =[FS.Write]=> Unit { ... }

// but the build runner has its own fork of the same list:
//   jet build --allow-net        (BuildEffect, not Effect)
// and the trust store a third:
//   jet trust grant build ./tool (authority kinds, not effects)
```

#### Options

##### A — One substrate

All nine mechanisms become readers of one rights tree, one holds-set relation, one tighten rule, and one gate record. Nothing users type changes. The compiler, the budget, the sandbox, the REPL, and the trust store give the same answer with the same names. Ratified-but-unbuilt work (the agent executor, jetos install trust, the sandbox proof) lands on this substrate instead of adding more copies.

Technical: One foundation module owns the root table, the subsumption lattice (effect_covers), the scope ladder (D-MARK-SCOPE1), and the gate ledger. Effects.rs, Policy.rs, EffectBudget.rs, BuildEffects.rs, both purity checkers, and the REPL authorizer become consumers. Semantics live in the substrate per I3/I9; generated Rust stays byte-identical.

```text
// Same right, one name, every checkpoint:
fn sync() =[Net]=> Unit { ... }        // compile: sema row
// pkg.jet: effects: { allow: [Net] }  // build: whole-graph budget
// jet run --allow-net                 // session: same name, same rule
// jet inspect authority               // one ledger of every grant
```

##### B — Law on paper only

The spec states the attenuation law and each mechanism must cite it, but every engine keeps its own code. Cheapest to adopt. Drift between the copies remains possible, and the ratified agent and jetos work still each build their own engine.

Technical: A spec section names the law and lists the nine mechanisms as instances. No shared code. New authority features must reference the section in their ballots.

```text
// The drift this cannot stop, live today:
// jet explain E1221 -> "ten-effect vocabulary (D-EFF4)"
// Facts.rs EFFECT_ROOTS -> 28 entries
fn f() =[Java]=> Unit { ... }   // compiles; no spec text behind it
```

##### C — One vocabulary, nine engines

Merge the six closed name tables into one rights tree so every surface uses the same names, but keep each mechanism’s own checking engine. Users see consistent words. The same tighten law still runs in nine places with nine error surfaces, and boundary features still need bespoke types.

Technical: EFFECT_ROOTS, BuildEffect, PolicyKey, trust authority kinds, and plugin capabilities merge into one table in jet-foundation. Resolution code stays where it is.

```text
// One name everywhere, but still two ladders:
#Policy(no_alloc)      // ladder 1: PolicyKey lattice, E0355
#Caps(FS) { ... }      // ladder 2: effect lattice, E0741
// same "only tighten" law, two implementations, two error families
```

##### D — Keep nine models

Decline. Each boundary keeps its own model and vocabulary. No re-founding work. The word capability keeps six meanings, the spec and code keep drifting, and every future boundary (agents, jetos, remote builders) adds another copy.

```text
// The tenth copy is already ratified and waiting:
// D-AGENT-EXEC2: ProcessAuthority, ProcessPlan, ProcessReceipt
// D-JOS-INSTALLTRUST1: per-app grant records
// each would mint its own vocabulary and checker
```

#### Comparisons

- **Java SecurityManager:** One runtime policy oracle over ambient authority. Retired: hand-written policy for real dependency trees collapsed to grant-everything, and every sensitive call site had to opt in forever.

```text
grant { permission java.security.AllPermission; }; // what real policy files said
```

- **Safe Haskell:** The compiler checks the trust chain across the whole import graph. The strongest prior art for making authority a static, transitive fact rather than a runtime hope.

```text
{-# LANGUAGE Safe #-} -- a Safe module cannot import untrusted code
```

- **Deno:** Default-deny with explicit allow flags works, but the grant unit is the whole process, so one needy dependency widens everyone.

```text
deno run --allow-net app.ts  // every module now has the network
```

#### Recommendation

- **Pick:** `A`
- **Why:** One substrate makes every authority answer agree, heals the drift at its root, and lets the ratified agent, jetos, and sandbox work land as instances instead of copies ten, eleven, and twelve. It also makes the safety story teachable in one sentence.
- **Why not:**
  - `B`: Paper cannot stop drift. The spec already said ten roots while the compiler shipped twenty-eight, with the stale count baked into user-facing explain text.
  - `C`: Shared names with nine engines still checks one law nine ways, keeps nine error surfaces, and leaves boundary features building bespoke authority types.
  - `D`: The fragmentation is already user-visible, and two more authority consumers are ratified and about to be built on nothing.
- **Trade-off:** A single substrate concentrates risk: a bug in the shared lattice touches every check at once. That is acceptable because one heavily tested lattice is safer than nine partly tested ones, and the golden suite pins generated output byte-for-byte.

#### Review passes

- **base:** The first draft compared one substrate, a documented law, a shared vocabulary without shared engines, and the status quo. It recommended the substrate because agreement between checkpoints is the point.
- **boilOcean:** The breadth pass also tested a dynamic runtime authority object and split language-side and toolchain-side substrates. The runtime shape repeats the SecurityManager failure and was rejected, and the split shape was folded into option C as its strongest form.
- **hybrid:** The hybrid pass tried document-now-unify-later and vocabulary-now-substrate-later stagings. Both let the agent executor build on the old engines meanwhile, creating the tenth copy this ballot exists to prevent, so no hybrid survived.
- **cooperative:** The cooperative pass strengthened B with a conformance test suite that checks each engine against the documented law, and strengthened C by including the manifest schemas in the merged vocabulary. Both improvements are kept in those options as written.
- **adversarial:** The adversarial pass attacked A with substrate bugs propagating everywhere and the refactor disturbing erasure. The golden suite pins byte-identical output and phase A changes no surface, so A remains the recommendation.

### D-AUTHORITY-ROOTS1 — Heal the effect root table: FFI languages become leaves

- **Group:** `syntax`
- **Card:** #1500
- **Status:** `ratified` (2026-08-06)
- **Amends:** Amends D-EFF4 and D-EFF5, plus the effect clauses of the D-FFI-*1 rulings. D-FFI-PY1 and D-FFI-OCTAVE1 are honored as `FFI.Py` and `FFI.Octave` leaves.
- **Gist:** What the closed list of effect roots actually is.

#### Lesson

D-EFF4 ratified ten closed effect roots. Since then, sixteen language-binding ballots each added a root (Go, Java, Lua, and so on), plus Browser and Secret, without amending D-EFF4. The code now has twenty-eight roots, the spec and the E1221 explain text still say ten, and D-FFI-PY1 promised a Py root that was never added. This choice picks the true closed list and where foreign-language calls live in it.

#### Story

Tom reads the Jet book, learns the ten effect roots, and audits a teammate’s service. He finds =[Java]=> in a signature. No documentation he owns mentions a Java effect, and jet explain tells him the vocabulary has ten roots. He no longer trusts the list he learned.

#### In the wild

```text
// Compiles today with no spec text behind it:
fn score(row: Row) =[Java]=> Float {
    jvm.call("Scorer", "score", row)
}
// jet explain E1221 -> "...effect names from the ten-effect
// vocabulary (D-EFF4)..." — the shipped doc contradicts the compiler.
```

#### Options

##### A — One FFI root, languages as leaves

The closed table becomes thirteen roots: the ratified ten plus FFI, Browser, and Secret. The sixteen language roots become leaves: FFI.Go, FFI.Java, FFI.Lua, and the promised FFI.Py and FFI.Octave arrive as leaves too. A bound like =[!FFI]=> forbids all foreign calls at once. Existing rows migrate in one pass and the old flat spellings are deleted.

Technical: Amends D-EFF4/D-EFF5 (root list) and the effect clause of each D-FFI-*1 ruling. Honors D-FFI-PY1 and D-FFI-OCTAVE1 by delivering their effects as leaves. Ancestor subsumption (D-EFFTREE1) already gives FFI covering FFI.Go. E1221 text and the stale Effects.rs comments are corrected in the same change.

```text
fn score(row: Row) =[FFI.Java]=> Float { ... }
fn pure_math(x: Float) =[!FFI]=> Float { ... }  // no foreign calls, ever
// pkg.jet: effects: { deny: [FFI] }            // whole package, one line
```

##### B — Bless the flat thirty

Keep every language as its own root and add the missing Py and Octave. Amend D-EFF4/5 and the explain text to list thirty roots. No migration. The teaching story grows from ten words to thirty, and forbidding all foreign calls takes sixteen prohibitions.

Technical: Facts.rs becomes the ratified list verbatim. D-EFF4/5 amended to thirty. No row changes anywhere.

```text
// Forbidding foreign calls under a flat table:
fn pure_math(x: Float)
  =[!Go, !Java, !DotNet, !Fortran, !Cobol, !Tcl, !Lua, !Ada,
    !Pascal, !Dart, !PowerShell, !Perl, !Ruby, !Php, !R, !Com]=> Float
```

##### C — Ten teaching roots, system roots documented apart

Keep the ratified ten as the beginner vocabulary. Everything else (language roots, Browser, Secret) becomes a documented system register that ordinary teaching never mentions. Code keeps compiling as today. Two registers of the same kind of name is a second vocabulary wearing a disguise.

Technical: No code change. Spec gains a system-roots appendix. E1221 text says ten user roots plus system roots.

```text
// Same program, two documentation registers:
fn score(row: Row) =[Java]=> Float { ... }  // "system root", appendix only
fn save(p: Path) =[FS]=> Unit { ... }       // "teaching root", chapter 3
```

#### Comparisons

- **WASI:** Preview 1 shipped a flat rights bitmask per handle. The grain was wrong and unwinding it took a full redesign. Grant shape calcifies early.

```text
// preview1: fd_rights: u64 bitmask -> preview2: typed interfaces
```

- **Android:** STORAGE bundled all files under one grant. Narrowing it later (Scoped Storage) took years of migrations.

```text
<uses-permission android:name="READ_EXTERNAL_STORAGE"/>
```

#### Recommendation

- **Pick:** `A`
- **Why:** One FFI root restores a small closed list that teaches in a breath, makes forbid-all-foreign-calls one word, and honors the two rulings whose promised roots never shipped. The tree already supports it: leaves and subsumption are ratified and built.
- **Why not:**
  - `B`: Thirty flat roots make the closed set a memorization exercise and a sixteen-term prohibition row the price of purity from foreign code.
  - `C`: Two registers of one vocabulary is drift by design. The audit that found this problem started from exactly such a split.
- **Trade-off:** Existing =[Go]=> style rows migrate to =[FFI.Go]=> in one repository-wide pass. That is churn without behavior change, accepted because the flat spellings were never ratified as a set.

#### Review passes

- **base:** The first draft compared the leaf collapse against blessing the flat table and recommended the collapse for teachability and single-word prohibition.
- **boilOcean:** The breadth pass added the two-register option and tested folding all foreign-language effects into Exec. The Exec fold was rejected because a JVM call and spawning a process are different risks an auditor must tell apart.
- **hybrid:** The hybrid pass tried A with Browser and Secret also demoted to leaves under IO or a new Data root. Neither has a natural parent among the ten, so the hybrid was rejected and both stay roots.
- **cooperative:** The cooperative pass strengthened B with its real virtue: a flat table has no hierarchy rules and never surprises anyone with subsumption. C gained its clearest framing as a documentation-only fix.
- **adversarial:** The adversarial pass asked whether FFI subsumption hides which language a dependency uses, and it does not: exact leaves stay in rows, budgets, and the ledger. The migration was checked against the examples and formatter round-trip traps, so A holds.

### D-AUTHORITY-MEM1 — Memory floors ride the rights tree

- **Group:** `safety`
- **Card:** #1500
- **Status:** `ratified` (2026-08-06)
- **Amends:** Amends D-MEM-FACTS1 and D-POLICY-WORD1. Memory floors leave `#Policy`; `#Policy` keeps non-memory policy and unsafe mode.
- **Gist:** Whether memory floors and effect rules share one ladder.

#### Lesson

Jet has two tighten-only ladders. The effect system checks rows and #Caps regions on one lattice. The #Policy ladder checks memory floors (no_alloc, zero_rc, arena_bounded, gc) and the unsafe mode on a second lattice with its own error code. Both enforce the same rule: inner scopes may only tighten. This choice decides whether the memory floors become denials on the rights tree (new Mem leaves) or keep their separate engine.

#### Story

Ana writes firmware for a flight-data recorder. Certification asks: prove this package never allocates after boot and never touches the network. Today those are two proofs from two systems with two vocabularies. She wants one proof from one system.

#### In the wild

```text
// Today: one goal, two ladders, two error families
#Policy(no_alloc)                 // ladder 1 -> E0355 on violation
fn isr_tick() =[!Net]=> Unit {    // ladder 2 -> E0749 on violation
    buffer.push_fixed(sample())
}
// pkg.jet: policy: .{ no_alloc: true }   effects: { deny: [Net] }
```

#### Options

##### A — One ladder, spellings kept

Mem.Alloc, Mem.Rc, and Mem.Gc become rights on the tree. #Policy(no_alloc) keeps its exact spelling and meaning but now records a Mem.Alloc denial on the same ladder #Caps uses. Nothing users type changes, diagnostics keep their codes, and a certification audit reads one ledger.

Technical: Amends D-MEM-FACTS1=B in substrate only: same facts, same transitivity, expressed as tree denials. PolicyKey memory variants retire; the unsafe mode stays a policy value (it is a gate mode, not a right). arena_bounded(N) carries its limit as gate data. E0355 remains the diagnostic for policy-shape errors.

```text
#Policy(no_alloc)                  // unchanged spelling
fn isr_tick() =[!Net]=> Unit { ... }
// jet inspect authority now shows one column:
//   scope isr_tick: denies Mem.Alloc (policy), denies Net (row)
```

##### B — Full respell as effect denials

Retire the memory keys from #Policy and write floors as prohibitions: =[!Mem.Alloc]=> on functions, deny: [Mem.Alloc] in manifests. One mechanism and one spelling, but the friendly floor words (no_alloc) disappear and arena limits need a new home.

Technical: Amends D-MEM-FACTS1 and D-POLICY-WORD1. #Policy keeps only non-memory arguments. arena_bounded needs a parameterized-denial form the row grammar does not have today.

```text
fn isr_tick() =[!Net, !Mem.Alloc]=> Unit { ... }
// pkg.jet: effects: { deny: [Mem.Alloc, Net] }
// open question this option must answer: arena_bounded(65536) as a row?
```

##### C — Two ladders stay

Decline. Memory floors keep their own lattice and error family. The certification story stays split and the substrate keeps two implementations of the tighten rule.

```text
// Unchanged: two proofs, two vocabularies, two inspect surfaces
#Policy(no_alloc)
fn isr_tick() =[!Net]=> Unit { ... }
```

#### Comparisons

- **Pony:** Fused all of aliasing and authority into one six-mode lattice that every beginner faces. Lesson taken: fold the substrate, not the beginner surface — the floor words stay.

```text
iso | val | ref | box | trn | tag  // mandatory vocabulary, day one
```

- **Rust:** no_std and #![forbid(unsafe_code)] are separate systems from the borrow checker. Certification audits there also stitch multiple proofs.

```text
#![no_std] #![forbid(unsafe_code)]
```

#### Recommendation

- **Pick:** `A`
- **Why:** One ladder gives embedded and certification work a single proof and deletes a duplicate lattice. The floor words stay because they read better than row noise, and the unsafe mode stays a gate setting, which keeps the model honest.
- **Why not:**
  - `B`: It trades the ratified, readable floor words for row noise and has no good answer for parameterized floors like arena_bounded.
  - `C`: It keeps two implementations of one rule and a permanently split audit story for exactly the users who audit hardest.
- **Trade-off:** Mem joins the root table, so the closed list grows by one root. That is acceptable because the alternative is a whole parallel lattice for three names.

#### Review passes

- **base:** The first draft compared substrate fold, full respell, and status quo, recommending the fold because it unifies proof without touching ratified spellings.
- **boilOcean:** The breadth pass tested an Unsafe root and per-type floors such as no_alloc on a struct. Unsafe stays a gate with modes rather than a held right, and per-type floors are not this decision, so both were folded out.
- **hybrid:** The hybrid pass tried A plus optional row spellings (=[!Mem.Alloc]=> as sugar for the same denial). That reintroduces two spellings of one mechanism, against I8, and was dropped.
- **cooperative:** The cooperative pass strengthened B with a concrete manifest form for limits and strengthened C by naming its real virtue: zero migration risk to certified code. Both are reflected in the option details.
- **adversarial:** The adversarial pass attacked A on arena_bounded, since a limit is not a set-membership fact; the denial carries its limit as gate data, as grants entries already carry per-dependency data. Tighten-only (D-PACKAGE-POLICY-SCOPE1) survives unchanged as the ladder law, so A holds.

### D-AUTHORITY-NAME1 — Name the authority value that crosses boundaries

- **Group:** `syntax`
- **Card:** #1500
- **Status:** `ratified` (2026-08-06)
- **Amends:** Amends D-AGENT-EXEC2 naming: `ProcessAuthority` becomes `Authority`; `ProcessPlan` and `ProcessReceipt` stay. It also resolves the nameable-value gap recorded by the type-unification audit.
- **Gist:** What the carried rights-set value is called, and whether it is one type.

#### Lesson

When rights cross a boundary the compiler cannot see past — into a spawned process, a plugin, or a session — they must travel as a value. Jet has four designs for that value today: the unnameable #Grant handle, the ratified-but-unbuilt ProcessAuthority, the plugin capability list, and the REPL grant copy. The type name Capability is currently accepted anywhere a type is legal but can never be constructed. This choice picks one type and its name.

#### Story

Priya builds a CI agent in Jet. She spawns a build in a workspace, hands a plugin its rights, and opens a debug session. Three handoffs, and today each would use a different type with a different shape. She wants to write one thing three times.

#### In the wild

```text
// Ratified direction (D-AGENT-EXEC1/2), not yet built:
fn run_build() =[Exec]=> ProcessReceipt {
    spec :: process.workspace()     // read-only repo, no net, no secrets
    process.run("jet build", spec)
}
// Meanwhile, live today:
fn f(x: Capability) => Int = 0     // accepted, then a dead end:
// every call is E0112 and no value of Capability can ever exist
```

#### Options

##### A — Authority — one Prelude type

One type named Authority carries a rights set across every boundary. The #Grant handle has this type, process.workspace() returns it, plugins receive it, sessions hold it. It can only be narrowed, never widened, and it never participates in dispatch. The dead-end phantom is fixed by making the real type exist.

Technical: Amends D-AGENT-EXEC2 naming: ProcessAuthority becomes Authority (ProcessPlan and ProcessReceipt stay). Reopens the type-unification audit’s post-v1 note on typed handles, value form only. Facts stay facts (D-FACTMODEL1): Authority is ordinary data, no type selection, no dispatch. Narrowing is the only operation family (with/without). Construction keeps the ratified PascalCase convention from D-AGENT-EXEC2: Authority.workspace(...), not a lowercase namespace call.

```text
a :: Authority.workspace()         // proposed
a2 :: a.with(FS.Write, "out/")      // extend only within holds; else E0712
process.run("jet build", a2)
plugin.load("lint", a2.without(FS.Write))
```

##### B — Caps — match the marker

Same one-type design, named Caps to rhyme with #Caps(...). Shortest name, already half-taught by the marker. The rhyme cuts both ways: #Caps narrows a static scope while the value crosses runtime boundaries, and one word for both may blur that line.

Technical: Identical semantics to option A. Amends D-AGENT-EXEC2 naming the same way.

```text
c :: Caps.workspace()
process.run("jet build", c.with(FS.Write, "out/"))
```

##### C — Clearance — plain-word metaphor

Same one-type design, named Clearance: what you have been cleared to do. Reads naturally in security prose and to newcomers. It is a fresh word with no anchor in existing Jet surface, so it adds a vocabulary item instead of reusing one.

Technical: Identical semantics to option A.

```text
c :: Clearance.workspace()
process.run("jet build", c.with(FS.Write, "out/"))
```

##### D — Bespoke types per boundary

Honor D-AGENT-EXEC2 exactly as ratified: ProcessAuthority for processes, plugin capabilities for plugins, session grants for the REPL, and the #Grant handle stays unnameable. Each boundary gets a type shaped for its job. Three shapes to learn, and narrowing rules are re-stated per type.

Technical: No amendment. The phantom-type regression is fixed separately by scoping marker-argument names to marker positions (card #1501).

```text
spec :: process.workspace()          // ProcessAuthority
plugin.load("lint", PluginCaps.read_only())
// #Grant handle: still unnameable, still fine for block scope
```

#### Comparisons

- **Fuchsia:** One handle type with a rights mask; duplication may only reduce rights. One narrowing rule serves the whole OS.

```text
zx_handle_duplicate(h, ZX_RIGHT_READ, &out)  // subset only
```

- **Austral:** Linear capability values threaded from main, no ambient authority anywhere. Proof the value form works in a language, unproven at ecosystem scale.

```text
let f: FileCap := open(root, "log.txt");
```

- **WASI:** Everything the guest may touch is a handle it was explicitly given at instantiation.

```text
// no ambient open("/etc/passwd") inside the sandbox
```

#### Recommendation

- **Pick:** `A`
- **Why:** One type with one narrowing rule serves processes, plugins, and sessions alike, lands the ratified agent design as an instance, and turns the Capability dead end into a real, teachable thing. Authority is the plainest word for exactly what the value is.
- **Why not:**
  - `B`: Caps names the static marker; reusing it for the runtime value blurs the static/boundary line the model works hard to keep sharp.
  - `C`: Clearance is a good word with no anchor: it adds vocabulary where Authority reuses the concept the whole model is named for.
  - `D`: Three bespoke shapes re-state the same narrowing law three times and leave the phantom type a permanent trap to re-fix.
- **Trade-off:** Renaming ProcessAuthority amends a ratified naming ruling before any code exists. That is the cheapest possible moment to amend it.

#### Review passes

- **base:** The first draft posed one-type-versus-bespoke and a name menu, recommending one type named Authority.
- **boilOcean:** The breadth pass weighed Warrant, Permit, and Leash, folding them into C as the best fresh-word representative. A generic parameterized form like Authority[FS.Read] re-enters the withdrawn typed-handle design and touches the facts-never-dispatch wall, so it stays out.
- **hybrid:** The hybrid pass tried one type with per-boundary aliases (ProcessAuthority = Authority). Aliases are exactly the parallel-spelling drift greenfield law forbids, so the hybrid was dropped.
- **cooperative:** The cooperative pass gave D its best case: boundary-shaped types can carry boundary-only fields without polluting a shared type. The answer, kept in A’s technical notes, is that boundary data rides the constructor rather than the type.
- **adversarial:** The adversarial pass attacked A with dispatch creep and with revocation. Authority is data and never a dispatch input, and scope end already revokes while boundary values die with their boundary, so A holds.

### D-AUTHORITY-MANIFEST1 — One manifest schema for what a package may do and delegate

- **Group:** `tooling`
- **Card:** #1500
- **Status:** `ratified` (2026-08-06)
- **Amends:** Amends D-EFFBUDGET1, D-JPK-POLICYSURFACE1, and D-JPK-GRANTSCHEMA1. Trust and provider bounds move into `authority:`; `policy:` keeps floors and unsafe mode.
- **Gist:** How pkg.jet spells every grant the package makes.

#### Lesson

Four separate schemas in pkg.jet answer authority questions: effects allow/deny, per-dependency grants, trust prompting (allow/prompt/deny), and provider allow/deny for package sources. Each has its own parser, its own malformed-block error, and its own documentation. They are one kind of record: this subject holds these rights, decided by this default. This choice decides whether the manifest keeps four spellings or gets one.

#### Story

Sam reviews a new dependency for work. He opens pkg.jet to answer: what does this package let its dependencies do? The answer is spread over effects:, grants:, policy.trust, and policy.providers, each shaped differently. He wants one block to read top to bottom.

#### In the wild

```text
// Today: four schemas, four shapes, one question
effects: { allow: [Net, DB.Read], deny: [Exec] }
grants: { "image-codec": [FS.Read] }
policy: .{
  trust: { default: prompt, ci: deny, services: { stripe: allow } },
  providers: { nix: { registry: "nixpkgs", deny: ["openssl-1.0"] } }
}
```

#### Options

##### A — One authority block

A single authority: block holds every grant the package makes: its own bound, per-dependency grants, trust defaults, and provider bounds. Old keys are migrated in one pass and deleted. A reviewer reads one block; the lockfile and the ledger mirror its shape.

Technical: Amends D-EFFBUDGET1 (key spelling only; E1220/E1221 semantics unchanged) and D-JPK-POLICYSURFACE1/D-JPK-GRANTSCHEMA1 (trust and providers relocate from policy:). policy: keeps floors and unsafe mode. One parser, one malformed-block diagnostic family.

```text
authority: .{
  holds: { allow: [Net, DB.Read], deny: [Exec] },
  grants: { "image-codec": [FS.Read] },
  trust: { default: prompt, ci: deny, services: { stripe: allow } },
  providers: { nix: { registry: "nixpkgs", deny: ["openssl-1.0"] } },
}
```

##### B — Keep keys, share the schema

Every current key stays where it is, but all four validate through one shared record shape and report through one diagnostic family. Reviewers still hunt four places; tooling and docs get one model underneath.

Technical: One internal grant-record type; E1221 and the two Bad*Policy manifest errors converge on it. No manifest migration.

```text
// Unchanged surface, one implementation:
effects: { allow: [Net, DB.Read], deny: [Exec] }
grants: { "image-codec": [FS.Read] }
policy: .{ trust: { default: prompt }, providers: { ... } }
```

##### C — Four schemas stay

Decline. Each block keeps its own parser and errors. The next authority surface (agent workspaces, jetos app grants) adds a fifth shape.

```text
// Status quo, plus the fifth block that is already coming:
// agents: { workspace: ..., allow: ... }   // D-AGENT-EXEC1 will need a home
```

#### Comparisons

- **Deno:** All grants in one place (flags) is the readable part of its design, even where the grain is wrong.

```text
deno run --allow-net=api.stripe.com --allow-read=./data app.ts
```

- **Android:** One manifest block for all permissions made review a habit; scattered grant surfaces never became one.

```text
<uses-permission .../>  // one place reviewers actually check
```

#### Recommendation

- **Pick:** `A`
- **Why:** One block makes package review a top-to-bottom read, gives the coming agent and jetos grants a ratified home instead of a fifth schema, and lets the lockfile and ledger mirror one shape.
- **Why not:**
  - `B`: It fixes the plumbing and leaves the reader’s problem: four places to check before trusting a package.
  - `C`: It guarantees a fifth shape the moment agent grants land, which is the pattern that produced this ballot.
- **Trade-off:** Every in-repo manifest migrates once and external examples in docs are rewritten. Accepted: greenfield law prefers one canonical current form over parallel spellings.

#### Review passes

- **base:** The first draft compared one merged block, shared-schema-hidden-keys, and status quo, recommending the merged block for reviewability.
- **boilOcean:** The breadth pass tested a separate authority.jet file and inline per-dependency grant blocks. The separate file splits the manifest story and was rejected, and inline grants were folded into A as formatting freedom rather than schema.
- **hybrid:** The hybrid pass tried A for effects and grants but leaving trust in policy:. That keeps two homes for allow/prompt/deny decisions and died on its own test: where would a reviewer look for service trust?
- **cooperative:** The cooperative pass strengthened B with the observation that it is zero-migration and fully invisible to users, now stated plainly in its detail. C gained its honest framing: it is the default outcome of doing nothing.
- **adversarial:** The adversarial pass confirmed that moving trust out of policy: amends the ratified one-policy-namespace ruling, and the ballot names that amendment rather than hiding it. Floors stay in policy: so tighten-only stays meaningful, and A holds.

### D-AUTHORITY-GATE1 — One audit ledger for every authority gate

- **Group:** `safety`
- **Card:** #1500
- **Status:** `ratified` (2026-08-06)
- **Amends:** Generalizes D-LINTPOLICY1's bypass-recording clause to every authority gate. Existing per-kind views remain filtered views.
- **Gist:** Where the record of every granted exception lives.

#### Lesson

A gate is any written widening of authority: an #Unsafe reason, a per-dependency grants: entry, an --allow flag, a trust grant. The ratified lint ruling already states the law for one gate kind: every bypass is spelled at the site and lands in the audit record. Today each gate kind keeps its own record: unsafe inspection, the lockfile, the trust file, session flags. This choice decides whether those records become one ledger with one reader.

#### Story

An auditor gives Maya’s billing service two hours. Their first question: list every exception this system granted itself. Today that answer is assembled from jet unsafe, the lockfile diff, ~/.jet/trust, and shell history. She wants one command.

#### In the wild

```text
$ jet inspect authority              # proposed
gates:
  src/dma.jet:41   #Unsafe "MMIO: board manual §4.2"  obligations: discharged
  pkg.jet          grants image-codec: FS.Read        lockfile: recorded
  build            --allow-net (jet build, 2026-08-06)
  user             trust grant build ./vendor-tool (2026-08-02)
```

#### Options

##### A — One ledger, one reader

Every gate kind records to one ledger and jet inspect authority reads it all: unsafe gates with obligation status, dependency grants, build and session flags, trust grants. The lint ruling’s bypass law becomes the stated law of every gate. Existing per-kind views keep working as filtered views.

Technical: Generalizes D-LINTPOLICY1’s recording clause to all gates. Sources stay where they are (source markers, lockfile, trust store); the ledger is the merged, provenance-keeping read model. No new write surface.

```text
$ jet inspect authority --scope package
$ jet inspect authority --kind unsafe     # today's jet unsafe, preserved
$ jet inspect authority --json            # CI diffs the ledger
```

##### B — Ledger in artifacts only

The merged record exists but only as a build artifact (lockfile section and receipts), with no new command. CI can diff it; humans assemble the story from files, as today.

Technical: Same merged record, serialized into the lock and receipts. No CLI addition.

```text
# CI-only consumption:
$ git diff jet.lock   # shows new grants and new unsafe gates as diff lines
```

##### C — Per-kind records stay

Decline. Unsafe, budget, trust, and session records stay separate with separate readers. Nothing new to learn, and the auditor’s question stays a four-source scavenger hunt.

```text
$ jet unsafe && git diff jet.lock && cat ~/.jet/trust && history | grep allow
```

#### Comparisons

- **Rust:** Unsafe hygiene is culture plus scattered tools (Miri, cargo-geiger). No single ledger; audits re-assemble it every time.

```text
$ cargo geiger   # counts unsafe, one dimension only
```

- **Deno:** Grants exist only as the command line; nothing records what a past run was allowed to do.

```text
$ history | grep -- --allow-   # the "audit trail"
```

#### Recommendation

- **Pick:** `A`
- **Why:** The model’s law has two halves and this is the second one: widenings are not only written at the site but readable in one place. One reader turns a scavenger hunt into a command and makes the exception list diffable in CI.
- **Why not:**
  - `B`: Artifacts serve machines. The human question — what did we grant ourselves — deserves a first-class answer.
  - `C`: Four separate records is how exceptions hide; the audit that motivated this found the trust store, budget, and unsafe views already disagreeing in vocabulary.
- **Trade-off:** The ledger merges records of different lifetimes (source, lock, machine, session), so provenance labels must be carried honestly. Accepted: provenance is exactly what an auditor needs anyway.

#### Review passes

- **base:** The first draft compared one reader, artifact-only, and status quo, recommending the reader as the second half of the bypass law.
- **boilOcean:** The breadth pass tested failing builds on unacknowledged new gates and a web view of the ledger. Enforcement is a later lint-style policy layer and the web view is presentation, so both folded out of this decision.
- **hybrid:** The hybrid pass combined A and B: the reader consumes the same merged record the artifacts carry, so B is A’s storage half rather than a rival. The options were rewritten to make that explicit.
- **cooperative:** The cooperative pass gave B its best case (zero new surface, CI-first teams already live in diffs) and C its honest one (nothing new to learn). Both survive in their details.
- **adversarial:** The adversarial pass attacked A on privacy and staleness. Unsafe reasons stay package-local unless published and session entries carry their lifetime label, so A holds.

### D-AUTHORITY-WORD1 — Retire the word capability from surfaces or reserve it for one thing

- **Group:** `syntax`
- **Card:** #1500
- **Status:** `ratified` (2026-08-06)
- **Amends:** Amends the user-facing wording selected by D-AUTHORITY-NAME1 and the borrow diagnostic wording. It adds no grammar and keeps design-history prose allowed.
- **Gist:** What the word capability is allowed to mean in Jet.

#### Lesson

The word capability means six unrelated things here: the unnameable grant-handle type, a marker-argument enum, borrow-diagnostic wording, the product claim manifest, the capability ledger card, and the remote-builder capability model. A word that means everything explains nothing, and two of the six sit in user-facing diagnostics. This choice picks one meaning or retires the word from surfaces.

#### Story

Leo hits two errors in one afternoon. One says a value needs a write capability (borrowing). The other says his #Grant handle, a Capability, escaped its region (authority). Same word, unrelated systems. He searches the docs for capability and gets both chapters interleaved.

#### In the wild

```text
// Same word, two shipped diagnostic families:
// E07xx: "the loop takes the write capability (&) for items"   <- borrowing
// E0711: "the Capability handle escapes the #Grant region"     <- authority
// plus: feature-claim-manifest.json, card #1499 "Capability ledger",
// card #422 "Remote builders: capability model"
```

#### Options

##### A — Retire from surfaces

No user-facing surface uses the word capability. Borrow diagnostics say write access. The authority value takes whatever name D-AUTHORITY-NAME1 picks. The product claim manifest becomes feature claims; internal cards rename at next touch. Docs may still say capability-based when describing the design tradition.

Technical: Diagnostic copy is snapshot-tested product text (I4): the &/^ wording change re-blesses those snapshots. No grammar change; no code identifier named Capability remains reachable by users.

```text
// After: one afternoon, two clear errors
// "the loop takes write access (&) for items"
// "the Authority handle escapes the #Grant region"  (name per NAME1)
```

##### B — Reserve for the authority value

Capability becomes the authority value’s name (overriding the NAME1 menu) and every other use renames. The industry term lands on exactly the industry concept. The borrow wording still changes, and the word’s six-way history keeps echoing in old docs and searches.

Technical: Binds NAME1 to the name Capability. Same renames elsewhere as option A.

```text
c :: capability.workspace()
process.run("jet build", c.with(FS.Write, "out/"))
```

##### C — Leave the word alone

Decline. All six meanings stay. Cheapest today; the docs search problem, the interleaved chapters, and the double-meaning diagnostics remain.

```text
// Leo's afternoon stays as-is: two systems, one word.
```

#### Comparisons

- **Pony:** Called its aliasing modes reference capabilities; the overlap with object-capability security confuses newcomers to both fields to this day.

```text
iso, val, ref  // "capabilities" that are about aliasing, not authority
```

#### Recommendation

- **Pick:** `A`
- **Why:** Retiring the word ends the collision at its root: each concept gets a word that means only itself, and the borrow diagnostics get plainer at the same time. Access is what & takes; authority is what a scope holds; neither needs the loaded word.
- **Why not:**
  - `B`: It rescues the word by spending it on one meaning while five renames happen anyway, and it preempts the NAME1 menu from inside a different ballot.
  - `C`: Six meanings in one project is a documentation and teaching tax that compounds with every new page.
- **Trade-off:** Searches and older external writing that say capability will not match surface text. Accepted: docs keep the term as a described tradition, so discovery survives.

#### Review passes

- **base:** The first draft posed retire, reserve, and decline, recommending retire because both shipped diagnostic uses read better without the word.
- **boilOcean:** The breadth pass tested namespaced uses such as borrow capability versus authority capability, and rejected them because qualified jargon is still jargon in an error message. Product-manifest rename wording folded into A as detail.
- **hybrid:** The hybrid pass tested retiring the word everywhere except documentation titles. That is option A as written, since surfaces retire the word while prose may describe the tradition, so no separate option was needed.
- **cooperative:** The cooperative pass strengthened B: if the industry word must live somewhere, the authority value is its one honest home, and the option now says so plainly. C’s cost was restated without exaggeration.
- **adversarial:** The adversarial pass attacked A on discoverability for newcomers who search the word capability, and on snapshot churn. Discovery survives through descriptive prose and re-blessing snapshots is the normal cost of copy changes, so A holds.

### D-AUTHORITY-SCOPE1 — One scope marker: #Caps absorbs #Grant

- **Group:** `syntax`
- **Card:** #1500
- **Status:** `ratified` (2026-08-06)
- **Amends:** Amends D-EFF1, D-SCAP1, and D-ARROW-CONTROL1. `#Grant` is deleted; its handle head moves to named `#Caps`, with E0711/E0712 and scope-end revoke retained.
- **Gist:** Whether one marker or two set the rights of a block.

#### Lesson

Two markers set what a block may do. #Caps(Net) narrows the block to a list of rights. #Grant(caps: FS) sets a list and binds a handle that ends with the block. Both check the same rule with the same names, but they use two spellings and two error families. This choice decides whether the two markers merge into one.

#### Story

Ines reviews a teammate’s service. One function narrows with #Caps, the next grants with #Grant, and she has to stop and ask why. The answer is history, not meaning. She wants one marker whose one difference — a named handle — is visible in its head.

#### In the wild

```text
// Today, two spellings for one relation:
#Caps(DB.Read) {
    db.query(req.id)          // outside the list -> E0741
}
#Grant(caps: FS, Net) {
    backup()                  // outside the list -> E0712
}                             // handle escape -> E0711
```

#### Options

##### A — One marker, optional handle head

#Caps does both jobs. A bare list narrows, as today. A name before the list binds the handle: #Caps(g: FS, Net). #Grant is deleted and every use migrates in one pass. One spelling, one error family, one thing to teach.

Technical: Amends D-EFF1 (region form), D-SCAP1 and D-ARROW-CONTROL1 (the caps: head moves to a name head on #Caps). The handle keeps S63 scope-end revoke and E0711/E0712 checks; E0741 merges into the same check. REPL host gating keys off the same marker.

```text
#Caps(DB.Read) { db.query(id) }      // narrow, unchanged
#Caps(g: FS, Net) { backup() }       // narrow + handle g (proposed)
// #Grant(...) no longer parses; the fix names the #Caps form
```

##### B — One marker, handle always bound

Same merge, but every #Caps block binds a handle, named or not: #Caps(_: DB.Read). One form with no optional part. The common narrow-only case pays a head it never uses.

Technical: Same amendments as option A. The anonymous head spelling follows the existing wildcard convention.

```text
#Caps(_: DB.Read) { db.query(id) }   // handle unused
#Caps(g: FS, Net) { backup() }
```

##### C — Keep both markers

Decline. #Caps narrows and #Grant grants, and the two verbs teach the two directions. The cost stays: two spellings and two error families for one relation, and every reader must learn why.

```text
#Caps(DB.Read) { ... }
#Grant(caps: FS, Net) { ... }        // unchanged
```

#### Comparisons

- **Fuchsia:** One operation — duplicate a handle with fewer rights — serves every narrowing job in the OS. One verb, learned once.

```text
zx_handle_duplicate(h, subset_rights, &out)
```

#### Recommendation

- **Pick:** `A`
- **Why:** Under the one law both markers set the holds of a block, so one marker is the honest spelling. The optional name head keeps the narrow-only case clean and makes the handle case visible at a glance.
- **Why not:**
  - `B`: A forced anonymous head taxes the most common case for a symmetry no reader asked for.
  - `C`: Two spellings for one relation is exactly the surface debt this proposal exists to delete, and the grant-versus-narrow story dissolves once the attenuation law holds everywhere.
- **Trade-off:** The word Grant leaves the marker surface, so the grant idea lives in manifests, flags, and the trust store only. That is acceptable because the block form never granted beyond what the outer scope held.

#### Review passes

- **base:** The first draft posed merge-with-optional-handle, merge-with-mandatory-handle, and keep-both, and recommended the optional-handle merge.
- **boilOcean:** The breadth pass tested a fourth shape: block-level effect rows (=[Net]=> on a block) instead of any marker. It was rejected because a row states a contract on a signature while a block sets a scope, and mixing the two spellings blurs where contracts live.
- **hybrid:** The hybrid pass tried keeping #Grant as sugar for the handle form of #Caps. Two spellings for one meaning is the drift this ballot deletes, so the hybrid died on its own test.
- **cooperative:** The cooperative pass strengthened C: two verbs can teach direction, and its detail now says so plainly. It also strengthened B with the wildcard-head convention so the forced head is at least familiar.
- **adversarial:** The adversarial pass attacked A on REPL gating, which today keys off #Grant, and on migration blast radius. The gate keys off the handle head with no behavior change, and the migration is one mechanical pass over a small marker population, so A holds.

### D-AUTHORITY-MEM2 — Where a bounded arena limit lives after memory keys leave #Policy

- **Group:** `safety`
- **Card:** #1500
- **Status:** `ratified` (2026-08-06)
- **Amends:** Amends the acceptance terms of D-AUTHORITY-MEM1=B. The denial row gains the closed `above: Bytes` argument so bounded arenas stay inside the one rights mechanism.
- **Gist:** How code declares: this function may allocate at most N bytes.

#### Lesson

D-AUTHORITY-MEM1 chose option B: memory floors become effect denials, written =[!Mem.Alloc]=> on functions and deny: [Mem.Alloc] in manifests, and the memory keys leave #Policy. The ratified option text left one hole it named itself: arena_bounded(65536) carries a number, and denial rows carry none. Until the number has a home, the memory-floor implementation card cannot start.

#### Story

Sam ships a firmware package for a device with 64 KB of spare RAM. The embedder asks for a written promise: this package never allocates past that line. Sam had #Policy(arena_bounded(65536)); after MEM1 that spelling is retired and nothing yet replaces it.

#### In the wild

```text
#Policy(arena_bounded(65536))
fn decode_frame(input: Bytes) => Frame {
    // the promise: everything here fits in one 64 KB arena
}
```

#### Options

##### A — Parameterized denial row

The denial row grammar gains arguments. A plain denial stays =[!Mem.Alloc]=>; a bounded one is =[!Mem.Alloc(above: 65536)]=>, and manifests write deny: [Mem.Alloc(above: 65536)]. One mechanism carries both the floor and the number, in the one place a reader already checks.

Technical: The rights tree row for Mem.Alloc accepts an optional above: Bytes argument. The authority gate records the number in the audit ledger with the denial. Sema enforces the unbounded form statically where it can and the bounded form at the allocator, on every tier. The row rides the signature effect arrow, in the return position, never a line above the function.

```text
fn decode_frame(input: Bytes) =[!Mem.Alloc(above: 65536)]=> Frame { ... }

// pkg.jet
deny: [Mem.Alloc(above: 65536)]
```

##### B — A typed setting on the fact plane

The bound becomes a typed build fact, arena_ceiling :: Bytes, on the settings plane the build/config law just created. The denial row stays parameterless. The reader must now check two places to learn one memory law: the row for the floor, the setting for the number.

```text
// pkg.jet settings
arena_ceiling :: 65536 bytes

fn decode_frame(input: Bytes) =[!Mem.Alloc]=> Frame { ... }
```

##### C — One surviving #Policy spelling for bounds

#Policy(arena(65536)) stays only for the bounded case. This reopens what MEM1 retired two days ago: the memory keys were removed from #Policy so that memory law has one mechanism. The friendly spelling returns, and so does the second mechanism.

```text
#Policy(arena(65536))
fn decode_frame(input: Bytes) => Frame { ... }
```

##### D — No declarative bound: an arena API in code

The bound is not a declaration. Code opens a bounded arena and runs inside it: arena(65536).run { }. Simple and explicit, but a manifest can no longer promise a cap to an embedder, which is the case MEM1 exists for.

```text
fn decode_frame(input: Bytes) => Frame {
    arena(65536).run {
        ...
    }
}
```

#### Comparisons

- **Zig:** Allocation bounds are values in code: a FixedBufferAllocator over a fixed buffer, like option D. Zig has no declarative package-level cap.

```text
var buf: [65536]u8 = undefined;
var fba = std.heap.FixedBufferAllocator.init(&buf);
```

- **Java:** The cap is an external runtime flag (-Xmx), not a fact the code carries. Nothing in the package declares it.

```text
java -Xmx64k App
```

#### Recommendation

- **Pick:** `A`
- **Why:** MEM1 chose one mechanism for memory law: the rights tree. Giving the denial row an argument keeps the number inside that one mechanism, visible at the same line a reader already checks, and the audit ledger records it with the denial.
- **Why not:**
  - `B`: It splits one memory law across two planes, so the floor and its number live in different places and can drift apart.
  - `C`: It reopens the retirement MEM1 made two days ago and restores the second mechanism that vote removed.
  - `D`: It loses the declarative promise. A package could no longer state its cap to an embedder, which is the very case this law serves.
- **Trade-off:** The row grammar gains arguments, and every consumer of rights rows — sema, the gate, the audit ledger, reflection — must carry the number through.

#### Review passes

- **base:** The first draft compared a parameterized row, a settings-plane fact, a surviving #Policy key, and a code-only arena API, and recommended the parameterized row for staying inside MEM1's one mechanism.
- **boilOcean:** The breadth review tested a refinement-type spelling on the function's return type and a comptime assertion. Both were folded out: they state the bound where no embedder or manifest can read it.
- **hybrid:** The hybrid review tested row-plus-API: the declarative row for promises and the arena API as its runtime enforcement. That is not a separate option; option A already implies an enforcing allocator, so the pairing was noted inside A's technical text.
- **cooperative:** The cooperative review strengthened B: typed settings are real law now and give the bound a typed home with provenance. It still leaves the floor and the number in two places.
- **adversarial:** The adversarial review asked whether above: opens the row grammar to arbitrary predicates. The repair is a closed argument list registered per right, and with that fence the recommendation survives.
