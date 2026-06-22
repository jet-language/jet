# Owner ballot results

_submitted 2026-06-22 11:11_ — **✅ PROCESSED 2026-06-22** (all 14 ratified into
`syntax-decisions.md`, cards stripped, board reconciled). The first eight were processed in
the prior pass; the six added in this batch:
- **D-DETACH1=A** — `task.detach()` consumes the handle + silences L1101.
- **D-REPRC1=B** (not rec A) — `#layout(c)` joins the unified `#layout(…)` family (with `soa`/`packed`/`align`).
- **D-STDIN1=A** — `io.stdin().lines()` reuses the file `FileLines` streaming type.
- **D-TERM1=A** — scoped `live { }` primitive (renamed from "raw mode"); full TUI is a separate `jet.tui` library.
- **D-LSDIR1=A + path.join** — `fs.list_dir -> [DirEntry]`, plus the `path.join` helper per your comment.
- **D-CSVROW1=A** — comptime `csv.decode<Row>`, folded into the unified serde model (D-SERDE1) per your comment.

D-DBG2 stays as resolved live (A default + expert `--raw-frames`), regardless of the "C" line here.

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
