# CoreCallRecord field census (2026-11-10, #6615 criterion 2)

D-HOST-LEAF1=E makes the runtime an ordinary Jet library. When that lands, the
compiler's `CoreCallRecord` rows go away. This census takes each field of the
record (`Compiler/JetFoundation/Source/Registry/CoreCalls.jet:60-86`) and
settles it one of two ways:

- **Home:** where its fact lives in the runtime's Jet signature, or
- **Delete:** proof that no compiler path reads it, so it can be deleted.

All locations are at dev `76955986d0`. The readers come from `git grep -E
'\.<field>\b'` over `Compiler/` and `Tools/agent/`, with hits on other
types dropped. The row counts come from the generated table
`Compiler/JetFoundation/Source/Registry/CoreCallRows.jet`, which has 1079 rows.

Some reads don't count as semantic reads:

- `ir_core_records_match` (`Compiler/JetIR/Source/CoreRecords.jet:112-120`)
  compares every field so that it can find the canonical row. It is an
  identity check, not a use of the fact, so it is listed but never counts as
  a reader.
- The generator `Tools/agent/CoreSourceRows.jet` writes the fields, so it is
  not a reader either.

## Summary

| Field | Rows not at default | Semantic readers | Verdict |
|---|---|---|---|
| `signature.borrow_mask` | 717 with a borrowed slot | sema, IR | Home: parameter conventions |
| `fallibility` | 0 (one variant, `Sema`) | none | Delete |
| `effect_kind` | 575 | sema effects, frames, taint | Home: declared effect row |
| `effect_leaf` | 229 | sema effects, frames | Home: declared effect row |
| `capability` | 575 | none | Delete |
| `pure_route` | 39 | none | Delete |
| `coverage` | 400 SEMA-only, 679 all bits | sema (SEMA bit only) | Home: the `pub` declaration itself; the other bits are deleted |
| `aot_direct` | 648 `true` | tests only | Delete |
| `native_int_result` | 88 `true` | none | Delete |
| `frame_accesses` | 1 (`core.crypto.wipe`) | sema frames | Home: parameter conventions |
| `frame_completion` | 0 | sema frames (dead branch) | Delete |
| `sink_class` | 28 | sema taint | Home: a typed marker on the function |
| `ui_capabilities` | 16 | driver, sema (E2937) | Home: a typed marker on the function |
| `marker` | 2 (`cbor` encode/decode) | sema lifecycle facts | Home: `#Deprecated` on the Core function |
| `authority` | 0 | none | Delete |

## Fields

### `signature.borrow_mask`: home in parameter conventions

Readers:

- `Compiler/JetSema/Source/Sema/Calls/Core.jet:755-756` maps each slot to an
  argument convention: `true` is `AccessConvention.Read` and `false` is
  `AccessConvention.Move`.
- The IR passes the mask to `ir_route` as the operand borrow flags:
  `Compiler/JetIR/Source/BuildExpr.jet:2341,2346` and
  `Compiler/JetIR/Source/BuildRoutes.jet:1627,5380`.
- `Compiler/JetFoundation/Tests/RawHostKeys.jet` is a test reader.

Home: the parameter's convention in the Jet signature. A borrowed slot is an
ordinary read parameter and an owned slot is a taken (`Move`) parameter.
Ordinary calls already carry this through the checked parameter conventions.

### `fallibility`: delete

`CoreCallFallibility` has one variant, `Sema` (`CoreCalls.jet:10`), and the
generator always writes `CoreCallFallibility.Sema`
(`Tools/agent/CoreSourceRows.jet:1047`). Its only read is
`CoreRecords.jet:115` in the identity match.

Another field has the same name but a different meaning:
`TExprKind.CoreCall.fallibility: TFailureCarrier`
(`Compiler/JetSema/Source/Sema/Tree.jet:324`). Sema derives that one from the
result type: `sema_expression_failure_carrier(result_type)` at
`Calls/Core.jet:1433` and `Calls/Data.jet:1140`. The record field is never
consulted, and the function's result type (`-> T E!`) is already its home.

