# #3233 — Field metadata and schema evolution (CORE-F004)

Date: 2026-09-29. Binary: `jet-debug-snapshot14`.

## Method

Witness: `~/.cache/jet-test-scratch/Closer01/parked/field_metadata.jet` (parked, not blessed).
Observed output: `.../parked/field_metadata.observed.out`.
Tiers: I ran `~/.cache/jet-test-scratch/Closer01/tiers.sh serde/field_metadata` while the file was at
`Examples/features/serde/field_metadata.jet`. Scratch repros:
`deny1.jet`, `pub1.jet`, `hd1-7.jet` (all in `~/.cache/jet-test-scratch/Closer01/`).

## Evidence (`jet run` == AOT, byte-identical)

```
lenient drops unknown: {"name":"Ada"}
strict complete: {"name":"Ada","nickname":"A","visits":3}
strict optional+default absent: {"name":"Ada","visits":0}
strict optional null: {"name":"Ada","visits":0}
strict required missing: [at `name`: expected Text, found null]
strict required null: [at `name`: expected Text, found null]
strict wrong type: [at `visits`: expected Int, found text "three"]
strict unknown key: [at `extra`: E2412: unknown field `extra`]
located flattened: {"city":"Reno","name":"Ada","zip":"89501"}
located flattened key missing: [at `zip`: expected Text, found null]
located nested form: [at `city`: expected Text, found null, at `zip`: expected Text, found null]
located unknown key: {"city":"Reno","name":"Ada","zip":"89501"}
account custom wins: ada@lovelace.org
account derived shape refused: [at `owner`: expected text, got {"address":"ada@lovelace.org"}]
published round trip: {"id":7}
```

- `jet run --interpret` stops with `E0956 core.collections.entries_to_map expects an object isn't supported by the current evaluator yet` at the `#[Codable, DenyUnknownFields]` struct that contains `#Flatten`. For a plain DenyUnknownFields struct (`deny1.jet`), the interpreter matches the JIT (`E2412: unknown field extra`).
- Custom Decode precedence: `#Codable` plus a hand-written `impl T.Decode` is `E0908 already implements Decode`. A hand-written `Decode` on an unmarked struct replaces the derived codec, including when that struct is a field of a `#Codable` struct (`account` lines above).
- Custom Decode that uses `text.split("@").to_list()` ICEs: `serde decode of Handle requires a checked decode method` (`hd4.jet`, `hd7.jet`; `hd6.jet` without split passes).

## Criteria

1. Exercised: grouped `#[Codable, DenyUnknownFields]`, `#Flatten` alone and combined with DenyUnknownFields, custom Decode precedence (E0908 when both are present; hand-written replaces derived), and required/optional/default fields. **Met as an exercise**; the defects are listed below.
2. Unknown-field rejection: **works** for a plain struct (JIT, AOT, interpreter). **Fails** under `#Flatten`: `extra` is accepted, and a nested `address` key is not reported as unknown. Not met.
3. D-BOUND-EVOLVE1=A (a PublishedSchema keeps unknown fields in order) is **not implemented**: `{"zeta":1,"id":7,"alpha":2}` round-trips as `{"id":7}`. Not met.
4. Missing versus null: **not distinguished**. Both print `expected Text, found null`; E2410 `missing required field` is never produced. Wrong type is distinct. For `T?`, absent and `null` both decode to None (both are dropped on re-encode). Not met.
5. The golden was not created because the tiers disagree and criteria 2-4 are broken. Not met.

Note: JSON encode of a struct orders keys alphabetically (`city,name,zip`), not by declaration order.
