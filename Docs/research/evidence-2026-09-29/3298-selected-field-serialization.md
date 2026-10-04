# #3298 — Selected-field serialization (CORE-F078, D-CORE-JSON-SELECT1)

Date: 2026-09-29. Binary: `jet-debug-snapshot14`. Author: Closer12. No Core, compiler or harness change.

## Question

What does today's typed Encode route give a bounded projection job? Can the strict
candidate/plain pair that D-CORE-JSON-SELECT1 needs be measured honestly tonight?

## Method

- Probe `~/.cache/jet-dev/scratch/Closer12/json/projection.jet`: a `#Codable Record` with `#Rename`,
  `#Skip`, an exact big Int (2^70), a nested object, an array and a 64-byte unselected blob. It is
  encoded whole with `json.to_string` and through the plain arm, a hand DTO `Selected` for
  `/count`, `/wire_name` and `/nested/depth`. It ran through `tiers.sh` (`jet run`, `--interpret`, AOT build+exec).
- Probe `no_selected_api.jet` calls `json.to_string_selected(value, ["/count"])` to confirm no stock API exists.
- Read: `Tools/gauntlet/harness/performance-surface.mjs` (`PAIR_ID` constant :24, hard-wired at
  :966 and :1118) and `Tools/gauntlet/measurement-manifest.json` `performance_surface_pairs` (one pair).

## Evidence

All three tiers print identical bytes:

```
whole={"count":2,"wire_name":"ada","big":1180591620717411303424,"nested":{"depth":3,"label":"inner"},"tags":["a","b"],"blob":"xxxx…(64)"}
whole_has_secret=false
dto={"count":2,"wire_name":"ada","nested":{"depth":3}}
dto_has_blob=false dto_has_secret=false
```

`jet check no_selected_api.jet` → `E1004 core.encoding.json has no item to_string_selected`. The
listed module items are `JSONReader, JSONWriter, canonical, decode, dump, dumps, events, load, loads, parse,
parse_allow_duplicates, patch, patch_with_limits, pointer, reader, reader_allow_duplicates, to_string,
to_string_pretty, writer`.

## Cell table

| Cell | Today (observed) | For option A (not implementable here) |
|---|---|---|
| Exact typed numbers | 2^70 Int is emitted exactly on all tiers | must reuse the same walker |
| Rename | `#Rename("wire_name")` → wire key `wire_name` in whole and DTO output | A's paths address wire names |
| Skip / hidden field | `#Skip secret` never emitted (whole or DTO) | no accidental hidden-field emission is shown for today's route only |
| Custom codec | not exercised (no custom-codec field in the probe) | unknown |
| Top-level + nested selection | plain arm: hand DTO with nested `NestedDepth` gives `{"count":2,"wire_name":"ada","nested":{"depth":3}}` | no candidate exists |
| Empty / duplicate / overlapping / unknown paths | not expressible: no path-selection API (E1004) | would be defined by A |
| Array-element projection | not expressible | A's ballot text already rejects index remapping |
| Privacy-safe vs invalid partial document | the DTO route emits a complete, valid JSON document of a different declared type. `#Skip` is the in-type exclusion. Neither can produce an invalid partial document | A must keep that property |
| Caller ownership / error locations | `json.to_string` borrows the value and returns a fresh String; no errors are raised in these cells | unknown for A |

## Measurement (criteria 3, 7, 9): BLOCKED tonight

- The harness measures one pair only (`PAIR_ID = "compound_assign_vs_binary_assign"`). The plan's
  step 2 (`--pair ID`) is a harness change shared with #3337, and neither `json_selected_vs_dto`
  nor any json-projection entry exists.
- There is no honest timing host. `uptime` at 02:20 showed load average 17.69/19.49/16.14 on 32 cores, with
  more than a dozen concurrent agent workers compiling. AGENTS.md's performance gate needs matched, repeatable cells.
- There is no allocation counter. `which valgrind heaptrack ltrace` finds nothing, so "allocations"
  (criterion 7) cannot be measured. Peak RSS would be a proxy, not the named metric.

No speed or allocation claim is made. D-CORE-JSON-SELECT1 option A stays unratifiable.

## Verdict

BLOCKED on measurement. Criterion 4 is met: no projection control was added, and the existing
ballot remains the gate. Criteria 1, 2 and 6 are partially evidenced for today's Encode route. The
candidate-dependent cells (5, 8) and the measurement cells (3, 7, 9) need the gauntlet pair on a quiet host.
