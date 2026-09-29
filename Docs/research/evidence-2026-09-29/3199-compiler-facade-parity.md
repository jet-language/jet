# #3199 — Typed, JSON and compiler-facade fact parity

Closer09, 2026-09-29. Binary via `~/.cache/jet-luna/safe-jet.sh`
(jet-current = `jet-debug-snapshot16`). Source head `e9c708fa7`. Scratch
`~/.cache/jet-test-scratch/Closer09/`.

## Question

For one fixed source, does the typed `CompilerChecked` value that build and
comptime code receive hold the same facts as the CLI JSON semantic index, and
does an unavailable fact come back as explicitly unavailable?

## Method

1. Typed side: `free/facts_parity.jet` reads `compiler.parse(@SOURCE)`,
   `compiler.check(@PARSED)`, then `functions().len()`, `effects().len()`,
   `semantic_index() != None` and `semantic_index().definitions.len()` in
   comptime constants. Also ran a copy of the shipped
   `Examples/features/foundations/tooling/run.jet` (with `input.jet`,
   `package.jet`) in `tooling2/`.
2. JSON side: `safe-jet.sh check --json free/facts_src.jet` and
   `safe-jet.sh inspect semindex --json free/facts_src.jet` on the same source
   (`fn helper() -[]> Int { 41 }` / `fn run() { print(helper() + 1) }`).
3. Source reading: `Compiler/JetSema/Source/Sema/Calls/CompilerServices.jet`
   (method tables), `Compiler/JetEval/Source/CompilerServices.jet:429-433`,
   `Core/compiler/compiler.jet`, `tests/compiler_api.rs:211-362`.

## Evidence

Typed side, `safe-jet.sh run facts_parity.jet` (exit 1):

```
Error [E0956]: `__fragment: source Core call `core.compiler.parse` has no loaded signature` isn't supported by the current evaluator yet  (5:21)
Error [E0956]: `__fragment: source Core call `core.compiler.check` has no loaded signature` …  (7:22)
Error [E0956]: `__fragment: unknown checked host method `functions` on `Int`` …
Error [E0956]: `__fragment: unknown checked host method `effects` on `Int`` …
```

The shipped tooling example fails the same way (`core.compiler.lex/parse/check
has no loaded signature`, then `E0119 There's no type called CompilerLexed` at
`Core/compiler/compiler.jet:30:32`).

JSON side. `check --json facts_src.jet` (exit 0) is the `jet.check/v1`
status contract (four `not applicable` rows, E2389–E2392) and carries no
semantic index. The CLI semantic-index surface is `jet inspect semindex
--json facts_src.jet` (exit 0, `ok: true`): `semindex.schema_version: 20`,
`definitions` 2, `definition_facts` 2, `references` 2, `calls` 2, `effects` 4,
`derivations` 2, `state_graphs` 0, `instances` 0, `outputs` 0, `arithmetic` 0,
`members` 0, `package_facts` (17 keys), `effect_projection` (required 1,
granted 3, denied 0), `workspace_overlays: null`. There is no
`fact_registry` and no `structural_nodes` key in this JSON (saved at
`free/facts_src.semindex.json`).

Source reading: sema exposes `CompilerChecked.semantic_index()` as
`CompilerSemanticIndex?`, but `sema_call_compiler_value_expected_args` /
`_method_type` have no `CompilerSemanticIndex` owner, so no nested projection
(definitions, references, calls, structural/effect/output facts,
`state_graphs`, `fact_registry`, `derivations`) is reachable by method from Jet
code. JetEval sets `semantic_index = Present` only when `!has_errors &&
index.complete` (`CompilerServices.jet:429`), which does keep the error case
absent. `tests/compiler_api.rs` has no `typed_and_json_semantic_index_agree`
test, and no prebuilt `compiler_api` test binary exists in
`target-integ/debug/deps/`.

## Inventory (criteria 4/10)

Sources: canonical `jet_semindex::SymbolDef` (`crates/jet-semindex/src/Types.rs:467-492`),
typed `CompilerDefinition` / `CompilerSemanticIndex` built by the shipped
facade (`Source/Compiler.rs:1637-1658`, `:1866-1957`, fallback `:2005-2016`),
and the CLI JSON of `jet inspect semindex --json free/facts_src.jet`
(observed keys).

### Definition fields

