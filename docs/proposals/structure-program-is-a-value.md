# The program is a value: engineering principles as compiler facts

Status: proposal. Audit run 2026-08-19 against the classic principles of software engineering, on the owner's request. Everything marked **proposed** awaits a ballot; everything with a decision ID is ratified law cited as-is.

## Executive summary

The audit asked one question: which principles of software engineering does Jet enforce, which does it merely permit, and which does it force programs to violate? The answer splits clean. Jet has already turned more principles into compiler law than any shipped language: information hiding is private-by-default with a five-grade ladder (S18, D-PUBPKG1, D-SHAPE-INTERNAL1), import cycles are hard errors (E0604), inheritance does not exist and delegation is compiler-written (S62), contracts run on every tier (D-PREPOST1, D-FAIL-TIER1), mutation is visible at every call site (D-MEM1), and least privilege has a whole effect-and-authority stack (D-EFF1..5, D-AUTHORITY-*). No memory-safe compiled peer ships this set.

The gaps are just as clean, and they are all the same gap. Five truths about a program's own structure have no home today: whether a name is **used**, what lifecycle **stage** an API is in, which **edges** the import graph may grow, where a cross-cutting **policy** lives, and how a repeating **registry** says itself once. Each of these is a fact about the program — and Jet already ratified the law that facts obey: one home, silent tightening, one written word to loosen, on the record (D-FACT-LAW1). The one idea of this proposal: **the program is a value; its structure is one fact plane under the existing fact law**. No new mechanism. Five missing instances of three ratified laws — the fact law, the override law (D-LINTPOLICY1), and the one compile-time program (D-META-*).

Concrete payoffs: dead code becomes visible the moment it dies (today `jet check` says `ok` over an unused import, an unused function, and an unused binding — probed, this run); every package gets the deprecation machinery Core hoards in a hardcoded two-row table inside `#[allow(dead_code)]` staging (crates/jet-sema/src/Sema/Edition.rs:34); architecture rules become one manifest key shaped exactly like the effect budgets that already exist (D-EFFBUDGET1) — a surface no mainstream language owns, which is why the ArchUnit/import-linter tool family exists; the field-audit decision on cross-cutting policy (docs/audits/field-audit-2026-08-03.md:179) finally gets its ballot; and the standing DRY escapes close — the stdlib's own Units.jet spends ~700 of 911 lines copy-pasting 24 SI-prefix rows 28 times because templates cannot reach a marker body.

The ballots ask six direction-level questions: adopt the plane, adopt liveness verdicts, adopt one lifecycle ladder, adopt edge rules, choose the callable-policy shape, and let templates reach registries. Each stands alone; any subset can be adopted. What does not change: the visibility ladder, the borrow prover, the trait system, the effect vocabulary, the no-macro walls, and every beginner default — the lowest rung of every element in this proposal is "you type nothing and today's programs mean exactly what they meant."

Also found and filed separately (bugs, not ballots): trait-value coercion at argument position rejects what spec.md:814 promises (`fn f(s: Shape)` — probed E0112), a struct declared in an inline module is invisible to its own module (probed E0119; ui_showcase.jet:16 documents shipping with the workaround), and E0113 emits a self-contradictory fix ("promises `bank.Account`, returns `vis2.bank.Account`, fix: use `bank.Account`").

## The problem, briefly

Jet's constitution already contains an engineering-principles engine. What it lacks is coverage: the engine runs for values, rights, effects, and time, but not yet for the program's own structure. First the scorecard, then the five-coats table that proves the gaps are one thing.

### The scorecard: every principle, one row

