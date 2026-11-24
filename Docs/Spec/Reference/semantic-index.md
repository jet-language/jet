# Semantic index: current version and fact shape

Vocabulary: [Jet vocabulary](../vocabulary.md).

This page is the one current reference for the semantic-index version and its
fact shape. Two documents carry the version. `jet inspect semindex --json
<file.jet>` writes the index under the `semindex` member of its
`jet.status/v1` envelope. The compiler facade's `check` (`jet inspect compiler
check`, and `core.compiler` at build time) returns the typed
`CompilerSemanticIndex` projection of the same checked index as its
`semantic_index`. Other pages name this page instead of restating the number.

Current version: 20

The version changes when either document's fact shape changes incompatibly.
It is the version of the index documents only. The status envelope
(`jet.status/v1`), the compiler-facade records (`schema_version` 1,
`api_version` 1, spec.md "Compiler API") and the Canvas protocols
(canvas-protocol.md) each carry their own version (D-BIG-DIAGORIGIN1).

The guard `Tests/Guards/fact_shape_version_from_output.jet` runs both commands
and checks that the version and the top-level members each one emits match
this page.

## Fact shape

The `jet inspect semindex --json` document's top-level members, in emitted
order:

| Member | Holds |
|---|---|
| `schema_version` | The version above. |
| `definitions` | Every definition with its stable identity, scope identity, span, kind and typed detail. |
| `definition_facts` | Content-addressed definition facts: stable id, content id, human identity, kind, module and span. |
| `derivations` | The derivation records that say how each checked fact was produced. |
| `instances` | Applied generic-module instances: fingerprint, template, arguments, applications and exported members. |
| `outputs` | Declared outputs and their entry facts. |
| `state_graphs` | Declared typestate graphs: states, reachability and transitions. |
| `package_facts` | Typed package facts, or `null` outside a package. |
| `workspace_overlays` | Typed workspace-overlay facts, or `null` outside a workspace. |
| `effect_projection` | The checked effect projection, with each effect's provenance call path. |
| `references` | Every reference with its resolved definition identity; unresolved or ambiguous references carry no target. |
| `calls` | Call edges, with argument values and their provenance. |
| `effects` | Per-definition effects with provenance. |
| `arithmetic` | Arithmetic operations with their overflow policy and spans. |
| `members` | Fields, variants, inline, external inherent and trait methods and trait requirements, in stable owner order. |

## Compiler facade projection

The typed `CompilerSemanticIndex` that the compiler facade's `check` returns
as `semantic_index`, member by member, in emitted order. It projects the same
checked index; its record fields are the `core.compiler` declarations (spec.md
"Compiler API").

| Member | Holds |
|---|---|
| `schema_version` | The version above. |
| `source_digest` | The SHA-256 of the checked source bytes. |
| `definitions` | Every definition with its identity, path, scope, module, span and typed kind. |
| `references` | Every reference with its typed target. |
| `calls` | Call edges. |
| `structural_nodes` | The structural `expr`, `stmt`, `item` and `type` boundaries with their parent, slot and span. |
| `effects` | Per-function direct, callee and inferred effects with provenance. |
| `arithmetic` | Arithmetic operations with their overflow policy and spans. |
| `outputs` | Declared outputs. |
| `state_graphs` | Declared typestate graphs. |
| `fact_registry` | The registered fact declarations (effects, knowledge planes and their rules). |
| `derivations` | The derivation records that say how each checked fact was produced. |

## History

Each row is the change that moved the version. A version this table skips
was never in a recorded tree. Ratified decision text keeps the version it was
written against: D-SEMINDEX1 describes schema v12, which was current when it
was ratified. Those numbers are history, not the current version.

| Version | Change | Decisions |
|---|---|---|
| 1 | First versioned index. | D-SEMINDEX1 |
| 2 | Stable definition and scope identities; the structural audit. | |
| 3 | Member facts with their kind and origin. | |
| 4 | Content-addressed `definition_facts`. | |
| 5 | `instances` for applied generic modules. | D-GENMOD-IDENTITY1 |
| 6 | Instance template, arguments, applications and exported members. | D-GENMOD-IDENTITY1 |
| 7 | Instance application facts. | D-GENMOD-IDENTITY1 |
| 9 | Returned-view provenance on callables. | |
| 10 | Effect provenance call paths. | |
| 11 | `outputs` and their entry facts. | |
| 12 | Provenance-polymorphic views: one view can come from several source paths. | D-SEMINDEX1 (as ratified) |
| 13 | Function types gain their call contract. | |
| 14 | Call argument values and their provenance. | |
| 15 | `state_graphs`. | |
| 17 | Failure contract and failure source on callables; `arithmetic`. | |
| 18 | `derivations`; the document moves into the `jet.status/v1` envelope. | |
| 20 | Typed callable signatures: parameter source, default, access, zone and type. | D-META-REFLECT2 |
