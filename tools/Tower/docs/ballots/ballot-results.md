# Owner ballot results

_submitted 2026-06-22 13:57_

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