| Principle | Jet today | Evidence | Verdict |
|---|---|---|---|
| Information hiding (Parnas) | Private by default; `pub`, `pub(package)`, soft-public `pub _name` (L0601), `_name` ladder; explicit `pub use` facades; nothing auto-surfaces | S18, D-MOD3/4, D-PUBPKG1, D-SHAPE-INTERNAL1, D-NAME-SIGIL1; probed E0605/E0609 | **Enforced.** Best visibility ladder in the field |
| Acyclic dependencies | Import cycles, package cycles, formula cycles all hard errors; package depth capped at 1 | E0604, E1322-4, E0338; probed | **Enforced by construction** |
| Composition over inheritance | Inheritance does not exist; delegation is compiler-written `impl Trait using field` | philosophy.md:229; S62; examples/features/modules/library.jet:57 | **Enforced by construction** |
| Design by contract / fail fast | `#Pre`/`#Post` on every tier, erased under proof; `validate` blocks; must-use results; typestate; transactions | D-PREPOST1, D-FAIL-TIER1, D-VALIDATE1, E0402, D-STATE1, examples/features/contracts/pre_post.jet | **Enforced.** No compiled memory-safe peer ships this |
| Least privilege | Inferred effect rows, `#Caps` ceilings, package effect budgets, dataflow tags, sandbox target | D-EFF1..5, D-EFFBUDGET1, spec.md:3013-3220 | **Enforced** (authority substrate ratified-unbuilt, D-AUTHORITY-*) |
| Command-query separation | Mutation marked in signature and mirrored at call site (`&`, `^`); read is the default | D-MEM1, D-MUTSELF1, E0202/E0205 | **Visible** (marking enforced; strict CQS deliberately not) |
| Illegal states unrepresentable | Payload enums, `distinct Int(0..255)`, typestate, session protocols | D-TYPE2-*, D-STATE1, D-PROTO1 | **Supported**; corpus rarely reaches for it; stdlib itself ships stringly watcher events (watcher.jet:31) |
| Least astonishment / one way | One mechanical path is invariant I8; wildcard imports rejected; no invisible auto-imports | I8, E0612, D-NAME-FILES1=C | **Enforced** |
| Open/closed | Enums + traits cover both directions; public API semver snapshot (ApiFreeze E1218/E2601); extension tiers with a hard lid | S48, spec.md:4062, D-EXT1 | **Supported**; user packages cannot declare deprecation — evolution machinery is Core-only and unwired |
| DRY | Generics, generic modules, derive templates, `@loop` over fields; say-it-once is law for the compiler's own corpus | D-GENMOD*, D-META-*, D-ONCE-LAW1 | **Supported with forced escapes**: templates cannot reach marker bodies, impl items, or test/bench blocks — see the numbers below |
| Separation of concerns (cross-cutting) | `#Context` exists but its keys are compiler-known (`deadline:`, `allocator:`); no signature-preserving transform; no middleware value | spec.md:2432; field-audit-2026-08-03.md:156-184 | **Structural gap.** Logging/tracing/retry/auth must be hand-scattered through every call site |
| Dependency inversion | Trait in type position auto-boxes (S48); generic bounds monomorphize | S48, probed `[Shape]` path works | **Supported — but drifted**: direct argument coercion rejects (E0112), and `&`/`^` functions cannot become values, so a mutating dependency cannot be injected |
| Interface segregation | Traits are nominal, arbitrarily small, signature-only | S28 | **Supported** |
| Package stability | ApiFreeze named-delta errors; depth-1 packages; effect budgets per dependency | E1218/E2601, D-JPK-REF1, D-EFFBUDGET1 | **Supported** for API shape and effects; **absent** for import direction |
| Dead-code hygiene / YAGNI | Nothing. No unused-import, unused-binding, unused-function, or unreachable-pub diagnostic exists in the entire L-table | diagnostics.md L-table; probed: `jet check` prints `ok` over all three | **Absent** |
| Law of Demeter | No surface; corpus is nearly clean anyway (~3 genuine reach-throughs in 613 files) | corpus audit | **Absent on purpose** — kept absent (see What stays) |
| SRP as a lint | No surface; one planning card for a complexity budget (c0sbsdkf) | Tower c0sbsdkf | **Absent on purpose** — depth beats responsibility-counting (see What stays) |

### Where Jet loses today

