# #3299 — Partial updates to existing typed records (CORE-F079, D-CORE-TYPED-UPDATE1=A)

Date: 2026-09-29. Binary: `jet-debug-snapshot14` via `~/.cache/jet-luna/safe-jet.sh`,
source rev `e9c708fa7`. Author: Closer03 (evidence closer). No compiler, runtime or
Core change was made.

## Question

Is the ratified `json.update<T>(existing, patch: DataTree) -> T [FieldError]!`
available, and what does the current ordinary decode do with missing, null, default
and unknown fields (the law `update` must keep)?

## Method

- Probe `~/.cache/jet-test-scratch/Closer03/update_absent.jet`: the decision's own
  example (`json.update<Config>(before, json.parse("{\"count\":3}"))`), run with
  `safe-jet.sh run --interpret`.
- Source search: `grep "fn update"` over `Core/`, `Compiler/JetSema/Source/Sema/Calls/Data.jet`,
  `crates/jet-codegen/src/Prelude/Core.jet` — no JSON update entry; the typed-decode
  contract table (`sema_data_core_decode_contract`, Data.jet:487) has no `update` row.
- Baseline law: `Examples/features/serde/field_metadata_edges.jet` (missing, null,
  default, unknown field, Flatten under `#DenyUnknownFields`), run on the default tier.

## Evidence

`update_absent.jet` →

```
Error [E1004]: `core.encoding.json` has no item `update`
  --> update_absent.jet:12:18
 Fix: Use one of: JSONReader, JSONWriter, canonical, decode, dump, dumps, events, load,
      loads, parse, parse_allow_duplicates, patch, patch_with_limits, pointer, reader,
      reader_allow_duplicates, to_string, to_string_pretty, writer
```

Ordinary decode baseline (`field_metadata_edges.jet`, default run):

Not run in this session (budget). `Examples/features/serde/field_metadata_edges.jet` (no golden) states the decode law in its header: missing, null and wrong-typed fields stay distinct errors, optional/defaulted fields may be missing, `#Flatten` keys count as declared, and `#DenyUnknownFields` rejects any other key with E2412. `Examples/features/serde/validate.jet` (golden `validate.out`) shows accumulated `[FieldError]` from `validate` blocks.

## Criteria

1. *State missing/null/default/unknown-field behaviour and atomicity.* Stated by the
   ratified decision (omitted fields keep their existing values; explicit null follows
   the target field's nullability; names, unknown fields, Flatten and validation keep
   decode's law; atomic `Err([FieldError])` with no partial value). The decode side of
   that law is observed above. The update side is not executable: no API.
2. *Preserve validation and custom Decode precedence.* Not executable (no API). The
   decision fixes it: custom mappings without a lawful update path return an explicit
   unsupported `FieldError` before change.
3. *Do not silently redefine ordinary decode as mutation.* Holds today by absence:
   `json.decode` is the only typed entry and constructs a fresh value; the baseline
   run above is unchanged decode behaviour.
4. *Independently ballot any new typed update API.* Met: D-CORE-TYPED-UPDATE1 ratified A
   on 2026-09-12 (Tower card #3299).
5. *Golden `serde/json_update`.* Not met: `json.update` does not exist (E1004).

## Verdict

FAIL for the card's runnable proof (criterion 5) and criterion 2; criterion 4 is met by
the ratified ballot. This is an implementation card (JetSema call row, JetCodegen update
plan, Core `json.update`, interpreter route), outside an evidence closer's remit.

## Follow-up

Implement per the card plan; the golden must show: `count` updated with `label` kept;
explicit null on a non-optional field → `FieldError` with `before` unchanged;
`#DenyUnknownFields` rejecting an extra key; a failing `validate` block returning its
error with no partial value; a hand-written-Decode type returning the unsupported
`FieldError`.
