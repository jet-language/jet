# The program is a value: engineering principles as compiler facts

Status: proposal. Audit run 2026-08-19 against the classic principles of software engineering, on the owner's request. Everything marked **proposed** awaits a ballot; everything with a decision ID is ratified law cited as-is. Reviewed fresh-context 2026-08-19; all findings fixed in this revision, including one stale-law blocker the review caught (Element 4).

## Executive summary

The audit asked one question: which principles of software engineering does Jet enforce, which does it merely permit, and which does it force programs to violate? The answer splits clean. Jet has already turned more principles into compiler law than any shipped language: information hiding is private-by-default with a five-grade ladder (S18, D-PUBPKG1, D-SHAPE-INTERNAL1), import cycles are hard errors (E0604), inheritance does not exist and delegation is compiler-written (S62), contracts run on every tier (D-PREPOST1, D-FAIL-TIER1), mutation is visible at every call site (D-MEM1), least privilege has a whole effect-and-authority stack (D-EFF1..5), and — as of 2026-08-13 — cross-cutting policy has a typed wrapper that preserves the complete checked contract (`#Policy`/`apply`, D-CALLPOLICY1=E), which is more than Python's decorators ever offered. No memory-safe compiled peer ships this set.

The gaps are just as clean, and they are all the same gap. Four truths about a program's own structure have no home today: whether a name is **used**, what lifecycle **stage** an API is in, which **edges** the import graph may grow, and how a repeating **registry** says itself once. A fifth truth has a home but a locked door: **who may author a policy setting** — the `#Policy` vocabulary is a Core-closed table (`PolicySetting`), the same closed-table shape as the deprecation machinery. Each of these is a fact about the program — and Jet already ratified the law that facts obey: one home, silent tightening, one written word to loosen, on the record (D-FACT-LAW1). The one idea of this proposal: **the program is a value; its structure is one fact plane under the existing fact law**. No new mechanism. Five missing instances of three ratified laws — the fact law, the override law (D-LINTPOLICY1), and the one compile-time program (D-META-*).