Honesty first: on the missing rows, Jet loses to tooling ecosystems it otherwise beats. `cargo clippy`, `eslint`, and even `go vet` all report unused code; Jet reports nothing. Rust has `#[deprecated(since, note)]` for every crate; Jet's deprecation type sits in dead staging and covers two Core items. Java teams enforce layer rules with ArchUnit; Jet cannot express a layer rule at all. Python decorators centralize cross-cutting policy; the field audit already conceded that vector (field-audit-2026-08-03.md:52). These are the four losses, and all four sit in this proposal's scope.

### Five coats of one law

Every gap is the same underlying thing — a truth about program structure with no home. The proof is that Jet already does each of these jobs *somewhere*, in a fragment:

| The job | Done today by | Home | Defect |
|---|---|---|---|
| "This name is past its prime" | `L0601` soft-public warning | shipped (D-SHAPE-INTERNAL1) | one rung, no ladder |
| "This name is retiring, use X" | `DEPRECATIONS` table, 2 hardcoded rows | crates/jet-sema/src/Sema/Edition.rs:34, `#[allow(dead_code)]` | closed to users, unwired |
| "This API's shape is a promise" | ApiFreeze semver snapshot | shipped (E1218/E2601) | enforcement half without a declaration half |
| "This spelling is retired" | `@retired` rows in Markers.jet; D-ONCE-RETIRE1 ratchets | shipped / ratified-unbuilt | compiler surface only |
| "This value must be used" | must-use + `.drop("reason")` | shipped (E0402, D-MARK-DISCARD1) | values only — the same fact about a *name* does not exist |
| "This dependency may not do X" | effect budgets `effects: { allow: […], deny: […] }` | ratified (D-EFFBUDGET1) | effects only — the same rule about an *import edge* does not exist |
| "This concern lives in one place" | `#Context(deadline:)`, `#Context(allocator:)` | shipped | two compiler-known keys; users cannot add one |
| "Say this table once" | `@loop` over `T.@fields` in derive templates | ratified (D-META-*) | cannot reach marker bodies, impl items, bench/test blocks |

Same shape as the number tower and the fact law before it: many coats, one law underneath. A name's stage, a name's liveness, an edge's legality, a policy's home — these are facts about the program. The fact law already says what facts do: *a fact moves toward safety silently; every move away is one written word, at the site, on the record; at runtime, no fact remains* (philosophy.md:68, D-FACT-LAW1=B).

### The DRY escapes, measured

The stdlib itself cannot obey the corpus law inside Jet source. These are today's numbers, not hypotheticals:

| Forced duplication | Where | Count |
|---|---|---|
| SI-prefix block copy-pasted per unit family | crates/jet-codegen/src/Prelude/Units.jet:6-910 | 24 rows × 28 families ≈ 700 of 911 lines |
| Byte-identical error-conversion impls | crates/jet-codegen/src/Prelude/Errors.jet:41-91 | 13 |
| Hand-unrolled bench blocks for a 4×3 matrix | examples/features/tooling/para_map_crossover_bench.jet:47-141 | 24 |
| `#Target(JS)` repeated per function because the module grouping is bug-blocked | examples/features/web/ui_showcase.jet:16-29 | 10 |
| Body-cap magic number restated at call sites | 7 net examples | 11 sites, 2 spellings |

## The proposal

One sentence: **the program is a value; its structure — name liveness, API lifecycle, dependency edges, callable policy — is one fact plane under the existing fact law.** Every element below is an instance of a ratified law, not a mechanism. Each climbs the rungs: the beginner types nothing and loses nothing; the expert gets the ledger, the explicit spelling, and the refusal switch. No upper rung changes what the lowest rung does.

### Element 1 — Liveness: an unused name is a fact *(proposed)*

Today the compiler knows a name is dead and says nothing. Probed this run — this program passes `jet check` with `ok`:

```jet
// today: jet check says "ok: no problems"
module bank
use core.files as files      // never used

fn unused_helper() => Int {  // never called
    return 7
}

fn run() {
    a :: bank.open()
    dead :: 5                // never read
    print(a.owner)
}
```

