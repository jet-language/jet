# D-WD2 — Semantic Dossier Umbrella

**Cards:** #230, #87. **Decisions:** D-WD2 over D-DOSSIER1. **Status:** docs
slice; implementation waits on stable semantic-index API.

## Goal

`jet inspect dossier` is the umbrella explain view over facts Jet already owns. It is
not a new checker, not a new syntax surface, and not a parallel graph. It reads
named fact producers and renders one coherent human/JSON report.

Beginner path: one command answers "what is this thing and why did Jet do
that?" Expert path: stable lenses expose exact symbols, types, effects, calls,
generated facts, package provenance, cache keys, trust grants, and impact.

## Existing Anchors

- **D-DOSSIER1** ratified option B: build the type/member dossier after the
  semantic-index API is stable.
- **D-SEMINDEX1** owns symbol/reference/type/call/effect facts.
- **D-IMPACT1** owns blast-radius facts.
- **D-EXPANDCLI1** owns sema fact rendering for `jet inspect expand --facts`.
- Epoch 4 slices add package/trust/lock provenance facts that can later appear
  as dossier sections.

## Canonical Shape

`jet inspect dossier <target>` groups existing facts by lens:

```text
summary
symbols
types
effects
calls
impact
generated
package
trust
provenance
```

Each lens has one owner module. The dossier command may omit unavailable lenses,
but it must name the missing producer; it must never synthesize a second truth.

## Build Order

1. Finish stable semantic-index API and schema tests.
2. Implement D-DOSSIER1 type/member view as first dossier lens.
3. Add `jet inspect dossier <file-or-symbol>` as a renderer over the same lens data.
4. Thread `impact`, `expand`, package provenance, and trust grants as additive
   lenses when their fact producers are stable.
5. Add JSON snapshot tests for lens identity and schema version.

## Non-Goals

- No owner-facing syntax.
- No IDE-only feature without CLI parity.
- No duplicate parser, checker, or package graph.
- No broad "AI explanation" prose that cannot be tied to facts.
