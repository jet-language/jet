# Ontology report and finding taxonomy

The report is a single file under `docs/audits/`, installed through the project-approved non-serve CLI. Use the shared audit-dispositions contract before publication. Report completion does not implement a proposed spelling or semantic change.

## Required report sections

```markdown
# Isomorphic ontology audit — YYYY-MM-DD

## Thesis
One paragraph: what Jet currently teaches about its own ontology.

## Dual-facet scorecard
| Lens | Grade (aligned/drift/unknown) | Evidence |
| Exploratory density vs Python | … | … |
| Systems expressiveness vs Zig/Rust/Odin/C/C++ | … | … |
| Clarity | … | … |
| Isomorphic consistency | … | … |

## Concept map (Jet → ontology)
Table: Jet surface form → ontology id(s) → X-axes → one-line “what it is” → status.

## Concept families
For each non-trivial family: members; shared ontology; spellings today; isomorphism/clarity score; exploratory and systems impact; smallest “ohhh” move or leave alone.

## Findings
Ranked. Each finding gives kind, evidence, ontology IDs, dual-facet impact, recommendation, and owner gate (`yes/no`, plus ballot title when yes).

## Celebrated isomorphisms
What already works and must stay.

## Ontology gaps / extensions
Ontology concepts with no Jet landing, marked absent, deferred, or deliberately out of scope. List any extension made this run.

## Next actions
Ballot titles or card IDs only. Do not create them in a report-only run.

## Finding dispositions
<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
<!-- /audit-dispositions -->
```

## Finding kinds

| Kind | Use when |
| --- | --- |
| `missed unification` | The same ontology has divergent spellings. |
| `false rhyme` | Similar spelling hides a different ontology. |
| `clarity failure` | The form does not teach what it is. |
| `ceremony without teaching` | Tokens buy neither safety nor clarity. |
| `facet failure` | The form loses the exploratory/Python or systems/safety bar. |
| `keep / celebrate` | An existing isomorphism creates the “ohhh.” |

Cover every inventoried form in the concept map and group rows by ontology family. Do not turn next actions into cards or ballots during the report-only run.

## Calibration patterns

Use these as starting points, not as a substitute for new evidence: named function ≈ named binding of a function value; lambda ≈ the same function, anonymous; method ≈ function plus receiver and dispatch rule. Discover more in the declared closure.