Proposed: each dead name gets one registered verdict — unused import, unused binding, unreachable private function, and (package tier) a `pub` item no consumer reaches. All are warnings; the override law already guarantees warnings never fail a build by default (D-LINTPOLICY1). The suppression spelling already exists and is already ratified: the underscore ladder (D-NAME-SIGIL1) makes `_name` the human-internal/discard mark. Renaming is the whole gate:

```jet
// proposed verdicts, spelled with today's ratified escape
use core.files as files     // warn: import `files` is never used — remove it, or name it `_files`
dead :: 5                   // warn: `dead` is never read — remove it, or name it `_dead`
fn _unused_helper() ...     // silent: `_` already means "internal / intentionally unreferenced"
```

The rungs. Beginner: types nothing; scratch scripts still run; a warning teaches, never blocks. Intermediate: renames to `_x` or deletes — one obvious repair. Expert: `policy: .{ lints: .{ deny: [unused_import] } }` makes it a team wall (names not codes, c0s9oygd), and the same policy key allows a team to switch any of the family off. The three exits every magic owes: **see it** — `jet inspect structure <file>` lists every liveness fact and every `_` gate *(proposed lens)*; **spell it** — `_name` at the site; **refuse it** — the policy key, project-wide.

The agent case is the strongest: dead code is context poison. An agent reading a module pays tokens for every line; unused names are pure waste the compiler already knows about (context economy), and "remove it or mark it `_`" is a one-repair verdict (repair determinism). This closes the one hygiene loss to clippy/eslint/vet with zero new syntax.

### Element 2 — Lifecycle: one ladder from internal to retired *(proposed; opens a closed compiler table)*

Today a name's lifecycle stage is expressed by five fragments (table above). The ladder already has its bottom rungs ratified — this element names the whole ladder and opens the top to users:

```text
_name            internal        hidden from discovery            D-NAME-SIGIL1 (ratified)
pub _name        soft-public     callable, unsuppressible L0601   D-SHAPE-INTERNAL1 (ratified)
pub              stable          shape frozen by ApiFreeze        E1218/E2601 (shipped)
#Deprecated(...) retiring        warn with replacement            proposed — today Core-only, hardcoded
(removed)        retired         named-delta error                E2601 names the delta (shipped)
```

The one new spelling is the marker every library author needs and no Jet user has:

```jet
// proposed: any package, any pub item — same registry Core uses
#Deprecated(since: "1.2", use: "parse")
pub fn decode(bytes: [Byte]) => Config ? DecodeError { ... }
```

A consumer calling `decode` gets one warning carrying the replacement — a typed edit, not archaeology. `jet fix` can apply it when the replacement is a plain rename. The mechanism is not new: the marker registry is one Jet file the compiler reads (Prelude/Markers.jet, "law zero"), the warning window machinery is spec'd (L2001/E2002, spec.md:3413), and the enforcement half already ships as ApiFreeze. What gets **deleted**: the hardcoded `DEPRECATIONS` table and the `#[allow(dead_code)]` staging in Edition.rs — Core's two rows become ordinary `#Deprecated` markers on `cbor.encode`/`cbor.decode`, eating the compiler's own dogfood.

Rungs: beginner ignores the ladder entirely and reads clean warnings when a library retires something. Library author marks one line. Expert wires editions: `#Deprecated(since:, use:, removed_in:)` plus the package edition gives the same warn-then-error window Core items get. Exits: **see it** — `jet inspect api <package>` shows every item's stage and every consumer of a retiring item *(proposed lens over existing ApiFreeze data)*; **spell it** — the marker is already the explicit spelling; **refuse it** — `policy: .{ lints: .{ deny: [deprecated_use] } }` turns retiring-API use into a team wall, or `allow` silences it for a vendored dependency.

### Element 3 — Edges: architecture rules are manifest policy *(proposed; the ArchUnit gap, claimed in-language)*

