# Owner ballot results

_submitted 2026-06-25 13:55_

Decisions captured from Tower. Tell Claude **"go"** to ratify these
into syntax-decisions.md, strip the cards, and implement the plans.

## Decisions

**D-DBG2** — Policy for frames with no Jet source line
Decision: **C**

**D-EFF1** — An effect system, expressed as tags on functions
Decision: **B**
Comment: Please crosscheck the new subquestions for B against existing ballots. Then create new non duplicate ballots as needed so we can proceed. We need to nail down syntax, which is correlated with another ballot

**D-QUAL1** — Spelling the qualifier surfaces (Core D + Roles + Unified block) — rec 4 (Hybrid), 1 if D-ATTR2 stays
Decision: **1**

**D-TXN1** — Rollback semantics for `#transact { }`
Decision: **A**

**D-MIGRATE1** — Compile-time enforcement of breaking data-shape changes
Decision: **A**

**D-SOA1** — Cache-friendly data layout (SOA)
Decision: **A**
Comment: I want a better name than SOA. Please propose that & the other open questions from this decision to the owner as new ballots: "Open questions for the owner: 1. Whole-struct SOA only in v1 (recommended), or support #layout(soa: field, …) partial annotation? 2. Should soa [Particle] (Option B) be a future-reserved spelling even if A is chosen, to enable per-container overrides later? 3. Interaction with #Serialize and reflection: does SOA layout affect the serialized representation?"

**D-LOGFMT1** — Human-readable log output for `jet.log`
Decision: **A**

**D-FLOATW1** — Precision-correct math on sized floats
Decision: **A**

**D-DETACH1** — Marking a task as intentionally detached (silence L1101)
Decision: **A**

**D-REPRC1** — C-compatible struct layout annotation
Decision: **B**

**D-STDIN1** — Streaming line-by-line stdin
Decision: **A**

**D-TERM1** — Terminal direct-input control: the name + the low-level surface
Decision: **A**

**D-LSDIR1** — Directory listing: paths, not just names
Decision: **A**
Comment: Include a path.join helper (C) to be shipped also for experts who need finer control

**D-CSVROW1** — Typed CSV row decoding
Decision: **A**
Comment: We are working on a serde equivalent in jet which handles serialization/deserialization for toml, yaml, json, etc. -> So CSV should be included in that plan

**D-UNSAFE2** — Keep `#Unsafe` / `#Audit` separate or merge the audit text into unsafe?
Decision: **B**
Comment: The recommendation is naive. The unsafe description IS the review artifact, it just isnt two separate LOCs.

**D-FIXARR1** — Should fixed-size lists `[T#N]` lower to real stack arrays?
Decision: **B**

**D-JSONOUT1** — Serialize a typed struct to JSON
Decision: **A**
Comment: We will be building our own implementation of rust's serde with json compatibility. I am fine with the serialize tag but need this to be joined at the hip with the serde planning

**D-MATHLIB1** — Linear-algebra library home & scope
Decision: **A**

**D-REACT1** — Should reactive/dataflow be core semantics, tooling, or a library?
Decision: **B**

**D-FANOUT2** — Add namespace/member fan-out sugar beyond S75 call fan-out?
Decision: **B**

**D-STRPARSE1** — String parse APIs and comptime `Result`/`Option` evaluation
Decision: **A**

**D-BIND2** — Spelling of the immutable binding
Decision: **A**

**D-TEST4** — doctest convention
Decision: **A**

**D-NUMOPS1** — Integer overflow behavior + the expert numeric value/op surface
Decision: **A**

**D-SERDE1** — A unified, format-agnostic Serialize/Deserialize model
Decision: **A**

**D-ITER1** — Standard lazy iterator-adapter set
Decision: **A**

**D-MUTSELF1** — self-mutation in `mut self` methods
Decision: ****

**D-CAP8** — what does an unmarked parameter `T` mean?
Decision: **C**

**D-CAP9** — `&` / `*` in expression position, and `*T` vs `Ptr<T>`
Decision: **D**
Comment: Use your full recommendation

**D-CAP10** — are capability-only overloads in scope at all?
Decision: **A**

**D-BENCH1** — how do you write a benchmark?
Decision: **A**

**D-PKGSIGN1** — what proves a published package is authentic?
Decision: **B**
Comment: A as an OPT-IN / non-blocking layer with SHA-256 checksum (B) as the always-on floor — `require_signed` stays an org policy, OFF by default — not a hard gate that refuses unsigned packages. That keeps A's unique win (offline authorship proof) while spending friction only on publishers who opt in

