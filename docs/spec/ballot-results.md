# Owner ballot results

_submitted 2026-06-19 12:23_

Decisions captured from Tower. Tell Claude **"go"** to ratify these
into syntax-decisions.md, strip the cards, and implement the plans.

## Decisions

**D-CTOR2** — Constructor marker
Decision: **A**

**D-ALLOC-C** — Which allocators ship + namespace
Decision: **C**

**D-ALLOC-D** — Reset/free verb + use-after-reset wording
Decision: **C**

**D-NARG-D2** — Default referencing earlier params
Decision: **A**
Comment: A gives a feeling of magic. We need to improve errors/tooling to address the issues on the backend. Our philosophy should be we do the hard work on the backend to make the frontend magic. But we also expose the tools for experts to have full control.

**D-NARG-D4** — Dedicated label-mismatch diagnostic
Decision: **A**