Jet already lets a package constrain what a dependency may *do*: `effects: { allow: […], deny: […] }`, with E1220 naming the offending dependency (D-EFFBUDGET1). No language — Jet included — lets a package constrain which of its own modules may *import* which. That rule is the single most-wanted architecture check in industry; a whole tool family (ArchUnit, import-linter, eslint-boundaries, deptrac) exists to bolt it on. The proposal is one manifest key with exactly the effect-budget shape:

```jet
// package.jet — proposed
boundaries: .{
    deny: [
        .{ from: app.ui,   to: app.db },     // UI never touches storage directly
        .{ from: app.core, to: app.* },      // the core layer imports nothing above it
    ]
}
```

A violating `use` is one error naming the edge and the rule, the same way E1220 names the dependency. The check is trivial by construction: Jet's import graph is already explicit (no wildcards E0612, no invisible auto-imports D-NAME-FILES1=C, cycles already rejected E0604) — the compiler holds the whole graph; this only lets the manifest state which edges are legal. Policy narrows and never widens, per the ratified policy law (D-PACKAGE-POLICY-SCOPE1).

Rungs: beginner has no manifest and no rules — nothing changes. A growing team writes two lines and the compiler holds the layering forever. Expert: rules per target or per role module when the ecosystem shape lands (D-ECO-*). Exits: **see it** — `jet inspect boundaries` prints the live graph and which rules each edge passed *(proposed)*; **spell it** — the key is the explicit spelling; **refuse it** — delete the key; absent key means no rules, exactly today.

Agent case: verdict fidelity for architecture. Today an agent can silently erode layering and no oracle objects; with edges as facts the loop catches it at edit time, and the repair is deterministic (route through the allowed layer or amend the manifest — one written word, on the record).

### Element 4 — Callable policy: a home for cross-cutting concerns *(proposed ballot; the field-audit decision, finally asked)*

This is the one place Jet forces programs to violate separation of concerns. Tracing, retry, metrics, auth checks: today each must be written *inside* every function or hand-wrapped per exact signature — there is no signature-preserving transform, function values cannot carry `&`/`^` parameters, and `#Context` keys are compiler-known only. The field audit demanded an owner decision comparing the shapes (field-audit-2026-08-03.md:179-184); no ballot was ever minted. This proposal mints it. The genuine alternatives:

**Option A — typed transform.** A function that takes a callable and returns a callable with the same checked signature (modes, effects, errors, views preserved). Most general; needs the signature-preserving machinery the field audit names as missing today.

```jet
// A, sketch (proposed): wrap preserves the full checked signature
traced_fetch :: trace.wrap(fetch)          // traced_fetch has fetch's exact contract
```

**Option B — policy values at registration points.** No transform in the language; frameworks accept explicit middleware values where callables register. Weakest, but zero new type machinery:

```jet
// B, sketch (proposed)
mux.use(trace.middleware())
mux.get("/api", handle_api)
```

**Option C — first-party markers.** A closed, compiler-known policy set applied by marker, like every other marker: `#Traced`, `#Retry(times: 3)`. No user-authored behavior injection; the policy body lives in Core. Keeps the no-macros wall maximally intact; least extensible.

```jet
// C, sketch (proposed)
#Traced #Retry(times: 3)
fn fetch(url: String) => Response ? NetError { ... }
```

**Option D — decline**; keep hand-threading, accept the scattering. Honest cost: the corpus shows what declining looks like — ten `#Target(JS)` repeats, twelve hand-placed panics per file, and Python keeps the API-composition vector conceded in the field audit.

User-extensible `#Context` keys ride whichever option wins (a typed, effect-tracked key declared like any other fact row — request id, principal, tracer), and the un-typed rights value already has its card (c0zjmtah). Every option keeps the frozen walls: no macros, no AST mutation, comptime never creates types (S26) — options A and B are ordinary values; option C is ordinary markers.

### Element 5 — Say it once, in programs: templates reach registries *(proposed; deletes ~700 lines of stdlib copy-paste)*

