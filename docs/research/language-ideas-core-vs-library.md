# 42 Ideas, Sorted: Library vs. Core

**The test I used:** *Could someone ship this as a package without forking your compiler?*
If it needs **new syntax**, **type-checker enforcement**, **control of the execution model**, or **the power to restrain or observe code the developer didn't write** → it must be **core**. Everything else can be a **library**.

**Why the split matters to you:**
- **Library bucket = your ecosystem surface.** Keep these *out* of the core so the language stays small and learnable, and let the community build and compete here.
- **Core bucket = your moat.** This is where guarantees live that rivals can't bolt on later. Most of the ★ flagship bets land here — which is exactly where defensibility belongs.
- **Biggest core investment:** the "living graph" (ideas 1–4) — one engine, four headline features.
- **Caveat:** "library-able" means it *can* be a library. A few you may deliberately pull into the core for ergonomics or differentiation — flagged inline.

---

# CAN BE LIBRARIES (keep them out of the core)

## Runtime frameworks
**6. Rewind to last-known-good** — checkpoints + auto-rollback to a clean state.
*Library:* a supervision/snapshot framework over serializable state.
```
supervise(svc, checkpoint: 5.min, on_crash: rewind)
```

**8. Memory with a shelf life** — data auto-expires after a set time.
*Library:* a TTL / expiring-cache type. (Compiler-enforced *lifetimes* would be the core upgrade.)
```
cache.put(token, ttl: 30.min)
```

**23. Adapts to its surroundings** — adjusts to battery, network, load, carbon.
*Library:* an adaptive-policy framework reading platform signals.
```
policy { when battery < 20%: lowPower() }
```

