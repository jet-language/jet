# #3260 — Observable grouped-collection behaviour (CORE-F036)

Date: 2026-09-29. Binary: `jet-debug-snapshot14`. Author: Closer12. No compiler or Core change.

## Question

Can Jet's existing collections and reactive composition express the observable grouped-collection
jobs? Those jobs are: key change, comparer order, missing group, duplicate or empty keys,
remove-empty-group, and observer cleanup. Is a new type needed?

## Method

- Read: `Core/reactive/reactive.jet` (immutable `Signal/Computed/Derived/Effect` carriers),
  the `core.web.table` rows (`CoreCallRows.jet:1399-1430,2008`), `Examples/features/web/state_surface.jet:179-220`,
  `Core/math/combinatorics.jet:320` (`groupby` over `[Int]` runs), and the `List.group_by` law
  (`Compiler/JetFoundation/Source/Collections.jet:238,444`: Group law, eager, fresh map, source order).
- Probe 1 (`~/.cache/jet-dev/scratch/Closer12/grouped/grouped.jet`) uses a plain grouped map
  `[String: [Item]]` built with `List.group_by` and by hand, with a move helper that can drop empty groups.
- Probe 2 (`grouped_reactive.jet`) puts `reactive.signal` over the grouped map, adds an effect, sets
  the signal and calls `unsubscribe`.
- The card's named proof uses `Examples/features/ui/reactive.jet` and `reactive_scope.jet`.
- Everything ran through `tiers.sh` (`jet run`, `jet run --interpret`, `jet build` + exec).

## Evidence

### Probe 1: plain grouped map (run = AOT; interpreter differs on one line)

```
group_by keys=[, a, b]
group_by b_len=3 empty_key_len=1
insertion =[4];a=[2];b=[1, 3, 3];
missing_group_len=0 has_zzz=false        <- interpreter prints has_zzz=true
after_move_keep_empty =[4];a=[];b=[1, 3, 3, 2];
after_move_drop_empty b=[1, 3, 3, 2];c=[4];
sorted_keys=[b, c]
sorted_keys_desc=[c, b]
```

Minimal interpreter repro (`grouped/none_cmp.jet`):
`m :: [String: [Int]]{"a": [1]}; print(m.get("z") != None)` prints `false` under `jet run` and
`true` under `jet run --interpret`. The same holds for `[String: Int]`.

### Probe 2 and the named proof: `core.reactive` does not parse

Every program that imports `core.reactive` fails on all three tiers with:

```
Error [E0003]: Expected a name after `fn`, found the keyword `effect`
  --> <user file>:<line past EOF>:1
```

Root cause: `effect` is now a keyword (`KW_EFFECT_DECL`, D-EFFECT-DECL1=A,
`crates/jet-foundation/src/Syntax.rs:354-355`), and `Core/reactive/reactive.jet:46` declares
`pub fn effect(body: fn()) -> Effect`. Repro: a user file containing only `fn effect() -> Int { 1 }`
gets the same E0003 at `kw.jet:1:4`. The Core error is reported against the *user's* file, at a
line past its end (`r.jet:6:1` for a 5-line file). That misattribution is a second defect.
Because of this, `reactive.jet` and `reactive_scope.jet` both fail `jet run | diff` against their goldens.

## Capability table

| Job | Existing mechanism | Status (observed) |
|---|---|---|
| Group construction, encounter order | `List.group_by(key)` → `[K: [T]]`, keys in first-encounter order | Supported (run/interp/AOT agree) |
| Duplicate items | Kept as separate members (`b=[1, 3, 3]`) | Supported; no dedup is implied |
| Null/empty key | `""` is an ordinary key; Jet has no null key (`T?` would be explicit) | Supported |
| Missing group lookup | `groups.get(k) ?? []` is an explicit fallback; no implicit group is created | Supported on run/AOT. **Interpreter bug**: `get(missing) != None` is `true` |
| Key change (move item) | Remove from the old bucket and push to the new one (user code, O(group size)) | Supported by composition; no single operation exists |
| Remove empty group | A caller-chosen policy (`drop_empty` flag): `&groups.remove(k)` | Supported by composition. The empty-group policy is explicit, never implicit |
| Comparer / order | Map keeps insertion order; explicit `sort_by`/`sort_by_desc` on `keys()` gives comparer order | Supported; ordering is a separate projection, not a map property |
| Change notification | `core.reactive.signal` over the map, with version counters | **Unavailable tonight**: core.reactive fails to parse |
| Observer cleanup | `reactive.unsubscribe(effect)` → `is_active == false` | Unavailable (same parse failure) |
| Item-level notifications (added/removed/moved within a group) | None. A signal notifies at whole-value granularity (`set` replaces the map, `version+1`) | Unsupported. Item deltas would require diffing |
| Keyed UI rows | `core.web.table.new_keyed`, `insert_row/replace_row/update_row/remove_row`, `sort_by`, `filter_by` | Keyed rows, sorting and selection exist. **No grouping** operation exists in web.table |

### Iterator destinations vs UI notification (criterion 2)

`List.group_by` is an eager Group-law destination (`Collections.jet:238`: "a fresh map … every
item is buffered into the output"). It produces a value and has no observers.
Notification is only in `core.reactive` (`Signal.version`, effects) and `core.web.store`/`core.web.table`
state. Neither iterator traversal nor group construction notifies anyone. The two concerns share no
mechanism today, so criterion 2 holds by construction: a grouped observable, if balloted, would be
a reactive/UI carrier over a map value, never an iterator adapter.

## Ballot decision (criterion 3)

No new type is balloted from this card. Whole-value grouping, key moves, empty-group policy, ordering
and missing-group lookup are all expressible with existing collections. The one job with no
equivalent is item-level grouped change notification without an O(n) diff. It cannot be evaluated
while `core.reactive` does not parse. Re-run probe 2 after the `effect` keyword defect is fixed.
Ballot only if a signal over the map then shows lost notifications or needs O(n) rebuilds.

## Verdict

PASS on the investigation criteria. Criterion 1: every job is covered in the table above. Observer
cleanup is covered from source (`unsubscribe` sets `active` to false) and is recorded as not executable
tonight. Criterion 2: destinations and notifications are kept separate. Criterion 3: no ballot.
The card's Proof section (the `reactive` and `reactive_scope` goldens) fails on every tier because
`core.reactive` no longer parses. That, the misattributed diagnostic and the interpreter's missing-group
divergence are filed as defects.
