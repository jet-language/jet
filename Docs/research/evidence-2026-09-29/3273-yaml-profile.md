# 3273 — YAML version, trailing input, unsupported features and resource behaviour

Date: 2026-09-29. Closer02. Binary: jet-debug-snapshot14 via `~/.cache/jet-luna/safe-jet.sh`.

## Question

What does `core.encoding.yaml` do with long lines, trailing content, document markers, anchors, aliases, tags, flow collections, number forms and deep nesting? Does it fail closed as its header claims? Is rendering or parsing linear?

## Method

- Read `Core/encoding/yaml.jet:1-83` and `crates/jet-codegen/src/Prelude/Core.jet:106-107`. The Jet source module owns only `parse`. `decode<T>` and `to_string` are host (`DataFmt.rs:209-230`, `jet_std::yaml::render`).
- Witness `~/.cache/jet-test-scratch/Closer02/yaml_probe.jet` (26 cells). Perf programs are in `~/.cache/jet-test-scratch/Closer02/perf/`: `yaml_render.jet` (host `to_string`), and `yaml_parse.jet` / `yaml_parse_{125,250,500}.jet` (Jet `parse`).
- Tiers. JIT via `safe-jet.sh run`. The interpreter fails with `E0956 core.collections.slice_list_range() isn't supported by the current evaluator yet`. The AOT build of any program calling `yaml.parse` fails in rustc with `error[E0308]: mismatched types … jet_std::DataEvent::Int(…) expected i64, found JetInt` (the json.jet fast-path dependency). So `yaml.parse` runs **only on the JIT**.

## Evidence: cells (JIT, observed)

```
block map: accepted {"a":1,"b":"two","c":{"d":true}}
block list: accepted [1,"x","- nested"]                     (nested block sequence `- - x` becomes text)
literal |: accepted {"s":"line1\nline2\n"}
folded >: accepted {"s":"line1 line2\n"}
json fast path: accepted {"a":[1,2]}
numbers: accepted {"a":"0x1F","b":1000.0,"c":".inf","d":0,"e":"0o17","f":".nan","g":"1_000","h":12}
bools/null: accepted {"a":true,"b":"yes","c":null,"d":null,"e":"Off"}
long line 1MB: accepted (1000008 chars)
trailing after scalar root: accepted "hello"                  (input "hello\nworld: 1\n": the second line is silently dropped)
trailing after json root: rejected line 1 col 10 @9: unexpected trailing data after the JSON value
second document: rejected line 1 col 1 @0: YAML map entry needs ':'   (rejected, but for the wrong reason and at the wrong location)
leading ---: accepted "---"                                   (the document marker becomes the scalar root and `a: 1` is dropped)
document end ...: rejected line 1 col 1 @0: YAML map entry needs ':'
directive %YAML: accepted "%YAML 1.2"                         (the directive becomes the root and the document is dropped)
anchor: accepted {"a":"&x 1","b":2}
alias: accepted {"a":"&x 1","b":"*x"}
tag !!int: accepted {"a":"!!int 3"}
local tag: accepted {"a":"!thing x"}
flow map value: accepted {"a":"{b: 1}"}
flow seq value: accepted {"a":"[1, 2]"}
duplicate key: rejected line 1 col 1 @0: duplicate YAML map key   (the duplicate is on line 3)
bad indent: accepted {"a":{"b":1}}                            (input "a:\n  b: 1\n c: 2\n": ` c: 2` is silently dropped)
tab indent: accepted {"a":{"b":1}}                            (YAML forbids tab indentation)
depth 257: internal compiler error: typed drop value recursion limit exceeded   (exit 101; no bounded Limit error)
```

YAML 1.2 core schema comparison: `0x1F` should be int 31, `0o17` int 15, `.inf`/`.nan` floats. All four stay Text. `yes`/`Off` → Text is correct for 1.2.

## Evidence: linearity (measured, not asserted)

Render (host `yaml.to_string`, AOT binary `perf/.jet/build/yaml_render`, 3 repetitions each, run on a shared machine with other agents active):

| entries | bytes | µs (rep 0/1/2) |
|---|---|---|
| 2 500 | 65 279 | 415 / 379 / 383 |
| 5 000 | 132 779 | 756 / 715 / 710 |
| 10 000 | 267 779 | 1 344 / 1 268 / 1 351 |
| 20 000 | 557 779 | 2 724 / 2 671 / 2 700 |
| 40 000 | 1 137 779 | 5 442 / 5 275 / 5 088 |

Each doubling costs about 2×, so the render that actually ships is **linear**. The quadratic `out = "{out}…"` loops in yaml.jet's own render helpers are not on this path, because `to_string` is host.

Parse (Jet `yaml.parse`, JIT only, one sample per size because the program ICEs with `typed drop 'Int' field 0 is unavailable` after the first parse; the three smaller sizes ran as three concurrent processes):

| entries | bytes | µs |
|---|---|---|
| 125 | 2 905 | 88 751 |
| 250 | 6 030 | 336 810 |
| 500 | 12 280 | 2 147 959 |
| 1 000 | 24 780 | 12 079 261 |

Each doubling costs 3.8× to 6.4×, so the Jet parse is **superlinear (about quadratic or worse)**. A 24 KB flat map takes 12 s. The numbers are single samples on a loaded machine, so the scaling class is the finding, not the absolute times.

## Criteria

1. Exercise long lines, trailing content, nesting and aliases, numbers and document boundaries: **met (investigation).** All were exercised above. The findings are defects: silent truncation, markers taken as scalars, and a depth ICE.
2. Reject unsupported tags and features explicitly: **met as an inventory.** Anchors, aliases, tags, flow-in-block and directives are all *accepted as text* instead of rejected. This is a defect (the fail-closed claim at yaml.jet:20-21 is false), filed in the closer report.
3. Measure rather than assert linearity: **met.** Render is linear. Parse is superlinear (defect).
4. Gate any added profile or dependency: **met.** No profile or dependency is added or proposed.
5. Unsupported YAML returns `EncodingError` with the true line and column; golden `serde/yaml_profile` on every tier: **not met.** Unsupported constructs are accepted, every block error reports line 1 col 1, the interpreter and AOT cannot run `yaml.parse`, and depth 257 ICEs. No `.out` was blessed.

## Verdict

**PARTIAL.** Criteria 1-4 are met with executed evidence. Criterion 5 is blocked by the defects listed in the closer report.