Concrete payoffs: dead code becomes visible the moment it dies (today `jet check` says `ok` over an unused import, an unused function, and an unused binding — probed, this run); every package gets the deprecation machinery Core keeps in a duplicated two-row table (canonical in jet-pkg-model's Manifest, mirrored in Sema/Edition.rs, live only for two cbor items); architecture rules become one manifest key shaped like the effect budgets that already exist (D-EFFBUDGET1) — a surface no mainstream language owns, which is why the ArchUnit/import-linter tool family exists; the `#Policy` vocabulary opens to user-authored settings or stays Core-grown by explicit choice; and the standing DRY escapes close — the stdlib's own Units.jet spends ~700 of 910 lines copy-pasting a 24-row SI-prefix block across 31 families because templates cannot reach a marker body.

The ballots ask six direction-level questions: adopt the plane, adopt liveness verdicts, adopt one lifecycle ladder, adopt edge rules, decide who authors policy settings, and let templates reach registries. Each stands alone; any subset can be adopted. Two ballots name amendments: D-STRUCT-LIFE1 opens the Core-only E2001/E2002/L2001 area to users, and D-STRUCT-ONCE1 extends the ratified template law in two named ways. What does not change: the visibility ladder, the borrow prover, the trait system, the `#Policy` spelling, the effect vocabulary, the no-macro walls, and every beginner default — the lowest rung of every element is "you type nothing and today's programs mean exactly what they meant."

Also found and filed separately (bugs, not ballots): trait-value coercion at argument position rejects what spec.md:814 promises (`fn f(s: Shape)` — probed E0112), a struct declared in an inline module is invisible to its own module (probed E0119; ui_showcase.jet:16 documents shipping with the workaround), and E0113 emits a self-contradictory fix ("promises `bank.Account`, returns `vis2.bank.Account`, fix: use `bank.Account`").

## The problem, briefly

Jet's constitution already contains an engineering-principles engine. What it lacks is coverage: the engine runs for values, rights, effects, time, and callables, but not yet for the program's own structure. First the scorecard, then the five-coats table that proves the gaps are one thing.

### The scorecard: every principle, one row

| Principle | Jet today | Evidence | Verdict |
|---|---|---|---|
| Information hiding (Parnas) | Private by default; `pub`, `pub(package)`, soft-public `pub _name` (L0601), `_name` ladder; explicit `pub use` facades; nothing auto-surfaces | S18, D-MOD3/4, D-PUBPKG1, D-SHAPE-INTERNAL1, D-NAME-SIGIL1; probed E0605/E0609 | **Enforced.** Best visibility ladder in the field |
| Acyclic dependencies | Import cycles and computed-field cycles are hard errors; package depth capped at 1, never recursive | E0604, E0338 (probed E0604); D-JPK-REF1 | **Enforced by construction** |
| Composition over inheritance | Inheritance does not exist; delegation is compiler-written `impl Trait using field` | philosophy.md:229; S62; examples/features/modules/library.jet:57 | **Enforced by construction** |
| Design by contract / fail fast | `#Pre`/`#Post` on every tier, erased under proof; `validate` blocks; must-use results; typestate; transactions | D-PREPOST1, D-FAIL-TIER1, D-VALIDATE1, E0402, D-STATE1, examples/features/contracts/pre_post.jet | **Enforced.** No compiled memory-safe peer ships this |
| Least privilege | Inferred effect rows, `#Caps` ceilings, package effect budgets, dataflow tags, sandbox target | D-EFF1..5, D-EFFBUDGET1, spec.md:3013-3220 | **Enforced** (authority substrate ratified-unbuilt, D-AUTHORITY-*) |
| Command-query separation | Mutation marked in signature and mirrored at call site (`&`, `^`); read is the default | D-MEM1, D-MUTSELF1, E0202/E0205 | **Visible** (marking enforced; strict CQS deliberately not) |
| Illegal states unrepresentable | Payload enums, `distinct Int(0..255)`, typestate, session protocols | D-TYPE2-*, D-STATE1, D-PROTO1 | **Supported**; corpus rarely reaches for it; stdlib itself ships stringly watcher events (watcher.jet:71) |
| Least astonishment / one way | One mechanical path is invariant I8; wildcard imports rejected; no invisible auto-imports | I8, E0612, D-NAME-FILES1=C | **Enforced** |
| Open/closed | Enums + traits cover both directions; public API semver snapshot (ApiFreeze E1218/E2601); extension tiers with a hard lid | S48, spec.md:4062, D-EXT1 | **Supported**; user packages cannot declare deprecation — that machinery is Core-only |
| DRY | Generics, generic modules, derive templates, `@loop` over fields; say-it-once is law for the compiler's own corpus | D-GENMOD*, D-META-*, D-ONCE-LAW1 | **Supported with forced escapes**: templates cannot reach marker bodies, impl items, or test/bench blocks — see the numbers below |
| Separation of concerns (cross-cutting) | `#Policy(retry(3), trace("users.load"))` is the one typed callable wrapper; `apply(retry(3), fn)` replaces the chain and preserves the complete checked contract | D-CALLPOLICY1=E, D-CALLPOLICY2=C (2026-08-13, card #1396); examples/features/callable/callable_policies.jet, shipped through every tier | **Enforced mechanism, closed vocabulary**: `PolicySetting` is compiler-known, so users apply policies but cannot author one; `#Context` keys are compiler-known too |
| Dependency inversion | Trait in type position auto-boxes (S48); generic bounds monomorphize | S48, probed `[Shape]` path works | **Supported — but drifted**: direct argument coercion rejects (E0112), and `&`/`^` functions cannot become values, so a mutating dependency cannot be injected |
| Interface segregation | Traits are nominal, arbitrarily small, signature-only | S28 | **Supported** |
| Package stability | ApiFreeze named-delta errors on the package's own public surface; workspace membership checks; effect budgets per dependency | E1218/E2601, E1322-4 (membership, not cycles), D-EFFBUDGET1 | **Supported** for API shape and effects; **absent** for import direction |
| Dead-code hygiene / YAGNI | Nothing. No unused-import, unused-binding, unused-function, or unreachable-pub diagnostic exists in the entire L-table | diagnostics.md L-table; probed: `jet check` prints `ok` over all three | **Absent** |
| Law of Demeter | No surface; corpus is nearly clean anyway (~3 genuine reach-throughs in 613 files) | corpus audit | **Absent on purpose** — kept absent (see What stays) |
| SRP as a lint | No surface; one planning card for a complexity budget (c0sbsdkf) | Tower c0sbsdkf | **Absent on purpose** — depth beats responsibility-counting (see What stays) |

### Where Jet loses today

Honesty first: on the missing rows, Jet loses to tooling ecosystems it otherwise beats. `cargo clippy`, `eslint`, and even `go vet` all report unused code; Jet reports nothing. Rust has `#[deprecated(since, note)]` for every crate; Jet's deprecation table covers two Core items and no user can add a third. Java teams enforce layer rules with ArchUnit; Jet cannot express a layer rule at all. These are the three losses, and all three sit in this proposal's scope. A fourth loss the 2026-08-03 field audit recorded — Python decorators for cross-cutting policy — was closed on 2026-08-13 by ratified, shipped law: `#Policy`/`apply` preserves the full checked contract, which Python never could. What remains of it is the closed vocabulary, Element 4 below.

### Five coats of one law

Every gap is the same underlying thing — a truth about program structure with no home, or a home with a locked door. The proof is that Jet already does each of these jobs *somewhere*, in a fragment:

| The job | Done today by | Home | Defect |
|---|---|---|---|
| "This name is past its prime" | `L0601` soft-public warning | shipped (D-SHAPE-INTERNAL1) | one rung, no ladder |
| "This name is retiring, use X" | `DEPRECATIONS` table, 2 rows, live for cbor only | canonical: jet-pkg-model Manifest.rs:110; mirror: Sema/Edition.rs:34 ("keep in sync") | closed to users, duplicated inside the compiler |
| "This API's shape is a promise" | ApiFreeze semver snapshot of the package's own surface | shipped (E1218/E2601) | enforcement half without a declaration half; no consumer data |
| "This spelling is retired" | `@retired` rows in Markers.jet; D-ONCE-RETIRE1 ratchets | shipped / ratified-unbuilt | compiler surface only |
| "This value must be used" | must-use + `.drop("reason")` | shipped (E0402, D-MARK-DISCARD1) | values only — the same fact about a *name* does not exist |
| "This dependency may not do X" | effect budgets `effects: { allow: […], deny: […] }` | ratified (D-EFFBUDGET1) | effects only — the same rule about an *import edge* does not exist |
| "Wrap this callable with policy" | `#Policy` chain + `apply` replacement | shipped (D-CALLPOLICY1=E) | vocabulary is a Core-closed table (`PolicySetting`); `#Context` keys compiler-known |
| "Say this table once" | `@loop` over `T.@fields` in derive templates | ratified (D-META-CODE1) | cannot reach marker bodies, impl items, bench/test blocks |

Same shape as the number tower and the fact law before it: many coats, one law underneath. A name's stage, a name's liveness, an edge's legality, a policy author's rights — these are facts about the program. The fact law already says what facts do: *a fact moves toward safety silently; every move away is one written word, at the site, on the record; at runtime, no fact remains* (philosophy.md:68, D-FACT-LAW1=B).

### The DRY escapes, measured

The stdlib itself cannot obey the corpus law inside Jet source. These are today's numbers, re-verified against the files:

| Forced duplication | Where | Count |
|---|---|---|
| 24-row SI-prefix block copy-pasted per unit family (Time exempt by design) | crates/jet-codegen/src/Prelude/Units.jet:6-910 | 24 rows × 31 of 32 families ≈ 700 of 910 lines |
| Byte-identical error-conversion impls | crates/jet-codegen/src/Prelude/Errors.jet:41-91 | 13 |
| Hand-unrolled bench blocks for a 4×3×2 matrix (n ∈ 64/256/1024/4096, cost ∈ 1/32/256, map + para_map) | examples/features/tooling/para_map_crossover_bench.jet:47-141 | 24 |
| `#Target(JS)` repeated per function because the module grouping is bug-blocked | examples/features/web/ui_showcase.jet:16-29 | 10 |
| Body-cap magic number restated at call sites | 7 net examples | 11 sites, 2 spellings |

## The proposal

One sentence: **the program is a value; its structure — name liveness, API lifecycle, dependency edges, policy authorship — is one fact plane under the existing fact law.** Every element below is an instance of a ratified law; the two places an element extends a law, the extension is named as an amendment in its ballot. Each climbs the rungs: the beginner types nothing and loses nothing; the expert gets the ledger, the explicit spelling, and the refusal switch. No upper rung changes what the lowest rung does.

### Element 1 — Liveness: an unused name is a fact *(proposed)*

Today the compiler knows a name is dead and says nothing. Probed this run — this single file passes `jet check` with `ok`:

```jet
// today: jet check says "ok: no problems"
use core.files as files      // never used

fn unused_helper() => Int {  // never called
    return 7
}

fn run() {
    dead :: 5                // never read
    print("hi")
}
```

Proposed: each dead name gets one registered verdict — unused import, unused binding, unreachable private function, and (package tier) an export nothing reaches. All are warnings; the override law already guarantees warnings never fail a build by default (D-LINTPOLICY1). The suppression spelling already exists and is already ratified: the underscore ladder (D-NAME-SIGIL1) makes `_name` the human-internal/discard mark. Renaming is the whole gate:

```jet
// proposed verdicts, spelled with today's ratified escape
use core.files as files     // warn: import `files` is never used — remove it, or name it `_files`
dead :: 5                   // warn: `dead` is never read — remove it, or name it `_dead`
fn _unused_helper() ...     // silent: `_` already means "internal / intentionally unreferenced"
```

The export tier is scoped honestly: it fires only where the full consumer set is closed — `pub(package)` items and application targets. Library `pub` is exempt by definition: its consumers live outside the build. And it needs new analysis: ApiFreeze snapshots only the package's own public surface (verified in Sema/ApiFreeze.rs this run), so unreachable-export requires a package-wide reference pass over the explicit import graph. That pass is the one genuinely new machine in this proposal.

The rungs. Beginner: types nothing; scratch scripts still run; a warning teaches, never blocks. Intermediate: renames to `_x` or deletes — one obvious repair, and `jet fix` can apply it. Expert: `policy: .{ lints: .{ deny: [unused_import] } }` makes it a team wall (names not codes, c0s9oygd), and the same key with `allow` switches any of the family off project-wide. The three exits every magic owes: **see it** — `jet inspect structure <file>` lists every liveness fact and every `_` gate *(proposed lens)*; **spell it** — `_name` at the site; **refuse it** — the policy key.

The agent case is the strongest: dead code is context poison. An agent reading a module pays tokens for every line; unused names are pure waste the compiler already knows about (context economy), and "remove it or mark it `_`" is a one-repair verdict (repair determinism). This closes the one hygiene loss to clippy/eslint/vet with zero new syntax.

### Element 2 — Lifecycle: one ladder from internal to retired *(proposed; opens a closed compiler table)*

Today a name's lifecycle stage is expressed by five fragments (table above). The ladder already has its bottom rungs ratified — this element names the whole ladder and opens the top to users:

```text
_name            internal        hidden from discovery            D-NAME-SIGIL1 (ratified)
pub _name        soft-public     callable, unsuppressible L0601   D-SHAPE-INTERNAL1 (ratified)
pub              stable          shape frozen by ApiFreeze        E1218/E2601 (shipped)
#Deprecated(...) retiring        warn with replacement            proposed — today Core-only, two items, duplicated table
(removed)        retired         named-delta error                E2601 names the delta (shipped)
```

The one new spelling is the marker every library author needs and no Jet user has:

```jet
// proposed: any package, any pub item — same machinery Core uses
#Deprecated(since: "1.2", use: "parse")
pub fn decode(bytes: [Byte]) => Config ? [FieldError] { ... }
```

A consumer calling `decode` gets one warning carrying the replacement — a typed edit, not archaeology. `jet fix` can apply it when the replacement is a plain rename. The mechanism is not new: the marker registry is one Jet file the compiler reads (Prelude/Markers.jet, "law zero"), and the L2001/E2002 warning path is live today — it fires for `cbor.encode`/`cbor.decode` from a hardcoded table that exists **twice** (canonical in jet-pkg-model Manifest.rs:110-141, mirrored in Sema/Edition.rs:34 with a "keep in sync" comment). What gets **deleted**: both copies, in one change, per the greenfield law — Core's two rows become ordinary `#Deprecated` markers, eating the compiler's own dogfood. ApiFreeze stays as the enforcement half: removing the item later is the same named-delta error it is today.

Rungs: beginner ignores the ladder entirely and reads clean warnings when a library retires something. Library author marks one line. Expert wires editions: `#Deprecated(since:, use:, removed_in:)` binds the removal edition once package editions are wired; until then the field is optional and the rung is warn-only. Exits: **see it** — `jet inspect api <package>` renders every item's stage from the markers *(proposed lens; a consumer column is new data, not an existing snapshot)*; **spell it** — the marker is already the explicit spelling; **refuse it** — `policy: .{ lints: .{ deny: [deprecated_use] } }` turns retiring-API use into a team wall, or `allow` silences it for a vendored dependency.

### Element 3 — Edges: architecture rules are manifest policy *(proposed; the ArchUnit gap, claimed in-language)*

Jet already lets a package constrain what a dependency may *do*: `effects: { allow: […], deny: […] }`, with E1220 naming the offending dependency (D-EFFBUDGET1). No language — Jet included — lets a package constrain which of its own modules may *import* which. That rule is the single most-wanted architecture check in industry; a whole tool family exists to bolt it on (ArchUnit, import-linter, eslint-boundaries, deptrac). The proposal is one manifest key in the effect-budget style:

```jet
// package.jet — proposed
boundaries: {
    deny: [
        { from: "app.ui",   to: "app.db" },     // UI never touches storage directly
        { from: "app.core", to: "app.*" },      // the core layer imports nothing above it
    ]
}
```

A violating `use` is one error naming the edge and the rule, the same way E1220 names the dependency. The check is trivial by construction: Jet's import graph is already explicit (no wildcards E0612, no invisible auto-imports D-NAME-FILES1=C, cycles already rejected E0604) — the compiler holds the whole graph; this only lets the manifest state which edges are legal. Policy narrows and never widens, per the ratified policy law (D-PACKAGE-POLICY-SCOPE1). The new manifest surface is named honestly: rule rows are record values, module paths are strings with one trailing-`*` subtree wildcard — three small additions to the manifest vocabulary, spelled out in the ballot's tradeoff.

Rungs: beginner has no manifest and no rules — nothing changes. A growing team writes two lines and the compiler holds the layering forever. Expert: rules per target or per role module when the ecosystem shape lands (D-ECO-*). Exits: **see it** — `jet inspect boundaries` prints the live graph and which rules each edge passed *(proposed)*; **spell it** — the key is the explicit spelling; **refuse it** — delete the key; absent key means no rules, exactly today. A rule matching zero edges is itself a liveness warning under Element 1, so dead rules surface too.

Agent case: verdict fidelity for architecture. Today an agent can silently erode layering and no oracle objects; with edges as facts the loop catches it at edit time, and the repair is deterministic — route through the allowed layer, or amend the manifest with one written line, on the record.

### Element 4 — Policy authorship: who may mint a policy setting *(proposed ballot; the wrapper itself is ratified and shipped)*

The review of this audit caught its own stale premise, and the correction is the finding. Cross-cutting policy is **not** an open gap: D-CALLPOLICY1=E and D-CALLPOLICY2=C (2026-08-13, card #1396) ratified and shipped the one typed callable wrapper, through every tier:

```jet
// shipped today — D-CALLPOLICY1=E, examples/features/callable/callable_policies.jet
#Policy(trace("users.load")) fn load_user(id: Int, label: String = "user") => String {
    return "{label}:{id}"
}

fn run() {
    selected :: apply(retry(3), load_user)   // replaces the chain, keeps the whole checked contract
    bare :: apply(load_user)                 // selects the bare function
    print(selected(7))
}
```

The replacement preserves labels, defaults, access, zones, effects, errors, variadics, and returned-view provenance — the exact list the 2026-08-03 field audit said was impossible. What remains is the door: `PolicySetting` is a compiler-known type ("write it only inside a compiler-owned `#Policy(...)` wrapper", Sema core_types.rs:174), so users **apply** policies but cannot **author** one. A team that needs `audit(...)` or `cache_for(30s)` files a compiler request — the same closed-table shape as the deprecation machinery in Element 2. `#Context` keys (`deadline:`, `allocator:`) are compiler-known the same way. The ballot (D-STRUCT-POLICY1, recast) asks one question: does the policy vocabulary open, and through which door — user-declared settings riding the checked `marker`-body machinery that already ships (D-META-USER1=A), or a Core-grown vocabulary where each new setting is its own ballot, or closed on purpose. The ballot explicitly does **not** reopen the `#Policy`/`apply` spelling; it extends D-CALLPOLICY1's vocabulary and names that as its scope.

Rungs today, unchanged by any outcome: beginner types nothing; intermediate applies `#Policy(retry(3))`; expert replaces chains with `apply` and reads the callable-signature lens (`jet inspect` shows the complete checked contract, D-CALLPOLICY1=E). The open door would add one rung above: declare a setting, checked like any marker body. Adjacent and unblocked by this ballot: `&`/`^` functions as values (the DIP drift in the scorecard) and the un-typed rights value (card c0zjmtah).

### Element 5 — Say it once, in programs: templates reach registries *(proposed; deletes ~700 lines of stdlib copy-paste; names two amendments)*

The corpus law makes an underivable second copy a build failure — for the compiler's corpus (D-ONCE-LAW1). Jet programs cannot obey the same law because the ratified template machinery (`derive T.Trait { … }`, `@loop` over `T.@fields`, D-META-CODE1) stops at type members: it cannot contribute marker-body entries, top-level impl items, or test/bench blocks. The three measured escapes, closed by letting the existing `@loop` reach those three positions. Before, verbatim from the file (this block, with 24 prefix rows, appears in 31 of 32 families):

```jet
// crates/jet-codegen/src/Prelude/Units.jet:604 — today
pub #UnitFamily(Resistance, dimension: Mass * Length * Length / Time / Time / Time / Current / Current, base: ohm) {
    ohm
    quectoohm(scale: 1/1000000000000000000000000000000)
    rontoohm(scale: 1/1000000000000000000000000000)
    yoctoohm(scale: 1/1000000000000000000000000)
    // ... 21 more prefix rows, then the same 24 again in 30 other families
}
```

```jet
// proposed: the same family, the prefix table said once
pub #UnitFamily(Resistance, dimension: Mass * Length * Length / Time / Time / Time / Current / Current, base: ohm) {
    ohm
    @loop p in si.prefixes { unit("{p.name}ohm", scale: p.scale) }
}
```

Family-specific rows (`inch`/`foot`, `celsius`, `hectare`, `psi`, `electronvolt`) stay hand-written — the loop replaces only the mechanical 24, which is why the win is ~700 lines, not the whole file. The same reach fixes the other two escapes as the same program they are today:

```jet
// proposed: Errors.jet's 13 identical conversions, said once over a written list
@loop E in [BrowserError, DBError, NetError, /* the 13 written names */] {
    impl E => Err { return Err("{self}") }
}

// proposed: the real 4×3 bench matrix, both subjects, today's exact expected values
cases :: [.{n: 64, c: 1, expect: 66496}, .{n: 64, c: 32, expect: 31573635}, /* ...10 more rows from today's file */]
@loop case in cases {
    #Test("map n{case.n} c{case.c}") {
        .measure { assert_eq(map_case(case.n, case.c), case.expect) }
    }
    #Test("para_map n{case.n} c{case.c}") {
        .measure { assert_eq(para_map_case(case.n, case.c), case.expect) }
    }
}
```

Two of these forms exceed today's ratified template law, and the ballot names both as amendments rather than pretending otherwise: **(a)** `@loop` over a closed, written *type* list (the impl form) adds a comptime binding kind D-GENMOD-VALUE1's value set does not contain; **(b)** declaration names built from interpolation (`{p.name}ohm`) admit comptime-computed names inside marker bodies. Both stay inside S26's intent — no type is created or selected at a use site; the lists are closed and written where the reader stands; the checker sees each generated declaration one by one, as derive templates already work — but intent is not law, so D-STRUCT-ONCE1 carries the amendment text. Rungs: beginner never sees a template; the stdlib just gets shorter and its tables become auditable. Expert writes loops over written tables. Exits: **see it** — `jet inspect expand` already covers every "the compiler wrote this for you" mechanism (spec.md:3710) and covers these; **spell it** — the unrolled rows stay legal, a loop is never required; **refuse it** — `policy: .{ lints: .{ deny: [template_loops] } }` walls the feature off for a team *(proposed lint name)*.

### The drift fixes (cards, not ballots)

Found by running the binary this session; each is a bug against ratified law, filed regardless of any ballot outcome:

| Bug | Probe | Contradicts | Card |
|---|---|---|---|
| Trait value never forms at argument position: `show(c)` where `fn show(s: Shape)` and `c :: Circle.{…}` → E0112 | target-probes/sr/trait3.jet | spec.md:814 "a trait name in type position (`[Shape]`, `fn f(s: Shape)`) means dynamic dispatch with invisible boxing"; S48. Only the `[Shape]` list path works — DIP through trait params is unusable at call sites | #2053 |
| Struct declared in an inline module is invisible to its own module (E0119 on `Account` inside `module bank { … }`) | target-probes/sr/vis3.jet | D-MOD1-4; ui_showcase.jet:16 ships a 10× `#Target(JS)` workaround and names this bug plus an I2 codegen rejection | #2054 |
| E0113 self-contradiction: "promises to return `bank.Account`, but this returns `vis2.bank.Account` … Fix: use `bank.Account` here" | target-probes/sr/vis2.jet | I4 — the fix text prescribes exactly what the user wrote | #2054 |
| Watcher events are stringly (`ev.domain == "process"`) from a stdlib API while `enum` and `distinct` exist | examples/features/io/watcher.jet:71 | illegal-states-unrepresentable, stdlib-api-laws | #2055 |

## The final vision

The same growing program, today and after. A small service that started as a script and grew a storage layer, a retiring API, and tracing.

**Today.** Structure is folklore: the dead import stays forever, the old `decode` has a `// TODO stop using` comment, layering lives in a wiki. Tracing alone is already solved — `#Policy(trace(...))` ships.

```jet
// app/api.jet — today: dead import silent, old API used silently, layering unenforced
use core.files as files                     // dead since the refactor; nothing will ever say so

#Policy(trace("api.config"))                // shipped today (D-CALLPOLICY1=E)
pub fn config(req: HTTPRequest) => HTTPResponse {
    cfg :: store.decode(files.read("cfg.cbor") ?? []) ?? default_config()
    //          └ decode is the abandoned API; the comment in store.jet is the only warning
    return http_server.response(200, cfg.render())
}
```

**After** (every changed line is proposed; unchanged lines mean exactly what they mean today):

```jet
// app/package.jet — two lines of architecture, held by the compiler forever
boundaries: {
    deny: [ { from: "app.api", to: "core.files" } ]   // handlers never touch disk directly
}
policy: .{ lints: .{ deny: [unused_import] } }        // team wall: dead imports fail CI
```

```jet
// app/store.jet — the library retires an API the way Core retires cbor.encode
#Deprecated(since: "1.2", use: "parse")
pub fn decode(bytes: [Byte]) => Config ? [FieldError] { ... }

pub fn parse(bytes: [Byte]) => Config ? [FieldError] { ... }
```

```jet
// app/api.jet — same function; the compiler now says what the wiki used to
#Policy(trace("api.config"))
pub fn config(req: HTTPRequest) => HTTPResponse {
    cfg :: store.parse(files.read("cfg.cbor") ?? []) ?? default_config()
    //          └ the compiler said: `decode` is retiring since 1.2 — use `parse` (one typed edit)
    //          └ and: `use core.files` from app.api breaks boundaries[0] (route through store)
    return http_server.response(200, cfg.render())
}
```

And the expert's ledger, one command:

```text
$ jet inspect structure app/            # proposed lens
liveness   app/api.jet:2    use core.files      unused — removed by fix, or mark `_files`
lifecycle  store.decode     retiring since 1.2  replacement: store.parse
boundary   app.api → core.files   DENIED by package.jet boundaries[0]
policy     unused_import    wall (package.jet)  gate uses: 0
expansion  Units.jet        31 families from 1 prefix table (~700 lines saved)
```

The end state as a shape — what the compiler holds about every program, all one plane, all erased at runtime:

```text
                    the fact law (D-FACT-LAW1, ratified)
                                   │
     ┌──────────┬───────────┬──────┴─────┬─────────────┬──────────────────┐
   values      rights      effects      time         callables        STRUCTURE (proposed)
  ranges,     authority   rows, caps,  typestate,   #Policy chain,   ├─ liveness   (used / _gated)
  refinements lattice     budgets      protocols    apply()          ├─ lifecycle  (_ → pub _ → pub → #Deprecated → gone)
  (ratified)  (ratified)  (shipped)    (shipped)    (shipped)        ├─ edges      (boundaries: allow/deny)
                                                                     └─ authorship (who mints a policy setting)
   one home per fact · silent tightening · one written word to loosen · one ledger · nothing at runtime
```

## What this unlocks

- **Agents**: the loop's oracle finally covers structure. Dead code cannot accumulate (context economy), architecture cannot erode silently (verdict fidelity), a deprecation is a typed edit (`use: "parse"` — repair determinism), and policy stays one written mark per callable (blast radius). These four quantities are exactly where long-running agent codebases rot today.
- **Libraries**: real evolution. Publish, soften (`pub _name`), retire (`#Deprecated`), remove — every stage checked, every consumer warned with the replacement in hand. Registry upload (D-PKGS1) lands on an ecosystem that already knows how to age.
- **Teams at scale**: the two rules every codebase writes in a wiki — "don't import across layers", "no dead code" — become two manifest lines. Critical-simulation extreme: boundaries + effect budgets + contracts give a supplier-auditable structure story. Trivial-one-liner extreme: a script has no manifest, sees at most a gentle unused warning, and loses nothing.
- **The stdlib itself**: Units.jet drops ~700 lines, Errors.jet drops 12 of 13 impls, and Core's deprecation table becomes two ordinary markers — the corpus law finally applies to the corpus's own Jet source.

## What stays

- **No Demeter lint.** The corpus is clean (~3 genuine reach-throughs in 613 files) and dot-chains are how builders and guards are supposed to read. A wall kept on purpose.
- **No SRP/responsibility-counting lint.** Ousterhout's classitis is the documented failure mode: SRP over-applied yields shallow modules. Jet's answer is depth (typed CLI structs, deep Core APIs) plus the separate complexity-budget card (c0sbsdkf). Not part of this slate.
- **Strict CQS stays unenforced.** Mutation *marking* (`&`, `^`, mirrored at call sites) is the honest, shipped core; banning value-returning commands would fight half the stdlib for no safety.
- **The `#Policy`/`apply` spelling stays exactly as ratified** (D-CALLPOLICY1=E); Element 4 only asks who may grow its vocabulary.
- **The visibility ladder, the borrow prover, the trait system, the effect roots, the no-macro/no-HKT/no-top-type walls** — untouched. Elements 1-3 are diagnostics and manifest policy riding D-LINTPOLICY1; Element 5's two extensions are named amendments, not silent ones.
- **Zero-cost stays zero-cost**: every fact in this proposal erases; nothing here exists at runtime (I3).

## Decisions for the owner

| # | Ballot | Question | Options |
|---|---|---|---|
| 1 | D-STRUCT-PLANE1 | Adopt structure as a fact plane: liveness, lifecycle, and edge facts register in the one fact registry, gates land in the one ledger, `jet inspect structure` reads them | A one plane (registry + ledger + lens) / B lints-only / C external analyzer tool / D decline |
| 2 | D-STRUCT-LIVE1 | Liveness verdicts — a scope dial: how much of the dead-name family ships | A full family incl. unreachable exports / B file-local (imports, bindings, private fns) / C imports only / D decline |
| 3 | D-STRUCT-LIFE1 | One lifecycle ladder; user-facing `#Deprecated`; both copies of the Core table deleted into it | A ladder + marker, delete both tables / B marker only, Core keeps its table / C manifest-declared deprecations / D decline |
| 4 | D-STRUCT-EDGE1 | `boundaries:` manifest key, effect-budget style, deny/allow import edges, error names the edge | A sibling key / B one merged rules surface (amends D-EFFBUDGET1) / C site markers on modules / D decline |
| 5 | D-STRUCT-POLICY1 | Who may author a `#Policy` setting (extends D-CALLPOLICY1's vocabulary; never its spelling) | A user-declared settings via checked marker bodies / B Core-grown, one ballot per setting / C closed on purpose |
| 6 | D-STRUCT-ONCE1 | Templates reach registries: `@loop` in marker bodies, top-level impl items, test/bench blocks — two named amendments to template law | A adopt with both amendments / B stdlib-internal only / C per-DSL knobs instead / D decline |

Each ballot stands alone; any subset is coherent. Ballot 3 amends the E2001/E2002/L2001 area from Core-only to user-facing. Ballot 5 extends ratified D-CALLPOLICY1=E and says so. Ballot 6 names its two extensions of D-META-CODE1/S26-adjacent law inside the ballot text.

## Implementation shape

- **A — internal re-founding, no surface change.** Wire the liveness computation into sema (the name ledger already exists; D-NAME-TREE1's resolver work is the natural host), build the package-wide reference pass behind no flag, and delete the duplicated deprecation table into the marker registry (both copies, one change). All tests green, no user-visible change until verdicts switch on.
- **B — land ratified-but-unbuilt work on the new substrate.** D-NAME-FENCE1 (one visibility story), D-ONCE-RETIRE1 (retirement ratchets), and the authority value (c0zjmtah) all touch the same name/stage/rights rows — build them once, on the plane, not three times beside it.
- **C — balloted surface unifications, each a coherent greenfield migration.** Verdicts on (element 1), `#Deprecated` shipped and Core's two rows migrated (element 2), `boundaries:` key (element 3), the vocabulary outcome (element 4), template reach (element 5) — each deletes its replaced form in the same change, per the greenfield law.

---

*Strongest unverified assumption:* the package-wide reference pass behind unreachable-export (Element 1) is new analysis whose incremental cost this audit did not measure; ApiFreeze was verified to hold only the package's own surface, so no existing data shortens that pass. If the pass proves expensive, Element 1 still ships file-local (imports, bindings, private functions) unchanged.
