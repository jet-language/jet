# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file:** a full decision card carries a worked,
user-story example for each option (what a real person types, sees, and hits as an
error) — not abstract option tables. Decisions not yet drafted to that bar are
listed below as one-liners with a recommendation; expand one into a full card
(with examples) when it's time to decide it.

---

## Open decisions

Every open decision, listed and available now — nothing parked or hidden. Decide
any directly when it has a full card; for the one-liners, ask the dashboard to
**expand into a full card** (options + worked examples) when you want to decide it.
Submitting a decision records it in `syntax-decisions.md` and removes it from here.

### Constructors & values

- **D-CTOR2** — Constructor marker — *open (confirm)* — none vs `new`/`init`/`@constructor`. With D-CTOR1=A (named constructors), a no-`self` static returning the type already *is* a constructor. (rec: no marker.)

### Allocators & memory

- **D-ALLOC-C** — Which allocators ship + wider-API namespace — *open* — `Arena` is in; bundle `Bump`/`Pool`/`Fixed` now or stage them, and is the expert API flat in `core.mem` or grouped under `core.mem.alloc`? (rec: `Arena` now, others staged; flat.)
- **D-ALLOC-D** — Reset/free verb + use-after-reset wording — *open* — capability vocabulary for cleanup (`reset`/`free`) and the diagnostic when you touch freed memory. (rec: settle alongside the allocator API.)

### Named arguments

- **D-NARG-D2** — Default referencing earlier params — *open* — allow `fn box(w: Int, h: Int = w)`? (rec: no in v1 — defaults are self-contained; teaching error.)
- **D-NARG-D4** — Dedicated label-mismatch diagnostic — *open* — transposed/unknown labels fold into E0104 today; give them their own teaching code? (rec: yes.)

### Language surface

- **S83** — External definitions for structs/modules — *blocked* — define methods/items out-of-body, identical semantics; needs a fresh separator (`::` spent by D-BIND1, `.` by D-MOD1). Pick a separator or withdraw.
- **D-JSON3** — Surface lenient JSON coercions — *open* — D-JSON1 coerces `"8080"`→`8080`; how is what-got-coerced shown (per-decode report? build log?). (rec: pick a surfacing, then card it.)
- **D-TOOL-SPLIT** — Split lsp/fmt/lint from the `jet` binary — *open, needs your call* — separate binaries/plugins vs one bundled tool. (no rec.)

### Bigger directions (previously deferred — available to decide now, not parked)

- **S53** — Concurrency: tasks & channels — *deferred to v2; open to revisit* — planned surface `tasks.spawn(closure) -> Task<T>`, `t.join()`, `tasks.channel<T>()`; no shared mutable state (ownership rejects it). Decide whether to pull any of it forward.
- **S56** — Typed reflection / user-defined derives — *deferred to E3 (S26 Layer 3); open* — user-written derive macros + typed reflection, on top of built-in derives (S55). Decide timing/scope.
- **S60** — Compile-time pure evaluation + data embedding — *post-1.0; open* — `comptime` Layer 2 pure eval + embedding. Design-complete; decide whether to promote.
- **jetos config & platform surface (D-OS*, D-NX*)** — *post-Epoch-3 research track; open to direction* — the declarative-OS config/platform DSL. Big surface; context in `docs/plans/jetpack-jetos/`. Decide whether to start shaping it or hold.
