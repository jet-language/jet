# Jet Skills Router

Pick **one** skill. Do not chain audits, research, cleanup, or verify unless the
owner asks. Extra tests run only when they are the job.

## Outputs

| Kind | Where |
| --- | --- |
| Audit / research / cleanup reports | `docs/audits/` or `docs/research/` via `tower docs add` |
| Board work | Tower CLI (`plugins/tower/.tower/`) |
| Ratified language law | `docs/spec/` |
| Skill procedures | `.agents/skills/` |

File id: `<skill>-YYYY-MM-DD`. Never overwrite another day's note.

## Catalog

| Skill | Id | Job |
| --- | --- | --- |
| Jet Router | `jet-router` | Route to exactly one skill below |
| Surface Audit | `surface-audit` | Shape / uniformity / consistency outliers and gaps |
| Isomorphic Ontology Audit | `isomorphic-ontology-audit` | Map syntax to foundational concepts; missed isomorphisms / false rhymes / clarity |
| Persona Audit | `persona-audit` | Persona status, push/pull, practical feel |
| Spec Compliance Audit | `spec-compliance-audit` | Codebase vs ratified syntax/spec |
| Mission Audit | `mission-audit` | Language and experience vs philosophy/mission |
| Pragmatism Audit | `pragmatism-audit` | Finish real jobs across domains; default magic + reject/override |
| Field Audit | `field-audit` | Leave/stay pressure + peer-strength gaps (one report) |
| Surface Research | `surface-research` | Mine other languages for surface ideas |
| Lessons Learned | `lessons-learned` | Peer failures Jet must not repeat |
| Structure Cleanup | `structure-cleanup` | Structure-only cleanup, no behavior change |
| Garbage Collection | `garbage-collection` | Delete dead code and stale docs/plans/outputs |
| Verify | `verify` | Code closeout only — not an audit |

Tower board skills live under `plugins/tower/skills/`. Writing modes (`simple*`)
are opt-in and outside this table.

## Route

| If the request is about… | Use |
| --- | --- |
| Unclear which skill / pulse / health check | **Jet Router** → then that skill |
| Shape, uniformity, consistency, syntax/structure outliers | **Surface Audit** |
| Conceptual unity, “what is this”, isomorphisms, false rhymes, clarity-vs-ceremony | **Isomorphic Ontology Audit** |
| Personas, push/pull, real-user feel | **Persona Audit** |
| Ratified syntax/spec vs source | **Spec Compliance Audit** |
| Philosophy / mission alignment | **Mission Audit** |
| Getting work done / domain friction / missing defaults / reject+override | **Pragmatism Audit** |
| Leave language X / peer strengths Jet lacks | **Field Audit** |
| Mine languages for surface ideas | **Surface Research** |
| Lineage / regrets / do-not-repeat | **Lessons Learned** |
| Restructure files, no behavior change | **Structure Cleanup** |
| Dead code or stale docs/plans/outputs | **Garbage Collection** |
| Code done / bless snapshots / false-green traps | **Verify** |
| Board / ballot / burndown / setup | Matching **Tower** plugin skill |