The corpus law makes an underivable second copy a build failure — for the compiler's corpus (D-ONCE-LAW1). Jet programs cannot obey the same law because the ratified template machinery (`derive T.Trait { … }`, `@loop` over `T.@fields`, D-META-*) stops at type members: it cannot contribute marker-body entries, top-level impl items, or test/bench blocks. The three measured escapes, closed by letting the *existing* `@loop` reach those three positions — no new evaluator, no macro, no string splicing, same typed item-template law:

```jet
// today: Units.jet writes this 24-row block 28 times (~700 lines)
pub #UnitFamily(Resistance, dimension: M*L*L/T/T/T/A/A, base: ohm) {
    ohm
    quectoohm(scale: 1/1000000000000000000000000000000)
    // ... 22 more identical-shape rows
}

// proposed: the prefix table said once, looped inside the marker body
pub #UnitFamily(Resistance, dimension: M*L*L/T/T/T/A/A, base: ohm) {
    ohm
    @loop p, si.prefixes { unit("{p.name}ohm", scale: p.scale) }
}
```

```jet
// today: Errors.jet hand-writes 13 byte-identical conversions
impl BrowserError => Err { return Err("{self}") }
impl DBError => Err { return Err("{self}") }
// ... 11 more

// proposed: one template over a closed, written list — still nominal, still checked
@loop E, [BrowserError, DBError, NetError, /* … */] {
    impl E => Err { return Err("{self}") }
}
```

```jet
// today: 24 hand-unrolled #Bench blocks for a 4×3 matrix
// proposed: the matrix said once
@loop n, [64, 512, 4096, 32768] { @loop c, [1, 4, 16] {
    #Bench("para_map n{n} c{c}") { require_eq(para_map_case(n, c), expected(n, c)) }
} }
```

S26 stays whole: no type is created, parameterized, or selected — the loop contributes *declarations* the checker sees one by one, exactly as `derive` templates already contribute checked members. The lists are closed and written at the site; nothing is discovered or reflected into existence. Rungs: beginner never sees a template; the stdlib just gets shorter and its tables become auditable. Expert writes loops over written tables. Exits: `jet inspect expand` already exists for every "the compiler wrote this for you" mechanism (spec.md:3710) and covers these expansions too.

### The drift fixes (cards, not ballots)

Found by running the binary this session; each is a bug against ratified law, filed regardless of any ballot outcome:

| Bug | Probe | Contradicts |
|---|---|---|
| Trait value never forms at argument position: `show(c)` where `fn show(s: Shape)` and `c :: Circle.{…}` → E0112 | target-probes/sr/trait3.jet | spec.md:814 "a trait name in type position (`[Shape]`, `fn f(s: Shape)`) means dynamic dispatch with invisible boxing"; S48. Only the `[Shape]` list path works — DIP through trait params is unusable at call sites |
| Struct declared in an inline module is invisible to its own module (E0119 on `Account` inside `module bank { … }`) | target-probes/sr/vis3.jet | D-MOD1-4; ui_showcase.jet:16 ships a 10× `#Target(JS)` workaround and names this bug plus an I2 codegen rejection |
| E0113 self-contradiction: "promises to return `bank.Account`, but this returns `vis2.bank.Account` … Fix: use `bank.Account` here" | target-probes/sr/vis2.jet | I4 — the fix text prescribes exactly what the user wrote |
| Watcher events are stringly (`ev.domain == "process"`) from a stdlib API while `enum` and `distinct` exist | examples/features/io/watcher.jet:31 | illegal-states-unrepresentable, stdlib-api-laws |

## The final vision

The same growing program, today and after. A small service that started as a script and grew a storage layer, a retiring API, and tracing.

**Today.** Structure is folklore: the dead import stays forever, the old `decode` has a `// TODO stop using` comment, layering lives in a wiki, and tracing is pasted into every handler.

```jet
// app/main.jet — today
module store
module api
use core.files as files          // dead since the refactor; nothing will ever say so

fn run() ? NetError {
    mux :: http_server.mux()
    mux.get("/cfg", (req: HTTPRequest) => api.config(req))    // api calls store.decode — the old one
    http_server.serve(mux)?
}
```

