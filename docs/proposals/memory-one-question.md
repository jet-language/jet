# Memory: one question, six surfaces

Owner-commissioned first-principles audit of Jet's memory system, 2026-08-12. Verdict: **evolve**. Evidence base: a quarantined blind design (`docs/audits/memory-blind-design-2026-08-12.md`), an advocate dossier with 57 live probes on both tiers (`docs/audits/memory-advocate-dossier-2026-08-12.md`), and a 130-cell delta scorecard (`docs/audits/memory-delta-scorecard-2026-08-12.md`). All owner decisions below were made at the Phase-C checkpoint; each ballot carries the owner's quoted pick.

## Executive summary

Every memory strategy answers one question: **can this reference outlive the memory it points into?** The compiler's prover answers it statically for most code — that part of Jet is already ahead of every peer language, proven by live probes: compile-time use-after-free for arena values with zero lifetime syntax (E0631/E0632), callsite capability sigils, transitive `no_alloc` facts. A designer who had never seen Jet, working blind from first principles, independently re-derived this exact skeleton. The skeleton stays.

The audit found the gaps at the edges of the prover. This proposal closes them with five ratified decisions plus one owner-designed tooling element:

1. **A read-only view flowing into a non-view slot means a copy** — a language semantic, not an error, exactly where the checker refuses the flow today (string views: E2307; generic views: E2305). Declared `View<T>`/`ViewMut<T>` fields and returns keep D-MEM-VIEWRET1 provenance semantics untouched. A visibility ladder (`jet audit copies`, editor ghosts, `jet fmt --explicit-copies`, `policy: .{ copies: .Explicit }`) gives experts every rung from receipt to refusal. *(Decision A)*
2. **The runtime is scaffolding that removes itself.** Wherever a dynamic witness had to step in — the gc in a `#Policy(gc)` scope, a sentry in an `#Unsafe` gate — `jet audit memory` lists the place and `jet fix memory` applies the static repair where exactly one exists. The gc becomes a teacher, not a destination. *(Owner's design, adopted verbatim)*
3. **`#Unsafe` is watched where you develop, silent where you ship.** The dev tier guards raw operations (quarantine, poison, bounds) and reports faults as located Jet errors; release builds carry zero sentries unless the hardened profile asks for them. *(Decision F)*
4. **The sigils cross the FFI border.** `&` lends for one call, `^` gives ownership away, `#Close(fn)` puts a foreign handle on Jet's `close(^)` protocol — the same three meanings they have everywhere else, as an opt-in expert tier above the by-value floor. *(Decision C)*
5. **`freeze` and `^`-captures complete the task story.** Give it (`^`), freeze it (`freeze`), or lock it (`Shared`) — the price of sharing is visible in the source, and data-race freedom stays a compile-time fact in all three. *(Decision D)*
6. **Memory safety becomes a checkable guarantee with its own dial.** `policy: .{ contain: [...], harden: true }` fences untrusted foreign code and keeps sentries in release; `jet inspect guarantees` prints what is proven, watched, fenced, and trusted — per component, honestly. Decoupled from gc on the owner's ruling: gc buys lifetime convenience, never safety. *(Decision G)*

What the ballots ask: ratify the five surface decisions the owner picked at checkpoint, each with its named amendments to ratified law. What does not change: the sigils, the one-fact-graph, arena checking, memory facts, `#Unsafe` reasons and obligations, the by-value FFI floor, `Shared`, `#Policy(gc)`'s one-line shape, and D-REF-SHORTHAND1/2 (raw `&T` fields and returns stay deleted).

The probed defect set (#1883 tier disagreement, view ICEs in AOT, dead freestanding gates, `Pool.add`, recursive enums — 8 of 31 memory examples failing) is repair work already owed by I9. Per the owner's ruling it stays out of this slate: repair cards, not ballots.

## The problem, briefly

Jet's prover is the best in its class where it fires, and the same five walls stop every real program at the same five places. Each row is a live probe from the advocate dossier, not a claim.

| # | The wall | What happens today | Probe | The cost |
|---|---|---|---|---|
| 1 | Store a view | `cache :: [ha, hb]` → E2307, fix says "write `~ha`" | P04j | zero-copy intent dies at every store; the most common memory error demands a fix the compiler could apply itself |
| 2 | Runtime step-ins have no cross-witness ledger | `jet gc report` (D-OPTGC1, shipped) covers gc per run; nothing unifies gc activity and unsafe faults into one repairable list | — | no one command back to static; "lazy" code stays lazy forever |
| 3 | `#Unsafe` at runtime | audited at compile time (P07d), naked at run time — a bad index is silent corruption | P07 | beginners pasting unsafe code get C's runtime behavior |
| 4 | FFI border | `&`/`^` banned (E0702, "foreign functions take owned copies") | P08 | no zero-copy, no handoff, no foreign handles; kernel/audio/embedding pay a copy per crossing |
| 5 | Task captures | lexical groups may borrow provably non-overlapping reads (D-TASKBORROW1) and `shared` covers shared-mutable (D-CONC-SHARE1); beyond those, a capture is E1101 | P09, P09c | sharing past provable non-overlap or past the group has no lock-free spelling; a handoff has no spelling |
| 6 | The guarantee | `--freestanding` builds heap+file-IO programs clean; no per-component safety statement exists | P10 | "is this binary memory-safe?" has no checkable answer |

One frame explains all six, and it is the audit's "ohhh": **every memory strategy is a witness for the one question, differing only in when the proof happens.** The prover is the compile-time witness. The gc is a runtime witness. A sentry is a dev-time witness. An `#Unsafe` reason is a human witness. Fragmentation happened because each witness grew its own surface with no shared receipts — no ledger of where the cheap witness ended and an expensive one stepped in.

## The proposal

### Element 1 — a read view entering a non-view slot means a copy *(Decision A; amends D-MEM-VIEWRET1's non-view-flow behavior only; D-SHAPE-COPY1's `~` stays the only copy spelling)*

Today the compiler prints the fix and makes you type it. Proposed, the store *means* the copy — nothing is injected anywhere, because there is nothing to inject; it is what the construct means, on every tier.

```jet
// today — E2307, both tiers (probe P04j)
cache :: [ha, hb]        // Error: write `~ha` first to get an owned String

// proposed — the same line, legal: a store of a read-only view IS a copy
cache :: [ha, hb]        // behaves exactly as if you wrote [~ha, ~hb]
```

Declared `View<T>`/`ViewMut<T>` fields and returns are untouched — D-MEM-VIEWRET1 already makes those legal with inferred provenance (probes P16, P04a). This rule governs only the flows the checker refuses today. `ViewMut` into a non-view slot stays an error under its existing codes (E2305/E0212) — copying a write-through window would silently break write-back. Meaning-class facts keep their sigils forever: `&` and `^` never become implicit.

The ladder, rung by rung — every rung opt-in, no rung changes what the rung below does:

```
beginner (types nothing):   cache :: [ha, hb]           it works; a store is a copy

rung 1 — the receipt:       $ jet audit copies
                            cache.jet:6  `ha` stored into `cache` as a copy (5 bytes)
                            cache.jet:6  `hb` stored into `cache` as a copy (4 bytes)

rung 2 — editor ghosts:     cache :: [~ha, ~hb]         phantom ~ via LSP inlay; file untouched

rung 3 — materialize:       $ jet fmt --explicit-copies
                            cache.jet: 2 copies written as `~`   (you invited the ink; now they are yours)

rung 4 — refuse:            #Policy(copies: .Explicit)           in source, any scope (mirror law)
                            policy: .{ copies: .Explicit }       in package.jet — today's errors return, exact fix text
```

The three expert exits for this magic default, in one row: see it (`jet audit copies`), spell it (`~`, unchanged, still the one copy spelling per D-SHAPE-COPY1), refuse it (`#Policy(copies: .Explicit)` / `policy: .{ copies: .Explicit }`, riding D-PACKAGE-POLICY-SCOPE1's mirror and tighten-only laws). The compiler never edits user source uninvited — the toolchain norm (`jet self doctor --fix` "never modifies user source") holds; `jet fmt --explicit-copies` is the invited exception.

Principle this element writes into the spec: **sigils that affect other code's observations (`&`, `^`) are mandatory forever; `~` marks cost, and cost defaults on with a receipt, a manual spelling, and a refusal switch.**

### Element 2 — the runtime is scaffolding that removes itself *(owner's design; new tooling, no new syntax)*

Two runtime witnesses exist after this proposal: the gc inside explicit `#Policy(gc)` scopes (D-OPTGC1, unchanged — its per-run receipt `jet run --gc-trace` + `jet gc report` already ships and stays), and dev-tier sentries inside `#Unsafe` gates (Element 3). Both feed one cross-witness ledger above the existing receipts. One command drains it.

```
$ jet run server.jet                 # dev tier; gc scope active in session.jet
$ jet audit memory                   # where did a runtime witness carry the program, and why?
session.jet:14  gc kept `user_cache` alive across requests (12 collections this run)
                static form exists: hoist owner to `App` struct — run `jet fix memory`
audio.jet:4     sentry caught raw write 8 bytes past `ring` (2 runs)
                static form exists: clamp `head` — run `jet fix memory`
graph.jet:31    gc managed `nodes` — cyclic links, no static form
                options: Pool<Node> with Ids, or keep the gc scope (documented)

$ jet fix memory                     # applies every repair with exactly one static form
fixed session.jet:14   owner hoisted — gc scope no longer needed, removed
fixed audio.jet:4      clamp inserted — obligation `valid_ptr` now met statically
skipped graph.jet:31   two valid shapes — choose one (shown above)
```

Deterministic repairs are applied; ambiguous ones are named with their options, never guessed. One honest limit, stated in the tool's own output: the dev tier sees only the runs you ran — the ledger is execution evidence, not proof, and `jet fix memory` covers what your runs exercised.

### Element 3 — `#Unsafe` watched in dev, silent in release *(Decision F; a named, owner-ratified I9 instrumentation carve-out; extends D-UNSAFE-OBLIG1 with runtime evidence; hardened profile is card #1888, open)*

Today the audit surface is real (`#Unsafe("reason")` mandatory per E3112, obligations tracked by `jet inspect unsafe` — probes P07b/P07d) and the runtime is naked. Proposed: every default `jet run` guards raw operations — freed storage quarantined and poisoned, writes checked against allocation provenance the runtime already tracks — and a violation is a located Jet fault, not corruption. Obligation law is untouched: required obligations keep their compile-time discharge (D-UNSAFE-OBLIG1); a runtime witness never substitutes for a required static one. Sentries watch asserted or `.Skip`-ped operations, and a fault is evidence the assertion was false on that run.

```
$ jet run audio.jet                  # dev tier: sentries on by default (proposed)
Runtime fault [R0801]: raw write outside `ring`'s storage
  --> audio.jet:4, in #Unsafe gate audio.jet:3
 Why: `p` points 8 bytes past the buffer the gate's reason names
 Fix: clamp `head` before the write — obligation `valid_ptr` was not met on this run

$ jet run --release audio.jet        # zero sentries, zero cost — audited obligations stand alone
$ jet build --profile=hardened       # ship WITH sentries when the domain wants them (#1888)
```

Beginner rung: nothing typed, every unsafe mistake becomes a named error while developing. Expert rungs: release is silent by default; the hardened profile (#1888, open) opts sentries into shipped binaries; `#Policy(sentries: .Off)` / `policy: .{ sentries: .Off }` refuse the instrumentation while keeping the dev tier; faults feed the Element-2 ledger so `jet fix memory` can repair the code statically. I9, stated honestly: a dev fault where release would corrupt IS an observable tier difference for undefined operations — this decision is the owner-ratified carve-out that permits exactly that, with sentry logic in the Prelude and engines marshalling. The R08xx fault family is proposed; #1892 registers and snapshots it (I4) before any code ships.

### Element 4 — the sigils cross the border *(Decision C; amends the D-FFI-UNIFY1 by-value-only law; E0702 narrows from "never" to "not without the tier"; the `extern c` block below is proposed grammar — #1893 integrates the tier with the ratified `<lang>.<lib>` mounts and `extern rust`/`#FFI` forms instead of minting a parallel shape)*

Today the border erases Jet's ownership language: E0702 bans `&` and `^` in extern signatures (probe P08), so everything copies. Proposed: the sigils keep their exact Jet meaning across the boundary, as an opt-in tier above the by-value floor. `&` means what D-MEM1 says it means — exclusive access for exactly this call: the foreign side may read and write through it, and must not retain it. No sigil changes meaning at the border.

```jet
// today — the floor, unchanged as the default
extern rust "std" { fn id(s: String) => String = "std::convert::identity" }   // copies in, copies out

// proposed — the expert tier: same sigils, same meanings, checked
extern c "libaudio" {
    fn c_hash(buf: &[U8]) => U64          // & = exclusive access for this call; C must not keep it — no copy
    fn c_submit(job: ^Packet)             // ^ = give away; C owns it, C frees it — `job` is gone
    fn c_open(path: String) => Db #Close(c_close)   // foreign handle joins close(^): c_close runs exactly once
}

fn run() {
    db :: c_open("app.db")
    close(^db)                            // Jet guarantees the free — or refuses to compile
}
```

Who-frees-what is in the signature. The beginner card stays two sentences ("foreign calls copy in and copy out; each side frees its own") because the floor is untouched; the tier is per-signature opt-in. Capability-tier functions are generated bindings, so the ratified D-FFI-UNIFY1 rule stands: bindings are callable directly, and raw symbols outside bindings still require `#Unsafe`. Tier behavior follows the existing FFI law — JIT/dev reports the native boundary, native build executes it; the web tier is out of scope for native FFI under that same law. A lying claim is contained to the declared extern surface — the boundary the E0702 family polices today.

### Element 5 — `freeze` and `^`-captures *(Decision D; extends D-TASKBORROW1 and rewrites E1101's fix menu; `Shared` unchanged)*

One program, one changed line. Today's baseline, stated fully: lexical `task.group` children may already borrow readable places where non-overlap is provable (D-TASKBORROW1), and D-CONC-SHARE1 covers shared-mutable state with `shared expr` plus plain field access (the `Shared.new`/`.read` spelling below is what the shipped binary runs — probe P09c — and D-CONC-SHARE1 replaces that spelling). What has no spelling is sharing past provable non-overlap and past the lexical group without a lock:

```jet
// today (probe P09c shape) — works; lock on EVERY read
fn render(a: Shared<Assets>, id: Int) => String { a.read(v => v.names[id]) }
assets :: Shared.new(Assets.{ names: ["sword", "shield"] })

// proposed — same program; the wrapper, the closures, and the lock are gone
fn render(a: Assets, id: Int) => String { a.names[id] }
assets :: freeze(Assets.{ names: ["sword", "shield"] })    // deeply immutable forever
task.group g {
    r :: task.all { render(assets, 0), render(assets, 1) } // bare capture legal BECAUSE frozen
}
```

And the handoff that has no spelling today (plain capture of a mutable local is E1101):

```jet
job := build_batch()
task.group h {
    done :: task.all { process(^job) }    // ^ in a capture = give; the task owns the batch, zero copy
}
// `job` is gone here — E0121 use-after-move, verbatim, if touched
```

The rule after this element, one sentence, teachable to a beginner and checkable by the compiler: **give it (`^`), freeze it (`freeze`), or share it (`shared`) — and the error names which one your capture needs.** Writing to a frozen value is a compile error naming the freeze site (code assigned and snapshotted by #1891). E1101's fix text gains the two new exits.

### Element 6 — the guarantee dial *(Decision G; named amendment to D-PACKAGE-POLICY-SCOPE1: `contain`/`harden` are the first package-only policy keys — they govern dependencies and the build profile, not lexical scopes, so they deliberately have no `#Policy` mirror; tighten-only holds)*

The owner's ruling decoupled this from gc: safe Jet is proven safe by the compiler with gc off; gc buys lifetime convenience, never safety. The parts of a binary the prover cannot vouch for are exactly two — your `#Unsafe` gates and foreign code — so the guarantee dial addresses exactly those two, in `package.jet`:

```
# package.jet                                (proposed keys, ratified policy shape)
policy: .{
    contain: ["libxml"],    # this dependency's pointers become tracked handles inside a fence;
                            # a wild write there faults with a report — it pays 2–4x, your code pays nothing
}
# harden: true is the total switch: release keeps Element-3 sentries AND contains every
# foreign dependency — with harden on, no component may report TRUSTED.
```

```
$ jet inspect guarantees                     # the guarantee as a checkable fact
component            guarantee                        how
your code            no memory corruption — proven    compile time (prover)
#Unsafe gates (2)    watched                          sentries: dev always, release via harden
libxml (contained)   faults fenced, cannot corrupt    tracked handles, 2–4x inside fence only
libz (not contained) TRUSTED — outside the guarantee  extern boundary audited (E0702 rules)
verdict: contain libz (or set harden: true) to make this binary's no-corruption claim total
```

Single-file programs keep R9's law — no manifest ever required; `contain`/`harden` are package-only and unreachable from a bare script, whose guarantee line is simply "your code: proven; externs: trusted." Freestanding targets state the honest limit: no runtime layer exists there to fence with — prover plus audit is the whole story, and the table says so instead of pretending.

### The frame that makes the six one thing

| Witness | When the proof happens | Jet surface | Cost | Receipt |
|---|---|---|---|---|
| prover | compile time | the default — sigils, views, one-fact-graph | zero | the diagnostic itself |
| copy | compile time (semantic) | view stores (Element 1) | the copy, sized in the ledger | `jet audit copies` |
| gc | run time, scoped | `#Policy(gc)` (D-OPTGC1, unchanged) | tracked scope only | `jet audit memory` |
| sentry | dev/hardened runs | `#Unsafe` gates (Element 3) | dev tier only, or `harden` | `jet audit memory` |
| fence | run time, per dependency | `contain:` (Element 6) | 2–4x inside the fence | `jet inspect guarantees` |
| human | review time | `#Unsafe("reason")` + obligations (unchanged) | zero | `jet inspect unsafe` |

One question, six witnesses, one ledger family, and `jet fix memory` walks programs down the table toward the zero-cost rows. That is the owner's hypothesis — the strategies really are one thing — landed as surfaces instead of a manifesto.

## The final vision

A long-lived server, today versus proposed — every difference marked:

```jet
// ─── TODAY ──────────────────────────────────────────────
fn tokenize(line: String) => [String] {
    parts :: line.before("|")
    return [~parts]                          // forced copy; E2307 without it
}
fn render(a: Shared<Assets>, id: Int) => String {
    a.read(v => v.names[id])                 // lock per read, wrapper in the signature
}
fn run() {
    assets :: Shared.new(load_assets())
    task.group g {
        r :: task.all { render(assets, 0), render(assets, 1) }
        print(r)
    }
}
// FFI: by value only — a 100 MB buffer crosses as a copy
// (Shared.new/.read above is the shipped spelling, probe P09c; D-CONC-SHARE1 replaces it with `shared`)
// #Unsafe at runtime: unwatched
// "is this binary memory-safe?": no answer

// ─── PROPOSED ───────────────────────────────────────────
fn tokenize(line: String) => [String] {
    return [line.before("|")]                // a store IS a copy; receipt in `jet audit copies`
}
fn render(a: Assets, id: Int) => String {
    a.names[id]                              // plain value, no lock exists
}
fn run() {
    assets :: freeze(load_assets())          // frozen: share anywhere, race-free by definition
    task.group g {
        r :: task.all { render(assets, 0), render(assets, 1) }
        print(r)
    }
}
extern c "libaudio" {                        // proposed grammar (#1893 integrates with D-FFI-UNIFY1 mounts)
    fn c_hash(buf: &[U8]) => U64             // exclusive for the call — zero copy, checked
}
// dev runs: sentries watch every #Unsafe gate; `jet fix memory` drains the ledger
// package.jet: policy: .{ contain: ["libxml"], harden: true }
// $ jet inspect guarantees → "this binary cannot corrupt memory", per component
```

The memory surface after this proposal, as one tree — items marked **(new)** are this slate; everything else is shipped or ratified today:

```
memory
├── sigils            &  ^  ~            (unchanged; ~ still the one copy spelling)
├── views             View / ViewMut     (stores of read views become copies — new semantic)
├── spaces            mem.Arena / Pool / Fixed / Bump + close(^)    (unchanged)
├── policy            #Policy(no_alloc | zero_rc | gc)              (unchanged)
├── tasks             Shared / Cell  +  freeze(x)  +  ^-captures    (new verbs)
├── ffi               by-value floor  +  & lend / ^ give / #Close   (new tier)
├── unsafe            #Unsafe("reason") + obligations  +  dev sentries R08xx   (new watcher)
├── package.jet       policy: .{ copies: .Explicit, contain: [...], harden: true }   (new keys)
└── receipts          jet audit copies | memory   ·  jet fix memory  ·  jet inspect guarantees   (new commands)
```

## What this unlocks

| Domain | Before → after |
|---|---|
| Server / editor / cache | "two long-lived values, one points at the other" finally has answers: copy-by-meaning for reads, `#Policy(gc)` for keep-alive, `jet fix memory` to come back down |
| Game frame loop | frozen asset tables shared lock-free; `^job` handoff to workers; arenas unchanged |
| Trading / audio | `&[U8]` lends across FFI with zero copy; `policy: .{ copies: .Explicit }` keeps hot paths honest |
| Kernel / embedded | zero-copy FFI tier; guarantee table states the freestanding limit honestly instead of a dead gate |
| Vendored C estates | `contain:` fences the one dependency you distrust at its own cost, not yours |
| Beginners | the most common memory error stops existing; unsafe mistakes become named dev-time faults |
| Agents | fewer error states, each with one repair; ledgers are machine-readable; `jet fix memory` is a deterministic loop-closer |

## What stays, on merit

The three sigils and callsite mirroring (the blind ideal had nothing as good — probes P01/P02b). The one-fact-graph and the window/view/place/owner error vocabulary. Arena UAF checking (E0631/E0632 — no peer catches this class at compile time at all). Transitive memory facts (`#Policy(no_alloc)`/`zero_rc`, E0921). Reason-mandatory `#Unsafe` + `jet inspect unsafe`. The by-value FFI floor as the beginner rung. `Shared` for genuinely shared-mutable state. `#Policy(gc)`'s one-line shape, exactly as the owner designed it. D-REF-SHORTHAND1/2: raw `&T` fields and returns stay deleted — the audit confirmed the ban; stored capability rides views and copies, never raw references.

## Decisions for the owner

All five were picked by the owner at the Phase-C checkpoint (quotes carried in each ballot); the ballots exist so the record is durable and each choice can be re-opened or amended independently.

| Ballot | Decision | Owner's checkpoint pick | Amends |
|---|---|---|---|
| D-MEM-COPYSEM1 | read-view flow into non-view slots: semantic copy + ladder | "Language semantic + visibility ladder" | D-MEM-VIEWRET1 non-view flows (E2307/E2305); D-SHAPE-COPY1 untouched |
| D-FFI-CAP1 | sigils cross the FFI border as an opt-in tier | "i think your proposal is great" | D-FFI-UNIFY1 by-value-only law, E0702 |
| D-CONC-FREEZE1 | `freeze` + `^`-captures; `Shared`/`shared` unchanged | "i accept your proposals" | extends D-TASKBORROW1, E1101 fix text; composes with D-CONC-SHARE1 |
| D-MEM-SENTRY1 | dev-tier sentries default-on; release silent; hardened opt-in | "im fine with decision f proposal" | named I9 instrumentation carve-out; extends D-UNSAFE-OBLIG1; rides #1888 (open) |
| D-MEM-GUARANTEE1 | guarantee dial: `contain`/`harden` + `jet inspect guarantees`, decoupled from gc | "Own dial, decoupled from gc" | amends D-PACKAGE-POLICY-SCOPE1: first package-only keys (tighten-only preserved) |

Withdrawn at checkpoint: unifying strategy attachment points (owner: the shapes are different on purpose). Ruled without ballot: defect set stays repair cards ("sure, it can be repair cards with ballot slate being pure design").

## Implementation shape

- **Phase A — spine repair, no surface change.** The I9 repair cards (#1883 tier disagreement, view-ICE family, freestanding gates, `Pool.add`, recursive enums) land first; every element above assumes a truthful prover on every tier. All existing tests stay green.
- **Phase B — ratified-but-unbuilt on the new substrate.** Element 1 (copy semantics + `jet audit copies`) and Element 5 (`freeze`, `^`-captures) land as the first surface changes, since both delete errors rather than add machinery. Ledger plumbing (`jet audit memory`) rides the same release.
- **Phase C — balloted expansions, each a coherent greenfield migration.** Element 3 sentries + R08xx fault reports; Element 4 FFI tier with E0702 narrowed; Element 6 policy keys + `jet inspect guarantees` + the `contain` fence. Each deletes its replaced form (old E2307 text, old E1101 fix menu, old E0702 absolutism) in the same change.
