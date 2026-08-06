# Authority: one model

Proposal, 2026-08-06. Companion ballots: D-AUTHORITY-MODEL1, D-AUTHORITY-ROOTS1,
D-AUTHORITY-MEM1, D-AUTHORITY-NAME1, D-AUTHORITY-MANIFEST1, D-AUTHORITY-GATE1,
D-AUTHORITY-WORD1.

## Executive summary

Jet answers one question in at least nine different places: **who may do what,
in what scope, granted how**. The effect system answers it for functions. The
`#Caps` and `#Grant` markers answer it for blocks. The `#Policy` ladder answers
it for memory and unsafe floors. The package effect budget answers it for
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
it once, and re-founds all nine mechanisms on it. No spelling that users type
today changes unless a ballot says so. Every capability is kept. Several
ratified-but-unbuilt decisions (D-AGENT-EXEC2, D-JOS-INSTALLTRUST1,
D-JPK-SANDBOX2) land on the new substrate instead of adding a tenth copy.

Why now: the drift is already user-visible. `jet explain E1221` tells users the
effect vocabulary has ten roots while the compiler accepts twenty-eight.
`=[Java]=>` compiles today with no spec text behind it. The `Capability`
phantom regressed from an honest "unknown type" error to a dead-end type error.
The agent executor and jetos install-trust work are ratified and about to be
built — building them on a shared substrate is one-time work; building them as
copies ten and eleven is permanent debt.

The ballots ask seven direction-level questions: adopt the model; heal the root
table; fold the memory floors in; name the authority value; unify the manifest
schema; adopt the audit law; reserve the word "capability". Each stands alone.
What does not change: effect inference, erasure, every diagnostic families'
meaning, the beginner's zero-annotation experience, and every frozen wall.

The score, in order. **Deleted:** the `BuildEffect` fork, the second
tighten-only lattice, the duplicate purity checker, three of four manifest
schemas, the dead-end phantom types, and five of six meanings of one word.
**Kept:** every capability, every ratified spelling, every diagnostic meaning,
zero-cost erasure. **Gained:** a nameable authority value for processes,
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
table: the sixteen FFI-language roots become `FFI.*` leaves, memory floors
become `Mem.*` leaves if D-AUTHORITY-MEM1 adopts them, and build actions and
host operations reuse the same names they already reuse informally. Unsafe
does not become a right: it is a gate — the audited way to widen — and gates
are the other half of the law, not entries in the holds-set.

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

1. **`#Policy(no_alloc)` is `#Caps` minus `Mem.Alloc`.** The memory floors are
   rights denials on the same ladder; the two tighten-only lattices in
   `Policy.rs` and `Effects.rs` are one lattice implemented twice.
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

Spelling principles the model implies:

1. **What you type today stays.** `=[Net]=>`, `#Caps`, `#Grant`, `#Unsafe`,
   `#Policy`, `effects:`, `grants:`, `--allow-*` all keep their meaning. The
   re-founding is substrate-first; surface changes are separate ballots.
2. **One name per idea.** The rights tree has one vocabulary everywhere: the
   same name in a row, a `#Caps` list, a manifest, a flag, and a trust grant.
3. **The authority value gets a name.** The opaque handle `#Grant` binds, the
   value a process receives, and the set a plugin holds become one nameable
   Prelude type (name balloted; the word "Capability" is either claimed for
   exactly this or retired from it).

Proposed syntax slate (each item marked; nothing here is typed until ratified):