```jet
// app/api.jet — today: tracing pasted per handler, old API used silently
pub fn config(req: HTTPRequest) => HTTPResponse {
    t0 :: clock.now()                                  // paste 1 of N
    cfg :: store.decode(files.read("cfg.cbor") ?? []) ?? default_config()
    log.info("config took {clock.now() - t0}")         // paste 2 of N
    return http_server.response(200, cfg.render())
}
```

**After** (every changed line is proposed; unchanged lines mean exactly what they mean today):

```jet
// app/package.jet — two lines of architecture, held by the compiler forever
boundaries: .{
    deny: [ .{ from: app.api, to: core.files } ]   // handlers never touch disk directly
}
policy: .{ lints: .{ deny: [unused_import] } }     // team wall: dead imports fail CI
```

```jet
// app/store.jet — the library retires an API the way Core does
#Deprecated(since: "1.2", use: "parse")
pub fn decode(bytes: [Byte]) => Config ? DecodeError { ... }

pub fn parse(bytes: [Byte]) => Config ? DecodeError { ... }
```

```jet
// app/api.jet — the concern has one home (option C shown; A/B differ only here)
#Traced
pub fn config(req: HTTPRequest) => HTTPResponse {
    cfg :: store.parse(files.read("cfg.cbor") ?? []) ?? default_config()
    //          └ the compiler said: `decode` is retiring since 1.2 — use `parse` (one typed edit)
    //          └ and: `use core.files` from app.api breaks boundary app.api → core.files (route through store)
    return http_server.response(200, cfg.render())
}
```

And the expert's ledger, one command:

```text
$ jet inspect structure app/            # proposed lens
liveness   app/main.jet:3   use core.files      unused — removed by fix, or mark `_files`
lifecycle  store.decode     retiring since 1.2  replacement: store.parse   consumers: 1
boundary   app.api → core.files   DENIED by package.jet boundaries[0]
policy     unused_import    wall (package.jet)  gate uses: 0
expansion  Units.jet        28 families from 1 prefix table (712 lines saved)
```

The end state as a shape — what the compiler holds about every program, all one plane, all erased at runtime:

```text
                    the fact law (D-FACT-LAW1, ratified)
                                   │
        ┌──────────┬───────────┬───┴────────┬─────────────┬──────────────┐
     values      rights      effects      time         STRUCTURE (proposed)
   ranges,      authority   rows, caps,  typestate,    ├─ liveness   (used / _gated)
   refinements  lattice     budgets      protocols     ├─ lifecycle  (_ → pub _ → pub → #Deprecated → gone)
   (ratified)   (ratified)  (shipped)    (shipped)     ├─ edges      (boundaries: allow/deny)
                                                       └─ policy     (one home per concern)
   one home per fact · silent tightening · one written word to loosen · one ledger · nothing at runtime
```

## What this unlocks

- **Agents**: the loop's oracle finally covers structure. Dead code cannot accumulate (context economy), architecture cannot erode silently (verdict fidelity), a deprecation is a typed edit (`use: "parse"` — repair determinism), and a policy change is one site, not N (blast radius). These four quantities are exactly where long-running agent codebases rot today.
- **Libraries**: real evolution. Publish, soften (`pub _name`), retire (`#Deprecated`), remove — every stage checked, every consumer warned with the replacement in hand. Registry upload (D-PKGS1) lands on an ecosystem that already knows how to age.
- **Teams at scale**: the two rules every codebase writes in a wiki — "don't import across layers", "no dead code" — become two manifest lines. Critical-simulation extreme: boundaries + effect budgets + contracts give a supplier-auditable structure story. Trivial-one-liner extreme: a script has no manifest, sees at most a gentle unused warning, and loses nothing.
- **The stdlib itself**: Units.jet drops ~700 lines, Errors.jet drops 12 of 13 impls, the diagnostics of the future stop copy-pasting tombstone filler — the corpus law finally applies to the corpus's own Jet source.