### `effect_kind` and `effect_leaf`: home in the declared effect row

Readers:

- `Calls/Core.jet:1086-1104` builds the call's effect bound from the root and
  the leaf.
- `Compiler/JetSema/Source/Sema/Effects/Checks.jet:2433-2434`.
- `Compiler/JetSema/Source/Sema/Scheduling/Frames.jet:98` marks the call as
  having an external effect.
- `Compiler/JetSema/Source/Sema/Taint.jet:229` reads `effect_kind`.
- `Compiler/JetFoundation/Tests/RawHostKeys.jet` is a test reader.

Home: the effect row declared on the `pub` runtime function. This is #6615
criterion 4, whose generated comparison proves that each declared row equals
the `effect_kind`/`effect_leaf` pair it replaces.

### `capability` (`CoreCapabilityContract`): delete

575 rows carry a contract other than the default
`Pure, provider: None, fallback: None, failure: None`. Its only read is
`CoreRecords.jet:118` in the identity match.

`core_call_capability_is_well_formed` (`CoreCalls.jet:136`) takes a contract,
but nothing outside its definition calls it.

Every other `.capability` hit belongs to a different type:

- the `String` field of TestKit UI rows (`Compiler/JetTestKit/Source/Ui.jet:130`);
- devserver swap state (`Compiler/JetDevserver/Source/NativeSwap.jet:469`);
- diagnostic payloads (`Compiler/Prelude/Diagnostics.jet:6876-6895,11506-11508`).

### `pure_route`: delete

39 rows name a route other than `NoPureRoute`. Its only read is
`CoreRecords.jet:116` in the identity match. No compiler path matches on
`CoreCallPureRoute.*`; the only other text that names it is the generator
(`CoreSourceRows.jet:1052`).

### `coverage`: the SEMA bit's home is the declaration; the other bits are deleted

Only two bit sets occur:

- `1` (SEMA only): 400 rows;
- `47` (all known projections): 679 rows.

Every read tests the SEMA bit:

- `core_call_projection(…, CORE_CALL_COVERAGE_SEMA, …)` at `Calls/Core.jet:574`,
  through `core_call_projection_in` (`CoreCalls.jet:208-216`);
- `Calls/Core.jet:597,1384`;
- `Compiler/JetFoundation/Source/Registry/CoreSignatures.jet:696`.

Nothing outside `CoreCalls.jet:2-6` and the test
`Compiler/JetFoundation/Tests/CoreSourceOwned.jet` reads the `TIR_SUBSET`,
`TIR_EVAL`, `AOT` or `COMPTIME` bits.

Home: a `pub` runtime function is checked, lowered and evaluated like any Jet
declaration. "Sema can see it" becomes the declaration existing and being
visible, so the field is deleted with the rows.

### `aot_direct`: delete

648 rows are `true`. Readers:

- tests only: `Compiler/JetFoundation/Tests/CoreSourceOwned.jet:21` and
  `Compiler/JetFoundation/Tests/RawHostKeys.jet:13`;
- `CoreRecords.jet:117` in the identity match.

No lowering path reads it.

### `native_int_result`: delete

88 rows are `true`. The generator sets it from `with_native_int_result`
(`CoreSourceRows.jet:965-969`). Its only read is `CoreRecords.jet:117` in the
identity match.

The comment at `CoreCalls.jet:80-81` ("engines widen them into the carrier")
has no reader behind it. The `Int` result type in the Jet signature is its
home.

### `frame_accesses`: home in parameter conventions

One row sets it: `core.crypto.wipe`, with argument 0 as `Write`
(`CoreCallRows.jet:453`). The reader is
`Scheduling/Frames.jet:63-75`, `sema_frame_core_call_operation`.

Home: argument 0 of `wipe` is a write (`&`) parameter. The ordinary-call path,
`sema_frame_checked_call_operation` (`Frames.jet:106-131`), already projects
frame accesses from the checked parameter conventions.