**24. Latency budgets flow downhill** — a deadline passes into the calls below it.
*Library:* a deadline-carrying context value (Go's `context` is exactly this). Implicit propagation = core upgrade.
```
ctx = deadline(100.ms); fetch(ctx)
```

**25. Trade accuracy for speed** — rough-but-fast methods within an error margin.
*Library:* approximate algorithms (sampling, sketches) you call explicitly. Auto-swapping = core.
```
approxCount(rows, error: 2%)
```

**28. One knob for quality under load** — degrade detail instead of dropping requests.
*Library:* a load-aware fidelity middleware (sibling of #23).
```
fidelity.scaleWith(load)
```

**42. Logging you can't forget** — traces generated at every external call.
*Library:* an auto-instrumentation agent. Cleanest if the core marks effect points (see #31).
```
autotrace(all external calls)
```

## Data & numeric types
**11. Self-versioning values** — a value carries old shapes so old readers keep working.
*Library:* versioned serialization with read-time adapters.
```
encode(order, asOf: v1)
```

**12. Define data once, get everything** — one schema → form, API, validation, storage.
*Library:* a schema + codegen framework.
```
generate(from: schema/Invoice)   // UI, API, checks, tables
```

**18. Honest numbers** — a number that tracks its real precision/error through math.
*Library:* an uncertainty/interval numeric type via operator overloading. Literal syntax (`5.0±0.1`) is nicer in core.
```
(0.1 + 0.2 : Measure)   // prints 0.3
```

**21. First-class "unknown"** — loading / pending / never-knowable as distinct values.
*Library:* just a sum type, once the language has them.
```
type Unknown = Loading | Pending | Never
```

**34. ★ Every value knows its unit** — dollars≠euros, ms≠seconds, user-id≠order-id.
*Library-able* with a strong type system — **but you may want it in core** for clean literals (`9.usd`) and pervasiveness, like F#.
```
9.usd + 7.eur   // error
```

## Dev tooling
**20. Always-on self-fuzzing** — hammer your own rules with bad inputs, surface counterexamples.
*Tooling:* property-based testing + a fuzzer in the toolchain.
```
check "balance ≥ 0" for withdraw
```

**37. Code = docs = tests in sync** — examples double as tests and documentation.
*Tooling:* examples-as-tests + a doc generator. A hard "can't go stale" guarantee would be core.
```
fn refund(...) examples { … }
```

**38. Ask your codebase questions** — "where can a balance go negative?"
*Tooling:* a code-query engine over the syntax tree.
```
find paths where balance < 0
```

**39. Refactors you can ship and undo** — a refactor as a named, replayable, reversible object.
*Tooling:* a codemod / refactoring engine.
```
apply RenameField{old, new}
```

**40. See the true blast radius** — what a change can actually affect.
*Tooling:* an impact / dependency analyzer.
```
impact(pricing)   // checkout, invoices, 2 reports
```

**41. Merges that understand intent** — combine edits by meaning, not text lines.
*Tooling:* a structural (AST-aware) diff/merge tool.
```
merge --structural
```

---

# MUST BE CORE (they need syntax, the type system, the runtime, or power over other people's code)

## The living graph — one engine, four features
**1. ★ It updates itself** — change an input, only affected parts recompute.
*Core:* this *is* how the language evaluates; only the runtime can track every dependency.

**2. ★ Ask "why?"** — every value remembers its sources.
*Core:* the runtime must instrument all operations (a library can only trace values you wrap by hand).

**3. Time-travel** — every variable keeps its history.
*Core:* pervasive history is a runtime property (libraries can only do opt-in history containers).

**4. Return a hole, don't crash** — a failure becomes a typed "missing" that flows on.
*Core:* the type checker has to track and propagate it through ordinary operators.

> Build #1–4 as **one substrate**. It's the highest-leverage thing in the whole list.

## New syntax / control flow
**5. `undo` as a keyword** — any computation can step backward.
*Core:* new syntax + a runtime that can reverse effects.

**27. Try both, keep the winner** — `maybe { } else { }` with automatic rollback.
*Core:* new control flow + rollback semantics for effects.

## Execution model
**7. Deterministic by default** — record once, replay exactly.
*Core:* you must control every source of randomness, time, and scheduling — only the runtime can.

**26. Parallel without the wiring** — sequential-looking code, auto-parallelized safely.
*Core:* needs a scheduler *plus* the compiler's proof that parallelizing is safe.

## Type-system enforcement
**13. Roles that change over time** — "admin until July, then user," checked.
*Core:* the type checker has to reason about time.

**14. Order-of-events types** — "charge before ship," enforced.
*Core:* compile-time ordering lives in the type system.

**16. Money that can't leak** — value can't be copied or dropped.
*Core:* requires linear types; a library can't forbid copying.

**19. Describe bad states, get a safe type** — forbidden states become unbuildable.
*Core:* the compiler synthesizes the constraint (refinement types).

**22. Budgets as types** — time/memory caps that break the build.
*Core:* static cost analysis in the compiler. (Runtime budget *checks* can be a library.)

**35. Meaning beyond shape** — "untrusted input can't reach the database."
*Core:* taint tracking; a library can tag a value but can't stop the leak.

## Proof / verification
**15. Dial up correctness** *(vertical bet)* — prove the risky parts, check the rest, see the map.
*Core:* a verifier wired into the compiler. (Runtime contracts alone = library.)

**17. Always-responds guarantee** — proven free of deadlock/hang.
*Core:* a verifier; no library can prove this.

## Security — you can't restrain code you didn't write from a library
**30. Rules travel with the data** — policy enforced wherever a value flows.
*Core:* information-flow control; a library can't constrain downstream code.

**31. Cap what code can do** — "network twice, never disk," dependencies included.
*Core:* an effect system; a callee can't be limited from a library.

**32. Lend a power, get it back** — capability revoked on scope exit.
*Core:* scoped/linear capabilities (try/finally is the weak library version).

**33. Compliance as red squiggles** — "EU data leaves EU" won't compile.
*Core:* information-flow types.

**29. Secrets that rot** — capabilities with a shelf life.
*Core* if they're real unforgeable capabilities. (Plain expiring *tokens* are library-able today.)

## Generated systems & solving
**9. ★ Write the conversation, not the services** — one description, the compiler builds each side.
*Core:* new syntax + a compiler pass that splits and type-checks it across roles.

**10. ★ Safe schema changes** — the compiler won't ship a breaking change without a migration.
*Core* (the enforcement). The runtime up/down-conversion can be a library — see #11.

**36. Every function runs backward too** — forward, or solve for the input, or list matches.
*Core* for the pervasive version. An opt-in solver sublanguage can be a library.

---

## The takeaway

| | Count | What it is |
|---|---|---|
| **Library** | 18 | Your ecosystem surface — keep the core lean, let people build here |
| **Core** | 24 | Your moat — guarantees and syntax rivals can't bolt on later |

**Sequencing suggestion:** ship a *small* core (the living graph #1–4 + two or three type-system pillars), let libraries cover the rest, and graduate the winners into core once they prove themselves. The line isn't sacred — a few "library-able" ideas (units #34, deadlines #24, honest numbers #18) are nicer in core, and some core features exist partly to give libraries hooks (effect system #31 → auto-tracing #42).

*Want me to turn the core bucket into a v1 scope (what ships first vs. later), or sketch the living-graph engine in more detail?*