**D-DBG3** — Debugger interactive command surface
Decision: **A**

**D-LINALG1** — Linear-algebra type & method names
Decision: **A**
Comment: Ratify Option A names, with C's Vec<N>/Matrix<M,N> as the underlying generic and A as aliases over it.

**D-SUPPLY1** — Supply-chain command surface
Decision: **A**

**D-TXN3** — Deferred post-commit effects
Decision: **A**
Comment: Can we name the transact scope? Meaning having something like "#Transact(Order) {...

**D-NUMOPS2** — overflow default for sized & unsigned integers *(reconcile)*
Decision: **A**

**D-QUAL3** — How a parameterized value-tag attaches to a type
Decision: **C**
Comment: use PascalCase for the unit family tag -> #UnitFamily

**D-SIMD2** — SIMD lane construction & access surface
Decision: **A**

**D-SERDE2** — serialize/deserialize hand-impl names
Decision: **A**

**D-SERDE3** — `rename_all` casing-style menu
Decision: **C**

**D-NOSTD1** — Embedded / freestanding: how a package opts out of the std baseline
Decision: **A**

**D-JSONVERB1** — value→JSON string verb
Decision: **A**

**D-TXN4** — Name the transaction scope so the name is the handle (`#Transact(order) { order.on_commit(…) }`)
Decision: **A**

**D-IF3** — explicit `if subject == { … }` value/pattern dispatch
Decision: **A**

**D-FMT1** — `jet fmt` preserves single-line bodies
Decision: **A**

**D-SERDE4** — derive-marker shape: one marker or two?
Decision: **B**
Comment: B, However, I want the collapsed version to be Codable, with Encode & Decode as the one way types

**D-SERDE5** — per-field attribute surface
Decision: **A**

**D-SERDE6** — typed decode: type argument + verb coherence
Decision: **C**

**D-SERDE7** — enum wire representation
Decision: **A**
Comment: Ship the chooser now, not post v1. How does this collide with the field tagging from DSERDE5? Wouldn't that cover this? If not, then reserve & implement the tag & untagged as a fallback only

**D-SERDE8** — unknown-field policy
Decision: **A**

**D-PUBLISH1A** — `jet publish` command shape + pre-flight refusals
Decision: **A**

**D-VERSION1** — version immutability / re-publish policy
Decision: **A**

**D-RESOLVE1** — dependency resolver default
Decision: **A**

**D-LOCK1** — is `.jet/lock` committed to version control by default?
Decision: **A**

**D-SERDE9** — generic serde bound propagation
Decision: **A**

**D-SERDE10** — phantom / non-serialized type params
Decision: **A**

**D-SERDE11** — manual bound override
Decision: **A**
Comment: Add a card to tower so that the shipping bound idea doesnt get lost

**D-SERDE12** — lift the E2413 gate
Decision: **A**

**D-DEP-ARCHIVE1** — which crate(s) `jet.archive` wraps
Decision: **A**

**D-DEP-DB1** — which sqlite crate `jet.db` wraps
Decision: **A**

**D-BFS1** — where a wrapped crate's source lives for an offline build
Decision: **A**

**D-LIN1-DROP** — how to deliberately discard a `#SingleUse` value
Decision: **A**

**D-TXN-ROLLBACK** — how a value opts into `#Transact` rollback
Decision: **C**
Comment: Auto snapshot by default -> but allow experts to define rollback trait for customizability AND to explicitly define on rollback. This gives magic out of the box at the potential cost of some performance, but experts will be able to work around performance hit using manual method if desired

**D-TAINT-SAN** — sanitizer-function spelling: bare `sanitizer fn` vs `#Sanitizer fn`
Decision: **B**

**D-DET-CAPAPI** — the method API for the deterministic `Clock` / `Rng` capabilities
Decision: **B**

**D-PARSE-1** — correctness-sensitive formats: keep hand-rolled subsets, or let I6 bend?
Decision: **C**

**D-STATE-REQ** — the "this method requires state S" marker spelling
Decision: **A**

**D-STATE-TRANS** — the transition-fn marker + arrow glyph
Decision: **A**

**D-JIT2** — where the Cranelift dependency physically lives
Decision: **A**
Comment: Let's go with A, but have the JIT included by default with a no jit kind of flag to opt out (but named better than that)

**D-STATE-DECL** — how is the state set declared? `state Reservation { … }` block
Decision: **B**

**D-ROLLBACK-TRAIT** — the `Rollback` trait's method shape
Decision: **A**

**D-ASSOC-NOW** — complete associated types now, or ship D-PARSE-1 first?
Decision: **C**