### `frame_completion`: delete

No row sets it: no `frame_completion: Val(…)` appears in `CoreCallRows.jet`
or `Compiler/Prelude/CoreCallExtensions.jet`. The `.Val` branch of its reader
(`Frames.jet:78-93`) is therefore dead, and the `.None` branch only rejects
accesses that have no completion metadata. Deleting the field removes the
dead branch with it.

### `sink_class`: home in a typed marker on the function

28 rows set `SinkClass.Credential`:

- the JSON, JSONL, XML and CBOR serializers;
- 24 `core.log` entry points.

The reader is `Taint.jet:281`, which refuses a Credential-tainted value at
these sinks. The Jet signature has no field for this fact, so its home is an
ordinary typed `marker` declaration applied to each of the 28 functions, the
same mechanism as C07 in `Docs/audits/jai-parity-2026-10-16.md`. Taint then
reads the marker from the callee. The marker's name is not decided here.

### `ui_capabilities`: home in a typed marker on the function

16 `core.ui.host*` rows each name one capability: `UI.FontShaping`,
`UI.FileDialog`, `UI.Shortcuts`, `UI.Accessibility`, `UI.Clipboard`, `UI.IME`
or `UI.DragDrop`.

Readers:

- `Compiler/JetDriver/Source/Driver/Loader.jet:253-259`
  (`jet_driver_is_ui_capability`);
- `Compiler/JetSema/Source/Sema/CoreUsage.jet:471-481`, which raises E2937
  against the manifest's `authority.needs`.

`UI` is not an effect root (`EFFECT_VOCABULARY_ROOTS`,
`Compiler/JetFoundation/Source/Registry/EffectVocabulary.jet:72`), so the
effect row can't carry these names without a vocabulary change. Their home is
a typed marker that carries the capability name on each function. The driver
lookup then scans the declared markers instead of `CORE_CALLS`.

### `marker` (`CoreMarkerApplication`): home in `#Deprecated`

Two rows set it (`CoreCallRows.jet:582-583`): the `cbor` members `encode` and
`decode`, retired in favour of `cbor.to_bytes` and `cbor.parse`. Both have
`since: "2027"` and `removed_in: "2028"`.

Readers:

- `Calls/Core.jet:1115-1116`, which copies it into the call fact's
  `lifecycle`;
- `CoreCalls.jet:200`, which matches a member to its marker.

Home: `#Deprecated(since: "2027", use: "cbor.to_bytes", removed_in: "2028")`
on the Core `cbor.encode` function, and the same with `use: "cbor.parse"` on
`cbor.decode`. This is the ordinary lifecycle rung
(`Docs/spec/release-policy.md:58-63`).

### `authority` (`AuthorityDecision?`): delete

No row sets it: no `authority: Val(…)` appears in `CoreCallRows.jet` or
`CoreCallExtensions.jet`. Its only read is `CoreRecords.jet:119` in the
identity match.

The checked `core.plugin.load` authority boundary is decided by name
(`core_call_authority_boundary_at`, `CoreCalls.jet:122-124`), not by this
field. Every other `.authority` hit in `Compiler/` belongs to another type,
such as manifest and CLI authority or `OutputCallableAuthority`.

## Result

- **Deleted with the rows (8 fields):** `fallibility`, `capability`,
  `pure_route`, `aot_direct`, `native_int_result`, `frame_completion`,
  `authority`, and the four non-SEMA `coverage` bits. No compiler path reads
  them.
- **Moved into the Jet signature (5 facts):**
  - `borrow_mask` and `frame_accesses` become parameter conventions;
  - `effect_kind` and `effect_leaf` become the declared effect row;
  - the SEMA bit of `coverage` becomes the `pub` declaration itself.
- **Moved to markers on the declaring function (3 fields):** `sink_class` and
  `ui_capabilities` become typed markers, and `marker` becomes `#Deprecated`.
