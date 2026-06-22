# Borrowing from SpacetimeDB → Jet

> **One line:** SpacetimeDB's *architecture* (database + server + live-sync in one box) doesn't transfer to a general-purpose language. Two of its *house rules* do — **checked determinism** and **atomic blocks** — and both are an unusually clean fit for Jet's "magic by default, control for experts."

> **Syntax note:** every Jet snippet below is *illustrative* and needs owner ratification (docs/02). Names like `pure`, `atomic`, `assume_deterministic` are placeholders to make the shape concrete.

---

## 0. What SpacetimeDB is (60 seconds)

A database that you upload your application logic *into*. The only way to change state is a **reducer** — a function exposed to clients as a remote call. Each reducer:

- runs as **one ACID transaction** — all its changes commit, or none do (auto-rollback on error);
- must be **deterministic** — no filesystem, network, timers, or OS randomness;
- still *can* get "now" and randomness, but only **deterministic versions injected through a context** (seeded RNG, a fixed invocation timestamp).

Mental model: `reducer(currentState, input) -> newState`. Determinism is what makes it safe to apply, roll back, or replay.

**The philosophy validation (free souvenir):** SpacetimeDB recently added **procedures** — like reducers, but allowed to make HTTP calls *if* you manage the transaction yourself. That is exactly Jet's pattern in the wild: **reducer = guardrailed default, procedure = explicit opt-in escape hatch.** Use it as evidence the model works at scale.

---

## 1. Survey — what transfers, what doesn't

| SpacetimeDB idea | What it means | Verdict for Jet |
|---|---|---|
| Reducer = atomic transaction | Change fully happens or fully un-happens | ✅ **Borrow** → `atomic { }` |
| Reducers must be deterministic | No clock/network/random inside | ✅ **Headline borrow** → checked `pure fn` |
| Deterministic time/RNG *injected via context* | Still get "now" + randomness, reproducible kind | ✅ **Borrow** → makes purity usable |
| Reducer vs. procedure split | Guardrailed default vs. opt-in escape | ✅ **Validates** Jet philosophy (not a feature) |
| `#[table]` / `#[reducer]` derive-everything | One annotation → serialization, accessors, bindings | 🟡 Reinforces Jet's general "derive by default" stance |
| Subscriptions / incremental deltas | Clients get only what changed | 🟡 **Library**, not compiler |
| Views (derived read-only data) | SQL-view-like computed tables | 🟡 **Library** (reactive/signals package) |
| In-memory tables + commit-log (WAL) | State in RAM, durability via append-only log | ❌ Out of scope — that's *being a database* |
| Collapse Client→Server→DB into one box | Fewer layers, less glue | ❌ Inspiration only — mirrors Jet's "collapse ceremony" ethos |
| Non-sequential auto-IDs, `ctx.sender` auth | Distributed-DB consequences | ❌ Not relevant to a language |

**Takeaway:** most of SpacetimeDB is database-shaped and out of scope. The gold is at the level of *checked language properties*, not deployment architecture.

---

## 2. Primary borrow — checked determinism (`pure fn`)

### Concept
A function marked `pure` is **guaranteed by the compiler** to give the same output for the same inputs. Inside it, the front end *rejects* the unpredictable: wall-clock time, OS randomness, filesystem, network, and any call to a non-`pure` function.

**Analogy:** a `pure fn` is a vending machine. Same coins + same button → same snack, every time, anywhere. A normal function is a barista who might be out of oat milk, in a mood, or closed. You don't make *everything* a vending machine — but for the parts you want to trust blindly (cache, replay, run on 8 threads), the guarantee is gold.

### The trick that makes it *usable* (straight from SpacetimeDB)
Don't just *ban* time and randomness — **inject deterministic versions**. Inside a `pure fn`, you don't lose "now" or dice; you receive a reproducible capability.

```jet
// Illustrative syntax — owner-ratifiable

// ❌ Rejected: reaches for the real wall clock
pure fn score(player: Player) -> Int {
    let t = clock.now()          // JET-PURE-001: nondeterministic call in pure fn
    player.base + t.seconds
}

// ✅ Accepted: the nondeterministic input is handed in
pure fn score(player: Player, at: Timestamp) -> Int {
    player.base + at.seconds
}

// ✅ Accepted: deterministic randomness via an injected capability
pure fn shuffle(deck: Deck, rng: Rng) -> Deck {
    deck.shuffled_with(rng)      // seeded; same seed → same shuffle
}
```

