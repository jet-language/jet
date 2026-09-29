# #3255 — DataTree losslessness across Core formats

Date: 2026-09-29. Binary: `jet-debug-snapshot14`.
Finding: `Docs/research/mine-for-jet-2026-09-12.md#finding-core-f029`.

## Question

For non-string keys, semantic tags, bytes, numeric widths and ambiguous
categories: which does the shared `DataTree` preserve, which does it reject,
and which only a typed codec carries?

## Method

Witness program: `~/.cache/jet-test-scratch/Closer01/parked/datatree_losslessness.jet`
(parked, not blessed). Every fixture prints the root `DataTree` variant plus
`json.to_string(tree)`, or the error. I ran it with
`safe-jet.sh run <file>` (Cranelift JIT). The TOML, XML and CBOR-rejection
cells ICE on the JIT, so I moved them to scratch repros:
`~/.cache/jet-test-scratch/Closer01/{toml1.jet,cbor_rej.jet}` (the latter I ran on
`jet run` and on `jet run --interpret`). The original TOML cells are kept in
`.../toml_cells.txt`.

## Evidence (observed, `jet run` unless noted)

```
json int 1: Int 1
json float 1.0: Float 1.0
json 2^63: Int 9223372036854775808
json 2^64-1: Int 18446744073709551615
json -0.0: Float -0.0
json 1e400: rejected: ? ? at byte 5, line 448, column 449, path /: non-finite JSON numbers are not allowed
json long decimal: Float 0.1
json numeric key: Object {"1":true}
json null member: a is Null null
json absent member: a absent ([at `a`: field `a` not found])
cbor uint 2^63-1: Int 9223372036854775807
cbor float64 -0.0: Float -0.0
cbor null: Null null
yaml complex key: Text "? [a]"
yaml binary tag: Object {"b":"!!binary aGk="}
yaml tilde null: n is Null null
yaml float 1.0: x is Float 1.0
yaml 2^63: x is Int 9223372036854775808
csv numeric cells: Array [["a","b"],["1","1.0"]]
typed json [U8] wire: {"id":7,"payload":[1,2,255]}
typed json [U8] back: [1, 2, 255]
typed json Int 2^63 decoded: 9223372036854775808
```

`cbor_rej.jet --interpret` (the JIT prints the first line, then ICEs):
```
cbor uint 2^64-1: rejected: CBOR integer is outside Jet Int
cbor float16 1.0: accepted 1.0
cbor tag 1: rejected: CBOR tags are unsupported
cbor bstr: rejected: CBOR byte strings are outside core.encoding.Data; use decode<[U8]>
cbor int key: rejected: CBOR map key must be text
```

TOML (`toml1.jet`, `toml.parse("n = 1")`) ICEs on every run:
`missing checked function target ...toml.jet::TomlParser::fail`. XML
(`xml.parse("<a n=\"1\">2</a>")`) ICEs:
`XmlParser::parse_element: checked arithmetic has no resolved fixed numeric type`.

## Losslessness table (criteria 1-2)

Cell legend: **P** = preserved in the dynamic tree, **R** = rejected with an
error, **C** = silently coerced (a defect), **T** = typed path only,
**?** = not observable on the current binary.

| category | JSON | CBOR | YAML | CSV | TOML | XML |
|---|---|---|---|---|---|---|
| non-string key | n/a (keys are text by grammar) | R (`map key must be text`) | **C**: `? [a]` complex key becomes Text `"? [a]"` | n/a | ? (ICE) | n/a |
| semantic tag | n/a | R (`tags are unsupported`) | **C**: `!!binary aGk=` kept as Text `"!!binary aGk="` | n/a | ? (datetime cell ICE) | ? |
| bytes | n/a; typed `[U8]` travels as an Int array (**T**, round-trips `[1,2,255]`) | R in `parse` (points to typed `decode<[U8]>`) | C (see tag) | text | ? | ? |
| integer > 2^63-1 | P as `Int` (`9223372036854775808`, `2^64-1`) | R `2^64-1` (`outside Jet Int`) | P as `Int` | text | ? | text |
| -0.0 | P | P (float64) | not run | text | ? | text |
| 1e400 | R (message garbled, see defects) | not run | not run | text | ? | text |
| long decimal | **C** to nearest double (`0.1`) | n/a | not run | text | ? | text |
| `1` vs `1.0` | P (Int / Float) | P | P (Float 1.0) | both text | ? | text |
| half float | n/a | accepted as `1.0` on the interpreter (contradicts cbor.jet:33 "half/single-precision floats rejected") | n/a | n/a | ? | n/a |
| null vs absent | P (`Null` vs `field not found`) | P (null) | P (`~` → Null) | n/a | ? | ? |

Typed path versus dynamic tree: a Codable `[U8]` field survives JSON as an Int
array, and a typed `Int` accepts `2^63` exactly (Jet `Int` is wider than i64 here).
The dynamic CBOR tree rejects both `bstr` and `2^64-1`, while JSON accepts
`2^64-1` as `Int`. The same integer therefore gets different verdicts from the
two formats.

## Algebra expansion (criterion 3)

The table needs no new `DataTree` variant. Each unrepresentable category is
already either rejected (CBOR) or a coercion defect (YAML). A public `Bytes`
variant stays forbidden (encoding-decisions.md:390-392). If Pip wants one, it
needs its own ballot. No parallel value tree was built.

## Verdict

- Criteria 1 and 2 are met by the fixtures and table above. The TOML and XML
  cells are recorded as not observable because of ICEs.
- Criterion 3 is met (no expansion needed; I built no parallel tree).
- Criterion 4 is **not met**: the golden cannot pass (TOML/XML/CBOR ICEs,
  YAML coercions), and I did not add the table to Docs/spec/encoding-decisions.md
  because the ICE cells are unresolved.
