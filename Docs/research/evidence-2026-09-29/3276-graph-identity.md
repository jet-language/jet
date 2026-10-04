# 3276 — Object identity, cycles and typed graph coding

Date: 2026-09-29. Closer02. Binary: jet-debug-snapshot14 via `~/.cache/jet-dev/safe-jet.sh`.

## Question

How does Jet's data and typed-codec model treat object identity, aliases, cycles, back-references and input-selected (polymorphic) types, compared with Jackson (`@JsonIdentityInfo`, `@JsonBackReference`, `@JsonTypeInfo`) and Serde/Glaze, which serialize values structurally?

## Method

- Source: `DataTree` is `Null|Bool|Int|Float|Text|Array|Object` (`crates/jet-codegen/src/Prelude/Core.jet:78`; the host adds `Bytes`, `Number` and `TypedText` carriers, `EncodingCodecs.rs:410-440`). None of these is an identity, reference or type-tag node. CBOR tags, including 28/29, are rejected by the host kernel (see `3271-cbor-profile.md`).
- Witness `~/.cache/jet-dev/scratch/Closer02/graph_probe.jet` (JSON cells plus CBOR tag 28/29) and `graph_nocbor.jet` (the JSON cells only), run on JIT, interpreter and AOT with `tiers.sh`.

## Evidence (`graph_nocbor.jet`: JIT exit 0, AOT exit 0, `SAME jit/aot`)

```
graph wire: {"nodes":[{"id":1,"name":"root"},{"id":2,"parent":1,"name":"child"},{"id":3,"parent":2,"name":"leaf"}]}
node 1 parent 0 root
node 2 parent 1 child
node 3 parent 2 leaf
alias wire: {"left":{"x":1,"y":2},"right":{"x":1,"y":2}}
enum wire: {"shape":{"Circle":{"radius":3}}}
---- decode attempts
accepted {"shape":{"Square":{"side":4}}}: {"shape":{"Square":{"side":4}}}
rejected {"shape":{"Triangle":{"a":1}}}: [at `shape`: no matching enum variant]
rejected {"shape":{"$type":"core.process.Command","argv":["rm"]}}: [at `shape`: no matching enum variant]
accepted {"@class":"java.lang.Runtime","shape":{"Circle":{"radius":1}}}: {"shape":{"Circle":{"radius":1}}}
id cycle decodes as data: 2 nodes, 1->2, 2->1
```

(The graph wire omits the absent optional `parent` of node 1, and the probe prints it as `0`.)

CBOR cells (`graph_probe.jet`, JIT): `cbor tag 28 shareable: rejected @0: CBOR tags are unsupported`, then `internal compiler error: JIT drop 'CBORErrorKind' enum discriminant is invalid` (exit 101). The same CBOR error path is an AOT compile failure (`cannot find type 'CBORError' in module 'jet_std'`, see 3271). On the interpreter both tags reject with `CBOR tags are unsupported` (`cbor_probe2.jet`, cells `tag 28 shareable` and `tag 29 sharedref`).

Interpreter, JSON cells: `E0956 MIR typed codec decode has no builtin decoder for [.::graph_nocbor.jet::Node] isn't supported by the current evaluator yet`.

## Findings and cells

| Cell | Status | Evidence |
|---|---|---|
| Explicit-ID graph (IDs as ordinary fields) | supported: round-trips as plain data | graph wire / node lines |
| Cycle expressed through IDs | supported as data; the application resolves it | `id cycle decodes as data: … 1->2, 2->1` |
| Implicit object identity and alias preservation | unsupported by design: aliases are duplicated | `alias wire` has two copies of `{"x":1,"y":2}` |
| In-memory cycles | not representable. Jet values are trees, with no reference field for the encoder to follow | model (Core.jet:78) |
| Back-references (`@JsonBackReference`) | unsupported (no reference node) | model |
| Input-selected type instantiation (`$type`, `@class`) | rejected. Decode dispatches only on variants of the target enum; a foreign discriminator is `no matching enum variant`, and an unknown `@class` sibling is ignored as an unknown field without instantiating anything | decode attempts |
| CBOR shared-reference tags 28/29 | rejected (`CBOR tags are unsupported`) on the interpreter; the JIT prints the rejection and then crashes on drop; AOT does not compile | above |

## Criteria

1. Separate explicit-ID graph data from implicit reconstruction: **met.** Explicit IDs are data. Implicit identity does not exist in the model.
2. Reject unsafe instantiation from untrusted input: **met on JIT and AOT.** The type is chosen by the program's target type, never by the input.
3. Keep identity, cycle and alias behaviour as an explicit profile: **met as a record.** The profile is "explicit IDs only; aliases duplicate; cycles only through IDs; tags 28/29 refused".
4. Graph or API additions need a ballot: **met.** No addition is proposed. If the owner wants a graph profile, it needs a ballot (for example, an opt-in `#Identity` Codable metadata with explicit ID and reference encoding). Nothing is drafted here, because no need was demonstrated.
5. Golden `serde/graph_identity` on AOT and default run with all four cells: **not met.** The three JSON cells agree on JIT and AOT, but the CBOR tag 28/29 cell crashes the JIT and does not compile on AOT (the CBOR error-type defect, D1/D2 in 3271). The interpreter cannot decode `[Node]`. No golden was blessed. The witness is ready at `~/.cache/jet-dev/scratch/Closer02/graph_probe.jet` for when CBOR errors work.

## Verdict

**PARTIAL.** Criteria 1-4 are met with executed evidence. Criterion 5 is blocked by the CBOR error-path defects and the interpreter's typed-decode gap.