| Item | Status |
|------|--------|
| `=[Net, FS.Read]=>` rows, `#Caps`, `#Grant(caps: …)`, `#Unsafe("reason")`, `#Policy(…)` | ratified, unchanged |
| `effect FS.Read` leaf declaration | ratified (D-EFFECT-DECL1), unchanged |
| Root table: thirteen closed roots — `Net FS IO DB Time Rand Env Exec Log GPU FFI Browser Secret`; FFI languages become `FFI.Go`, `FFI.Java`, … leaves; `Mem` joins as a fourteenth root only if D-AUTHORITY-MEM1 is adopted | proposed (D-AUTHORITY-ROOTS1; amends D-EFF4/D-EFF5 and the effect clauses of the D-FFI-*1 ballots) |
| `Mem.Alloc`, `Mem.Rc`, `Mem.Gc` leaves; `#Policy(no_alloc)` desugars to a `Mem.Alloc` denial on the one ladder | proposed (D-AUTHORITY-MEM1; substrate change, spelling kept) |
| A nameable authority type (name balloted: `Authority`, `Caps`, `Clearance`, `Warrant`, `Grant`) usable at boundaries: process spawn, plugin instantiation, session | proposed (D-AUTHORITY-NAME1; lands D-AGENT-EXEC2's `ProcessAuthority` as an instance) |
| One manifest authority schema shared by `effects:`/`grants:`/`policy.trust`/`policy.providers` | proposed (D-AUTHORITY-MANIFEST1; may keep all current key spellings) |
| `jet inspect authority` — one ledger of every gate: unsafe reasons, dep grants, flags, trust grants | proposed (D-AUTHORITY-GATE1) |
| "capability" reserved for the authority value; `&`/`^` diagnostic copy says "write access"; product claim ledger renamed | proposed (D-AUTHORITY-WORD1) |

## What it looks like

**Beginner — writes nothing, gets least privilege.** (All lines ratified
syntax; behavior today, kept.)

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

**Middle — bounds and scoped narrowing.** (Ratified syntax, kept.)

```jet
fn handle(req: Request) =[Net, DB.Read]=> Response {
    #Caps(DB.Read) {
        // Net is out of reach here; a stray net.get is E0741
        db.query(req.id)
    }
    respond(req)
}
```

`pkg.jet` (ratified, D-EFFBUDGET1 / D-PACKAGE-POLICY-SCOPE1):

```jet
policy: .{ unsafe: .Forbid }
effects: { allow: [Net, DB.Read, Log], deny: [Exec] }
grants: { "image-codec": [FS.Read] }
```

**Expert — boundaries and gates.** (Marked lines proposed.)

```jet
fn build_worker() =[Exec]=> ProcessReceipt {
    a :: Authority.workspace()           // proposed: D-AUTHORITY-NAME1 —
                                         // reified rights: read-only repo,
                                         // no Net, no Env, no secrets
    a2 :: a.with(FS.Write, "out/")       // proposed: attenuate-or-extend only
                                         // within what this scope holds
    process.run("cargo build", a2)       // ratified direction: D-AGENT-EXEC1/2
}

#Unsafe("MMIO: board manual §4.2, register is write-once") {
    mem.volatile_write(reg, 1)
    assert valid_ptr, aligned            // ratified: D-UNSAFE-OBLIG1
}
```

Audit read-back (proposed, D-AUTHORITY-GATE1):

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
  controller package writes `policy: .{ }` floors and effect denials in one
  vocabulary and gets one audit ledger for certification.
- **Trivial one-liners**: unchanged — zero annotations, one build line.
- **Teaching**: one sentence covers the whole security story: "your code can
  only do what its scope holds, holds only shrink, and every exception is
  written down."

## What does not change

- Effect inference, erasure, and zero runtime cost (byte-identical Rust).
- Every ratified spelling: rows, `#Caps`, `#Grant`, `#Unsafe("reason")`,
  `#Policy(…)`, `effect` declarations, `effects:`/`grants:` keys, `jet trust`.
- All diagnostic meanings; codes stay (E0740/E0741/E0711/E0712/E1220/E3112…).
- Frozen walls: no top type, no HKT, no macros, comptime never creates types,
  facts never select runtime types or dispatch (the reified authority value is
  ordinary data, never a dispatch input), I1-I9 (I1, I3, I9 are strengthened:
  one Prelude substrate, sema-only checking, one meaning on every tier).
- Availability mechanisms (runtime layers, target profiles, web partition)
  and data tags stay what they are.

## Decisions for the owner

| Ballot | Question | Standalone? |
|--------|----------|-------------|
| D-AUTHORITY-MODEL1 | Adopt the one relation + attenuation law as the substrate all nine mechanisms share (internal re-founding, no surface change) | yes |
| D-AUTHORITY-ROOTS1 | Heal the root table: thirteen closed roots with FFI languages as leaves; amend D-EFF4/5; fix E1221 text | yes |
| D-AUTHORITY-MEM1 | Memory floors ride the rights tree (`Mem.*`); one ladder, spellings kept | needs MODEL1 |
| D-AUTHORITY-NAME1 | Name and reify the authority value for boundaries (menu: Authority, Caps, Clearance, Warrant, Grant) | needs MODEL1 |
| D-AUTHORITY-MANIFEST1 | One manifest authority schema behind the existing keys | yes |
| D-AUTHORITY-GATE1 | One audit ledger + `jet inspect authority`; generalize D-LINTPOLICY1's bypass law to every gate | yes |
| D-AUTHORITY-WORD1 | Reserve "capability"; rename the five other uses | yes |

Bugs found during research (cards, not ballots): the `core_type_known`
marker-argument regression; the E1221 ten-vs-28 explain text; the stale
"ten/twelve roots" comments; the D-FFI-PY1 promised-but-missing `Py` root.
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
- **Phase C — balloted surface unifications**, each a coherent greenfield
  migration that deletes the replaced form: the root-table heal, the memory
  fold, the authority value, the manifest schema, the ledger, the word.
