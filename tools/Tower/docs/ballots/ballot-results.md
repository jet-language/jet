# Owner ballot results

_submitted 2026-06-19 14:55_

Decisions captured from Tower. Tell Claude **"go"** to ratify these
into syntax-decisions.md, strip the cards, and implement the plans.

## Decisions

**S83** — External-definition connector for `derive` / `impl` / `fn`
Decision: **D**

**D-TOOL-SPLIT** — Split lsp/fmt/lint out of the `jet` binary
Decision: **A**

**D-PATW** — Wildcard token in pattern position
Decision: **D**

**D-PATR** — Ranges inside a payload slot
Decision: **A**

**D-PATO** — Structural or-patterns binding shared names
Decision: **B**

**D-RANGE1** — Range arms in multi-arm `if`
Decision: **A**

**D-RANGE2** — Ownership of arm-head range semantics across c25 and c20
Decision: **A**

**D-ERR-CONV** — Typed error→error conversion across `?`
Decision: **A**
Comment: We decided earlier that for impl, derive, and fn scopes defined outside of a type, that we would use the ~~ operator. Is that the appropriate one here? IF NO, then proceed with ratifying the original A option. If YES, then ratify with the ~~ operator.

**D-DIST1** — Declaration spelling for distinct types
Decision: **C**

**D-DIST2** — Units of measure: in scope now, or deferred
Decision: **B**
Comment: I want units to be part of an extension of the stdlib, not the core lang

**D-WHEN1** — Compile-time conditional spelling
Decision: **A**

**D-WHEN2** — Checking of the unselected arm
Decision: **A**

**D-NARG-DIAG** — diagnostic codes/text for the named-args follow-ups
Decision: **A**

**D-CLI1** — Unknown `--`-flag before the `--` separator
Decision: **A**

**D-L0201** — How to cut implicit-clone (L0201) noise
Decision: **A**

**D-DBG1** — Debugger entry point
Decision: **A**

**D-EVAL1** — Default output shape for `jet eval --pure`
Decision: **A**