## What stays

- **No Demeter lint.** The corpus is clean (~3 genuine reach-throughs in 613 files) and dot-chains are how builders and guards are supposed to read. A wall kept on purpose.
- **No SRP/responsibility-counting lint.** Ousterhout's classitis is the documented failure mode: SRP over-applied yields shallow modules. Jet's answer is depth (typed CLI structs, deep Core APIs) plus the separate complexity-budget card (c0sbsdkf). Not part of this slate.
- **Strict CQS stays unenforced.** Mutation *marking* (`&`, `^`, mirrored at call sites) is the honest, shipped core; banning value-returning commands would fight half the stdlib for no safety.
- **The visibility ladder, the borrow prover, the trait system, the effect roots, the no-macro/no-HKT/no-top-type walls** — untouched. Element 5 runs entirely inside the ratified template law; elements 1-3 are diagnostics and manifest policy riding D-LINTPOLICY1.
- **Zero-cost stays zero-cost**: every fact in this proposal erases; nothing here exists at runtime (I3).

## Decisions for the owner

| # | Ballot | Question | Options |
|---|---|---|---|
| 1 | D-STRUCT-PLANE1 | Adopt structure as a fact plane: liveness, lifecycle, and edge facts register in the one fact registry, gates land in the one ledger, `jet inspect structure` reads them | adopt / lints-only (no registry) / decline |
| 2 | D-STRUCT-LIVE1 | Liveness verdicts: unused import, binding, private fn, unreachable pub — warnings under the override law, `_name` as the one suppression | full family / imports+bindings only / decline |
| 3 | D-STRUCT-LIFE1 | One lifecycle ladder; user-facing `#Deprecated(since:, use:)`; delete the Core-only DEPRECATIONS table into it | ladder + marker / marker only / decline |
| 4 | D-STRUCT-EDGE1 | `boundaries:` manifest key, effect-budget shape, deny/allow import edges, error names the edge | adopt / adopt + unify spelling with effect budgets / decline |
| 5 | D-STRUCT-POLICY1 | The callable-policy home (the field-audit decision) | A typed transform / B middleware values / C first-party markers / decline |
| 6 | D-STRUCT-ONCE1 | Templates reach registries: `@loop` in marker bodies, top-level impl items, test/bench blocks | adopt / stdlib-internal only / decline |

Each ballot stands alone; any subset is coherent. Ballot 3 amends the E2001/E2002/L2001 area from Core-only to user-facing (names its amendment in the ballot text). Ballot 6 operates inside S26 and D-META-*; it amends neither.

## Implementation shape

- **A — internal re-founding, no surface change.** Wire the liveness computation into sema (the name ledger already exists; D-NAME-TREE1's resolver work is the natural host), land the lifecycle stage as a fact row over existing ApiFreeze data, and delete Edition.rs staging into the marker registry. All tests green, no user-visible change until verdicts switch on.
- **B — land ratified-but-unbuilt work on the new substrate.** D-NAME-FENCE1 (one visibility story), D-ONCE-RETIRE1 (retirement ratchets), and the authority value (c0zjmtah) all touch the same name/stage/rights rows — build them once, on the plane, not three times beside it.
- **C — balloted surface unifications, each a coherent greenfield migration.** Verdicts on (element 1), `#Deprecated` shipped and Core's two rows migrated (element 2), `boundaries:` key (element 3), the chosen policy shape (element 4), template reach (element 5) — each deletes its replaced form in the same change, per the greenfield law.

---

*Strongest unverified assumption:* unreachable-`pub` liveness (element 1's package tier) assumes the ApiFreeze consumer data plus the explicit import graph suffice to decide reachability package-wide without a new whole-program pass; this run verified the graph surfaces (E0604, E0612 probes) and read the ApiFreeze spec (spec.md:4062) but did not run ApiFreeze itself. If that assumption fails, element 1 still ships file-local (imports, bindings, private fns) unchanged.