### What you get for free (the "magic")
| Free benefit | Because… |
|---|---|
| Safe memoization / caching | Same inputs always map to same output |
| Safe parallelism | No hidden shared state or I/O to race on |
| Reproducible tests & snapshots | Replaying inputs reproduces the bug, exactly |
| Time-travel / replay debugging | A log of inputs can rebuild any past result |

### Safe default → expert escape hatch
- **Default:** `pure` is **opt-in**. A beginner never types it; `println("hello")` needs no ceremony. (Forcing purity-by-default — the Haskell route — would tax newcomers and hurt the beginner experience Jet ranks first.)
- **Guardrail (on inside `pure`):** strict effect checking.
- **Escape hatch (explicit, opt-in):** `assume_deterministic { ... }` lets an expert run something the compiler can't prove is reproducible (e.g. a hash-cache hit, a config fixed at startup). Burden of proof shifts to the human — a *semantic* footgun, never a memory one, so it's v1-legal.

```jet
pure fn config_value(key: Str) -> Str {
    assume_deterministic {       // "trust me: frozen after boot"
        Config.global().get(key)
    }
}
```

### Core or library?
**Core — must be in the compiler.** Determinism can only be checked by tracking effects across the whole call graph; a library can't see inside other functions. Enforcement lives in **sema** (an effect-tracking pass); the deterministic `Clock`/`Rng` *capabilities* are a small std-lib API. Codegen stays dumb and rustc stays a silent verifier — invariants intact.

```
Jet source ─► parse ─► sema ──────────────► codegen ─► Rust ─► rustc
                         │                     (dumb)         (silent)
                         └─ effect-tracking pass:
                            walk call graph, mark effects,
                            reject nondeterminism in `pure`
                            ▲ all checking + all diagnostics live here
```

### Personas
| Persona | Experience |
|---|---|
| **Beginner** | Never sees it. Writes plain functions; nothing changes. |
| **Working dev** | Marks hot functions `pure`, gets free caching + trustworthy tests. |
| **Systems expert** | Leans on `pure` to parallelize fearlessly; uses `assume_deterministic` at the few edges the checker can't prove. |

### Tradeoffs
| For | Against / cost |
|---|---|
| Enables caching, parallelism, replay — *real* downstream magic | New effect-tracking pass to build + maintain in sema |
| Great, specific diagnostics (front end owns them) | A std capability story (`Clock`, `Rng`) must ship alongside |
| Distinctive — few mainstream languages check this | `assume_deterministic` can be abused → needs lint/visibility |
| Pure semantic guarantee — zero memory-safety surface | Teaching cost: explaining *why* "now" is banned |

### Comparison to other languages
| Language | Determinism story |
|---|---|
| Haskell / Roc | Purity by *default*, effects in types — powerful but heavy for newcomers |
| Rust | `const fn` is a narrow cousin; no general effect tracking |
| Clojure | Convention (`defn` vs side-effecting) — not checked |
| Python / JS / Go | None — purity is a comment, at best |
| **SpacetimeDB** | Enforced for reducers, with injected deterministic time/RNG — **the model Jet adapts** |
| **Jet (proposed)** | Purity **opt-in**, checked in front end, with injected capabilities + explicit escape — beginner-safe *and* powerful |

---

## 3. Secondary borrow — atomic blocks (STM-lite)

### Concept
A block whose mutations are **all-or-nothing**: if it errors or panics partway, Jet reverts every change as if it never ran. Borrowed directly from "every reducer is one transaction."

```jet
// Illustrative
atomic {
    accounts[from].balance -= amount
    accounts[to].balance   += amount
    ledger.append(Transfer { from, to, amount })
}   // any failure inside → all three revert together
```

### Safe default → escape hatch
- **Default:** beginners get rollback for free — no hand-written undo logic.
- **Escape hatch:** experts opt into looser isolation, or an `unchecked` mutation that skips the transaction.

### The subtle rule worth stealing
SpacetimeDB forbids I/O inside reducers *because you can't roll back a sent email*. Jet should reject **irreversible side effects inside `atomic`** and tell you to run them after commit. Same lesson, enforced.

```jet
atomic {
    order.status = .paid
    send_email(receipt)   // JET-ATOMIC-002: irreversible effect inside atomic block
}
// Fix: queue it to fire on commit, or move it after the block.
```

### Core or library?
| Option | One-liner |
|---|---|
| **Core** (`atomic { }`, auto-rollback) | Zero-boilerplate magic; needs runtime snapshot/restore + irreversible-effect check |
| Library (manual snapshot/restore) | No compiler change, but leaky and easy to misuse |

Lean **core** for the magic — but it's a **heavier lift than determinism** (needs runtime rollback, not just a sema pass), so sequence it *after*.