| `SymbolDef` field | typed `CompilerDefinition` | CLI JSON `definitions[]` | class |
|---|---|---|---|
| `identity` | `identity` | `identity` | present both |
| `name` | `name` | `name`, `leaf_name` | present both |
| `module_path` | `module` | `module` | present both |
| `def_span` | `span` | `span` | present both |
| `kind` | `kind` (struct) | `detail` | present both (different shape) |
| `view_provenance` | `view_provenance` | `view_provenance` | present both |
| `declaration_identity_unavailable` | missing | `metadata_declaration_identity` | restore (serialized) |
| `callable_signature` | missing | `callable_signature` | restore (serialized) |
| `derives` | missing | `derives` | restore (serialized) |
| `nominal_base` | missing | `nominal_base` | restore (serialized) |
| `trait_contracts` | missing | `trait_contracts` | restore (serialized) |
| `qualified_name` | missing | missing | not serialized anywhere; a new public member needs a ballot |

### Index families

| family | typed `CompilerSemanticIndex` | CLI JSON `semindex` |
|---|---|---|
| definitions / references / calls / effects / arithmetic / outputs / state_graphs / derivations | present | present |
| `structural_nodes` | present | absent |
| `fact_registry` | present | absent |
| `definition_facts` (stable/signature/content ids) | absent | present |
| `instances`, `members`, `package_facts`, `effect_projection`, `workspace_overlays` | absent | present |
| `source_digest` | present | absent |

The two surfaces are two partial shapes today, which criterion 3/9 forbids;
reconciling them is implementation work (card Change 2), not a new public API,
except `qualified_name`.

### Fallback path

With no bundle (`Source/Compiler.rs:2005-2016`) the facade synthesizes
`functions` from syntax items (`compiler_function_value`) and fills the
`effects` slot with `CtValue::absent(CompilerSemanticIndex)`, a value of the
wrong type for a `[EffectInfo]` field. Neither is an explicit `Unavailable`
state (criterion 2/8).

## Verdict

- Criteria 1/7 (typed consumers read the fact families or get an explicit
  unavailable result): FAIL. On the current binary the typed facade is not
  callable at all (E0956 "has no loaded signature"); that is an evaluator gap,
  not a contract-supported unavailable result. Nested projections are not in
  the sema method table.
- Criteria 2/8 (semantic-error result keeps diagnostics, absent index; no
  synthesized fallback facts): not met. Source shows the absent-on-error rule
  (`JetEval CompilerServices.jet:429`, `Source/Compiler.rs:1995`), but the
  no-bundle fallback synthesizes `functions` from syntax and mistypes
  `effects`; nothing executed, because the typed call fails before check.
- Criteria 3/9 (exact value comparison): BLOCKED. The CLI JSON side exists
  (`jet inspect semindex --json`, values above), but the typed side cannot be
  produced on this binary, so there is nothing to compare. The JSON lacks a
  `fact_registry` family that the card lists as already serialized.
- Criteria 4/10 (SymbolDef field inventory): met by the inventory above. Five
  fields are restore-class (already serialized in the CLI JSON), one
  (`qualified_name`) would need a ballot; no new member is proposed here.
- Criteria 5/11: no change made; E0956 gating of the facade is currently the
  evaluator's generic unsupported path, not the ratified runtime gate.
- Criteria 6/12, 13: not met (the named cargo test does not exist).

## Defects

1. `core.compiler.lex/parse/check` have no loaded signature in the comptime
   evaluator: repro `~/.cache/jet-test-scratch/Closer09/tooling2/run.jet`
   (copy of the shipped `Examples/features/foundations/tooling/run.jet`), `jet
   run run.jet` → E0956 x6 + E0119 `CompilerLexed`. Expected: `compiler facts:
   tokens=…; items=…; functions=…`.
2. The CLI semantic-index JSON (`jet inspect semindex --json`) has no
   `fact_registry` key although the card lists it among the already serialized
   families; repro `free/facts_src.jet`. Either the card's list is stale or
   the serializer dropped it; needs the inventory of criterion 4.
3. No-bundle fallback in `Source/Compiler.rs:2005-2016` puts
   `CtValue::absent(CompilerSemanticIndex)` into the `effects` slot of
   `CompilerChecked` (declared `[EffectInfo]`) and synthesizes `functions`
   from syntax (source reading; not executed).