### Comparison
| Language | Story |
|---|---|
| Clojure / Haskell | First-class STM (`ref`/`STM`) — proven, but niche-facing |
| Most languages | Manual `try`/`catch` + hand-rolled undo, or nothing |
| **Jet (proposed)** | `atomic { }` with auto-rollback + irreversible-effect guard, beginner-default |

---

## 4. Agent handoff (mapped to Jet's process)

### Roadmap (docs/05)
| Item | Proposed slot | Why |
|---|---|---|
| Checked determinism (`pure`) | **Nearer term** | Lives purely in sema + a small std capability API; no runtime change |
| Atomic blocks (STM-lite) | **Later** | Needs runtime rollback machinery; build after determinism lands |

> ⚠️ Slot both against the *actual* docs/05 ordering and current invariants in the repo before committing.

### Decision ballots (docs/02 — Open Decisions rows)

**Ballot A — How does Jet expose checked determinism?**
| Option | One-line tradeoff |
| --- | --- |
| **A. Opt-in `pure` + strict interior + `assume_deterministic` hatch** | Low beginner friction; guarantees only where asked — **recommended** |
| B. Pure-by-default + `io`/`impure` opt-out | Strongest guarantees; heavy ceremony, hurts beginner UX |
| C. No language support; lint/convention only | Zero compiler cost; no real guarantee, can't power caching/parallelism |

**Ballot A.1 — How are deterministic time/RNG supplied inside `pure`?**
| Option | One-line tradeoff |
| --- | --- |
| **A. Injected capability (`Clock`, `Rng` params)** | Explicit, testable, matches SpacetimeDB — **recommended** |
| B. Ordinary parameters only | Simplest; verbose for randomness-heavy code |
| C. Seeded ambient global | Convenient; reintroduces hidden state purity was meant to kill |

**Ballot B — Atomic blocks: where do they live?**
| Option | One-line tradeoff |
| --- | --- |
| **A. Core `atomic { }` w/ auto-rollback + irreversible-effect guard** | Zero-boilerplate magic; needs runtime support — **recommended, but sequence after determinism** |
| B. Library (snapshot/restore) | No compiler change; leaky and easy to misuse |
| C. Defer past MVP+N | Keeps the simplicity ratchet tight; ship determinism first |

### Diagnostics (docs/04 — code · what · why · fix · snapshot)
> Codes are placeholders; align to the existing docs/04 scheme.

| Code | What | Why | Fix |
|---|---|---|---|
| `JET-PURE-001` | Nondeterministic call (`clock.now()`) inside `pure fn score` | `pure` fns must return the same output for the same inputs so Jet can cache, parallelize, and replay them; `now()` changes per call | Pass the time in as a param, request a deterministic `Clock` capability, or wrap in `assume_deterministic { }` |
| `JET-PURE-002` | `pure fn total` calls non-pure `log_to_file` | Purity is transitive — calling an effectful fn breaks reproducibility | Mark the callee `pure` if it qualifies, move the call out, or use `assume_deterministic { }` |
| `JET-ATOMIC-002` | Irreversible effect (`send_email`) inside `atomic` block | If the transaction aborts, Jet reverts memory but can't un-send the email — leaving inconsistent state | Move the effect after the block commits, or queue it to fire on commit |

*Each needs a snapshot test of the rendered error (examples = executable spec).*

### Examples (executable spec — add under examples/)
1. `pure_cache.jet` — a `pure fn` memoized automatically; prove same-input → same-output.
2. `pure_rejected.jet` — the `clock.now()` version; assert it fails with `JET-PURE-001`.
3. `pure_injected_rng.jet` — seeded `Rng` capability; same seed → identical shuffle.
4. `atomic_rollback.jet` — a transfer that aborts mid-way; assert all fields revert.
5. `atomic_irreversible.jet` — `send_email` inside `atomic`; assert `JET-ATOMIC-002`.

### Repo checks before building (don't trust memory)
- Confirm docs/02 has no conflicting effect/purity decision already on the books.
- Confirm the std-lib has no existing `Clock`/`Rng` naming to honor.
- Confirm docs/05 ordering so the roadmap slots above are real, not assumed.
- Confirm the docs/04 diagnostic-code scheme and renumber accordingly.

---

## 5. What to explicitly *not* borrow
- In-memory tables, commit-log persistence, SQL subscriptions, client-direct connections — these make SpacetimeDB *a database*. Jet is a general-purpose language; adopting them would blow the simplicity ratchet for zero general-purpose gain.
- Pure-by-default effect typing — philosophically tempting, but it taxes the beginner experience Jet ranks first. Keep purity opt-in.
