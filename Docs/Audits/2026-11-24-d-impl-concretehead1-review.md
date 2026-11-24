# Adversarial review: D-IMPL-CONCRETEHEAD1, how a byte list keeps its own encoding once list codecs are ordinary impls

Date: 2026-11-24
Reviewer: ConcreteHeadReview (Claude Opus, fresh agent)
Ballot reviewed: `$JET_SCRATCH_ROOT/tmp/orch/ballots/D-IMPL-CONCRETEHEAD1.json` (draft, author G1Generation-2, recommendation A)
Compiler for probes: `$JET_SCRATCH_ROOT/tmp/orch/batch16-chain/s1/jet` (`jet a0.0.46-dev`, built from base 7ff5df3a66)
Source read: dev worktree `Main-dev-collectall`; the batch16 source tree agrees for every line cited here
Evidence: `review-probes/p*.jet` and `review-probes/q*.jet`. Every output is in `review-probes/outputs-batch16.txt`, made by one run of all probes, one at a time (O0 for all; O1 and O2 for p1, p4, p9, q3, q4 and q9).

Verdict scale: **holds**, **holds within limits** (the limits are stated), **fails**.
Finding IDs (F1…F16) are the items sent to the author. The open/closed state is in the last section.

## 0. Context the ballot depends on

- D-BUILTIN-METHOD1=A (ratified 2026-11-18): "Bounds attach to those parameters under D-IMPL-COND1; concrete-head specialization and overlapping impls remain illegal." The same text says "Bytes retains its own type identity, never silently becomes a list alias" and "Register structural targets and fixture every supported family before removing its library route".
- Card #6852 c8 (open): "Format selectors, decode, precise math and `loop line in handle.lines()` bind through public traits: JetIR's CoreReach.jet … names no Core file path or helper." Its proof is `git grep -n -E 'Core/|__decode_|__stdout_next_line' Compiler/JetIR/Source/CoreReach.jet` printing nothing. c9: "A user type implements each of those traits and works with the language form: … decode … on o0, o1 and o2." These criteria, not D-BUILTIN-METHOD1's text, are what put the codec routes in scope. D-BUILTIN-METHOD1 names library-written built-in methods and does not mention codecs.
- D-IMPL-STRUCTBOUND1 (draft, reviewed 2026-11-24) carries Rule 1 from `land/6852-c8-structural-heads.patch`: "A head name that resolves to a type in scope makes a concrete head. It is refused with Jet.InvalidStatement: \"`impl [Point].Describe` names the type `Point` in its head\"". Its review split the concrete-head question out as U1-U3, U3 naming "`__decode_map`'s String-only key. That last one needs a partly concrete `[String: V]` head, which neither Rule 3 nor X allows."
- Two unlanded c8 patches compete for the same helpers. `land/6852-c8-codec-move.patch` (stacked on `6852-c8-structural-heads.patch`, base ce3b8e7f59) declares `Encode`/`Decode` in core.encoding, adds `impl F32.Decode` and `impl Dyn.Decode`, deletes `__decode_f32` and `__decode_dyn`, keeps `__decode_bytes` "as ruled" and defers the rest to this ballot. It notes that "`-> Self` in an impl on a builtin is not substituted on the actual side". A user `impl F32.Decode` is refused with ImplOnBuiltinType. `land/6852-c8-decode-lines.patch` instead marks all eight helpers with a role (`#Builtin.Role(.DecodeBytes)` and so on, read by `ir_codec_decode_roles`) and keeps their names for the seed bootstrap.
- The byte rule today lives in more places than the ballot lists:
  - Decode, tree: `ir_dyn_decode` (BuildDyn.jet:545-573) → `ir_codec_decode_helper` (707-724; `[U8]` → `__decode_bytes` at 711) → `ir_dyn_fixed_decode` (578, `__decode_fixed_list`) → `ir_dyn_union_decode` → `jet_rt_codec_decode_{scalar}` (572).
  - Decode, streaming: `ir_decoding_read` (BuildDyn.jet:1063-1114) has its **own** byte exclusion (1084-1093). `[U8]` and fixed lists skip `read_list` and fall to body capture plus tree decode.
  - Encode: `ir_serde_encode` (BuildDyn.jet:783-850), with bytes at 800-810 through `ir_sequence_element` (BuildTypes.jet:4675) and an `ir_list_operand` copy for an inline `[U8#N]`. Also `ir_dyn_array_encode`, `ir_dyn_option_encode`, `ir_dyn_object_encode` (String keys only) and `ir_dyn_union_encode`.
  - Reachability: `ir_codec_decode_helpers` (CoreReach.jet:106-107, eight names), `ir_decoding_reads` (212-213), `ir_core_decoding_read`. The comptime tier has `jet_driver_comptime_codec_refs` (JetDriver Pipeline.jet:1423-1446), which loops `ir_codec_decode_helpers()` and matches the source path `Core/Encoding/Encoding.jet`. Also `Compiler/JetCLI/hot-text.list:9193`.
  - Sema, two tables that both say "a list is Codable when its element is":
    - `sema_type_codec_structural_bound` (JetSema TypeRelations.jet:2820-2848). It covers List, Option and FixedList by element, a Map by key **and** value, a Union by its members, and the named Dyn, DateTime, ZonedDateTime, JSON, TOML, YAML and CSV. Scalars are matched by name at 2807-2816.
    - `sema_serde_type_is_admissible_in` (JetSema Serde.jet:278-319). It covers List, Option, **Shared** and FixedList by inner type, a Map **only with a String key** (289), and a hardcoded Named list (Decimal, Date, LocalDate, LocalTime, DateTime, Duration, ZonedDateTime, Dyn, JSON, TOML, YAML, CSV).
    - The two tables disagree on `[U8: V]`.
  - Trait rows: `through_elements: Bool{true}` on Encode and Decode (Prelude Traits.jet:142-161; OwnedTraitRows.jet:33-34).
  - Runtime: the U8 adapter `jet_rt_codec_decode_u8` (DynCodec.jet:530) is one of nine IntN adapters, plus Int, over one `codec_decode_integer` (452-508). That function accepts Int, exact Number lexemes ("JSON number `1.5` is not an exact integer") and, inside the thread-local text-cell scope that CSV cells and the environment decode in, a Text leaf. It writes the range error "expected U8, found out-of-range Int". Its route rows are RuntimeRoutes.jet:1175 and RuntimeCExports.jet.
- Spec: encoding-decisions.md:960-973 (a Codable `[U8]` is CBOR major type 2, never an integer array; typed decode accepts only T's one schema), :224 (JSON writes bytes as padded base64), :1121-1146 (ETF `[U8]` and `byte_arrays`). spec.md:1083-1094 (D-CODABLE-DISTINCT1: a distinct type "codes as that base value, as a field, an element or a top-level value"). spec.md:4936-4965: an existing `Bytes` type, a growable byte builder with a read cursor.

## 1. Strongest existing solutions, stated fairly

| Language / library | How a byte list gets its own wire form | Best case | What it does that this design does not, or the ballot omits |
|---|---|---|---|
| Haskell `Show`/`Read` | `showList`/`readList` class methods with defaults; `instance Show a => Show [a] where showsPrec _ = showList`; `Char` overrides it | This is A exactly. Report §6.3.3: "The method showList is provided to allow the programmer to give a specialised way of showing lists of values. This is particularly useful for the Char type, where values of type String should be shown in double quotes, rather than between square brackets." | The hook belongs to the element class. A `newtype` with stock deriving loses the override, while `GeneralizedNewtypeDeriving` forwards it [INFERENCE, from GHC's deriving rules; relevant to F11]. Source: Haskell 2010 Report ch. 6.3.3 (haskell.org/onlinereport/haskell2010/haskellch6.html). |
| aeson (Haskell JSON) | `ToJSON` has `toJSONList`/`toEncodingList`, `FromJSON` has `parseJSONList :: Value -> Parser [a]`, all with defaults; the list instance calls them; `Char` overrides them so `String` is JSON text. Map keys go through a separate key hook class pair, `ToJSONKey`/`FromJSONKey` | **A's exact mechanism, in a codec, in production for over a decade**, and its key-class pair is the F9 answer for maps. The ballot cites `Show`, a weaker analogue. | The hook spreads: `ToJSON1`'s `liftToJSON` must carry the list encoder, and every container class threads it. Source: hackage.haskell.org/package/aeson, `Data.Aeson.Types` (classes ToJSON, FromJSON, ToJSONKey, FromJSONKey). |
| binary (Haskell) | `class Binary t where put; get; putList :: [t] -> Put` | Same hook on a binary codec; `Word8` lists can pack. | Same as above. Source: hackage.haskell.org/package/binary, `Data.Binary` class Binary. |
| Go `encoding/json` | `[]byte` encodes as a base64 string by a reflect kind test in the encoder; in v1, a byte array `[N]byte` encodes as an array of numbers; an element type that implements `Marshaler`/`TextMarshaler` opts its slices out of base64 | **D's model**, a library-internal type test, shipped since Go 1.0. Missing from the ballot, though it is the closest precedent for today's route. | v1's fixed-array split is the opposite of Jet's `[U8#N]` rule, and Go json v2 changed it to Jet's: "In contrast, in v2 a Go byte array is represented as a Base64-encoded JSON string. The FormatByteArrayAsArray option controls this behavior difference." Source: pkg.go.dev/encoding/json (Marshal: "[]byte encodes as a base64-encoded string", and the v1/v2 notes). |
| Rust std + serde | `min_specialization` is used **inside std** (for example `SpecToString` and `SpecFromElem`) and is closed to other crates; serde writes `Vec<u8>` as a sequence; `serde_bytes` opts in to bytes | **B's exact model**: specialization only by the trait's owner, in production inside std for years. The ballot cites only RFC 1210's unstable public feature. | serde_bytes README: "Without specialization, Rust forces Serde to treat &[u8] just like any other slice and Vec<u8> just like any other vector." In serde the **format** chooses the bytes form, and serde_json writes bytes as a number array. Sources: doc.rust-lang.org/unstable-book/language-features/min-specialization.html; RFC 1210 (rust-lang.github.io/rfcs/1210-impl-specialization.html); github.com/serde-rs/bytes README. |
| Swift Foundation | `Data` is its own type; `JSONEncoder.dataEncodingStrategy` defaults to `.base64`; `[UInt8]` encodes as a number array | **C's model.** | `Dictionary: Encodable` uses runtime tests `Key.self == String.self` / `Int.self`, and encodes other keys as an alternating array. SE-0320 later added the `CodingKeyRepresentable` protocol: a **key hook**, which is A's mechanism applied to map keys (F9). Sources: developer.apple.com/documentation/foundation/jsonencoder/dataencodingstrategy; SE-0320 (github.com/swiftlang/swift-evolution/blob/main/proposals/0320-codingkeyrepresentable.md). |
| C++ | partial specialization; `std::vector<bool>` | The cautionary case is fair. | Source: ISO C++ [vector.bool]. |

The ballot's Swift row ("a type conforms to a protocol once") is true, but it leaves out that Swift's own codec does D-style runtime type tests and later grew an A-style key hook. Its Rust row leaves out that std uses B internally. Both omissions make A look more singular than it is (F12).

## 2. Claims and verdicts

### C1. Situation: the byte rule, the spec requirement, and "D-BUILTIN-METHOD1 moves such routes onto ordinary impls"
The rule and spec hold (p1: JSON `"pixels":"AQI="`, CBOR major type 2). The routing sentence needs its source: D-BUILTIN-METHOD1 does not name codecs. Card #6852 c8/c9 ("bind through public traits") does (F3).
**Verdict: holds within limits.**

### C2. Detail: the inventory of routes
It is missing the streaming reader's own byte test (BuildDyn.jet:1084-1093), both Sema conformance tables, the `through_elements` rows, the comptime tier (Pipeline.jet:1423-1446), the runtime U8 route rows, the encode routes for list, option, map and union, and `hot-text.list`. Under ballot-review.md's whole-mechanism rule, a missing site is a failed claim (F1).
**Verdict: fails.**

### C3. "In every option, a head with a fixed argument that no other impl overlaps, such as `impl [String: T].Decode`, is legal"
This contradicts the draft D-IMPL-STRUCTBOUND1 Rule 1 ("names the type `String` in its head") and the reading of "concrete-head specialization" that ballot's review settled (U3). It also blocks a later general `[K: V].Decode` without overlap, which is the same problem this ballot solves for bytes. Probes q7 and q8 show non-String keys are not Codable today. q7's Fix line proposes `impl [Int:String].Decode`, a head that every option refuses (F9).
**Verdict: fails.**

### C4. "F32 and Dyn already decode through ordinary impls in core.encoding (#6852 c8)"
False on dev and batch16: `__decode_f32` and `__decode_dyn` are live in CoreReach.jet:106-107 and BuildDyn.jet:707-724. The impls exist only in the unlanded `land/6852-c8-codec-move.patch`. A's `Decode: trait {…}` in core.encoding presupposes that patch (F2).
**Verdict: fails.**

### C5. Lesson: "Jet builds a separate copy of a generic function for each set of types … at every call the compiler knows the exact type"
Refuted today for `[T]` beside `[T#N]`:
- p9: a generic function instantiated at both `[U8]` and `[U8#2]` prints `[2, 0]` for `[1, 2]` and gets SIGSEGV at O0. At O1 and O2 it is an ICE (`argument 0 s138 of Aggregate/8 ([U8#2]) for parameter Aggregate/24 ([U8]) of function 2 show`).
- p4 and p5: a `#Codable` struct with both a `[U8]` and a `[U8#2]` field, or with I16 elements instead, ICEs at `__read` ("a value of [U8] where [U8] is expected").
- p2 and p6-p8: each type alone is correct.

So the two instances collide. Every option's `[U8#N]` claim, and B's "Instances carry their full types", depend on fixing this (F8).
**Verdict: fails** (defect reported to Main).

### C6. A: "`impl [T].Decode` calls `T.decode_list` … U8's impl overrides the hook"
The dispatch works: q3 (an element override plus `impl Many<T: Dec>.Dec` calling `T.dec_list`, nested) prints `2 3 4` at O0, O1 and O2. A static default body without `Self` is inherited and overridden correctly (q4). But the **default body A shows cannot be written today**:
- q1: `Self.dec(item)` in a trait default body gives `Jet.UnknownName: Nothing named \`Self\` exists here`.
- q2: `item.decode<Self>()` gives `Jet.TraitNotImplemented: \`Self\` does not implement \`Decode\``, and `[Self]` is not substituted (`This needs [Big], but the value is [Self]`).
- The codec-move patch records that `-> Self` is not substituted on builtins.

This is language work that A needs and the ballot does not name (F4).
**Verdict: holds within limits** (the limit is a named prerequisite).

### C7. A loss: "U8 decode moves from the runtime adapter into a source impl"; mitigation "a range-checked Int decode"
The adapter does more than a range check: Number lexemes, the CSV and environment text-cell scope, and exact error texts. U8 would also become the one fixed-width integer decoded in source while I8, I16, I32, I64, U16, U32 and U64 stay in the runtime. That is two mechanisms for one meaning (I8) and a tier-divergence risk (I9) (F5).
**Verdict: fails.**

### C8. A: "The streaming reader gets the matching hook"
Unspecified. `__DecodeSource` is Core-private, so a public `Decode` member cannot take it. Today's streaming byte test (BuildDyn.jet:1084-1093) is a third U8 type test that A must remove too. There is precedent for private, generated `Decode` members (derived `__read`, the `__env_names` constant at BuildTraits.jet:494-505) (F6).
**Verdict: fails.**

### C9. A: "Encode gains `encode_list` the same way, which U8 answers with `Dyn.Bytes`"
No signature is given. For `impl [T#N].Encode`, a hook that takes `[Self]` forces the inline-to-List copy (`ir_list_operand`), which is today's cost but must be stated. A view or slice parameter avoids it. The fate of `ir_dyn_array_encode`, `ir_dyn_option_encode`, `ir_dyn_object_encode` and `ir_dyn_union_encode` is not stated (F7).
**Verdict: fails.**

### C10. A: "Any type may override the hooks, which is public API … packed lists are a real use"
Two cases are unstated:
- Who may write the hook for a builtin. Under codec-move only core.encoding may, and a user impl gets ImplOnBuiltinType.
- What a distinct type does. q9: `Level: distinct U8` gives `[Level]` encoding `[1,2]`, and base64 text refused, today at O0, O1 and O2. If D-CODABLE-DISTINCT1's forwarding also forwards the hooks, `[Level]` silently changes wire form.

"Packed lists are a real use" has no example (F11).
**Verdict: holds within limits.**

### C11. A: "Hand-written impls inherit the defaults and need no edit"
True once C6's prerequisite lands. Derives must not generate the hooks.
**Verdict: holds within limits.**

### C12. B: technical text, and whyNot "Overlap needs a specificity order … `[U8#N]` needs a second impl"
- "Instances carry their full types" is refuted today (C5).
- "Jet types lack lifetimes" leaves out tags: `ir_untag` peels `Tagged` before the byte test, so B must say whether `impl [U8]` matches a tagged U8 list.
- The best case is not credited: no public hook, no language work for `Self` defaults, the rule reads as one impl, and it is Rust std's internal model.
- The whyNot's second clause is weak, because A also needs `impl [T#N].Decode`.
- q5: an attempted `impl Many<Small>.Name` is refused as "`impl Many` takes no type parameters" with "Fix: Write `impl Many {`". That is misleading for an overlap attempt (F13, F15).

**Verdict: holds within limits** (the whyNot holds on the specificity order and soundness review).

### C13. C: "A distinct `Bytes` type over `[U8]`"
The name is taken: spec.md:4936-4965 defines `Bytes`, a byte builder with a read cursor. q6 shows it is not Codable, and the error is a misdiagnosis (`No trait named \`Encode\` is in scope for the bound on \`T\``). D-BUILTIN-METHOD1 says "Bytes retains its own type identity". C must name its type and its relation to the builder (F10).
**Verdict: fails.**

### C14. D: "The route stays invisible to a reader of Core … the IR keeps a type test on U8"
- With `land/6852-c8-decode-lines.patch`, D's strongest form (D′) puts a visible `#Builtin.Role(.DecodeBytes)` declaration in Core through the ratified role door. That form passes c8's proof grep.
- There are three IR type tests (711, 804, 1086-1090), not one.
- D loses on c8's text ("bind through public traits") and c9's (a user type implements the trait), not on D-BUILTIN-METHOD1.

(F3)
**Verdict: fails as stated.**

### C15. inWild and comparisons
No sources. It leaves out aeson's `toJSONList`/`parseJSONList` (A's exact precedent), Go's `[]byte` base64 kind test (D's precedent), Rust std's `min_specialization` (B's precedent), and Swift's `Data` base64 default with its Dictionary key tests and SE-0320 (C's precedent and the key hook) (F12).
**Verdict: fails.**

### C16. Recommendation gains: "One impl per type", "`[U8]`, `[U8#N]` and the Encode side follow one hook", "The by-name helpers and the hardcoded encode leave the compiler"
- The first holds by construction.
- The second depends on the C5 defect and C9's signature.
- The third holds only once C2's inventory is complete (F1, F7, F8).

**Verdict: holds within limits.**

### C17. Cases: "cases at every optimization level pin its results and errors"
No case list. The edge cases are unpinned: `[[U8]]`, `[U8]?`, `[String: [U8]]`, `U8?`, `[I8]`, `[U8]` beside `[U8#N]` in one program, `[Level]`, generic `load<[U8]>`/`load<[U8#2]>`, the streaming JSON field path, CBOR major type 2, a U8 CSV text cell, comptime decode, error texts (F14).
**Verdict: fails.**

### C18. Surface
It mirrors the body but carries none of the prerequisites (C6, C5) or D′ (F16).
**Verdict: holds within limits.**

### Checklist (AGENTS.md "Adversarial design review" and ballot-review.md)

| Item | State in the draft |
|---|---|
| Every site of the mechanism named, one source of truth | Fails (C2, C8, C9; F1, F6, F7) |
| Siblings in the same store (other `__decode_*`, map keys, IntN adapters, Shared, union) | Fails (F1, F5, F9) |
| Second store kept with numbers and reasons | Fails (two Sema tables, F1) |
| I8 one mechanism | Fails for U8 source decode (F5) and streaming (F6) |
| I9 one meaning at O0/O1/O2 and web | Fails today for `[T]`/`[T#N]` (F8); A's mechanism holds at O0/O1/O2 (q3) |
| Strongest prior art, sourced | Fails (F12) |
| Each option at its strongest | Fails for B and D (F3, F13) |
| Code in options compiles, or its prerequisite is named | Fails (F4, F7) |
| Ratified text quoted correctly | Fails (C3, F9; C1, F3) |
| Tooling, diagnostics, docs | Fails (F15) |

## 3. Findings sent to the author (round 0)

- **F1 Inventory.** Add an inventory paragraph that gives each site as `file:symbol` and its fate under each option: BuildDyn.jet `ir_codec_decode_helper` 707-724, `ir_dyn_fixed_decode` 578, `ir_decode_under` 687, `ir_serde_encode` 783-850 (bytes 800-810), `ir_decoding_read` 1063-1114 (**its byte exclusion 1084-1093**); CoreReach.jet `ir_codec_decode_helpers` 106-107, `ir_decoding_reads` 212-213, `ir_core_decoding_read`; Pipeline.jet `jet_driver_comptime_codec_refs` 1423-1446; TypeRelations.jet `sema_type_codec_structural_bound` 2807-2848; Serde.jet `sema_serde_type_is_admissible_in` 278-319; Traits.jet / OwnedTraitRows.jet `through_elements`; RuntimeRoutes.jet:1175, RuntimeCExports.jet, DynCodec.jet:530 (U8 adapter); hot-text.list. For any site kept, give the reason. State which ballot or card owns the non-byte siblings: union, Shared, and the named Dyn/DateTime/… lists.
- **F2 c8 claim.** Rewrite to: "PROPOSED in the unlanded `land/6852-c8-codec-move.patch` (base ce3b8e7f59, stacked on `6852-c8-structural-heads.patch`): `impl F32.Decode` and `impl Dyn.Decode` in core.encoding; on dev `__decode_f32` and `__decode_dyn` are still live." Say that A's `Decode: trait` in core.encoding presupposes that patch.
- **F3 D at its strongest, and the source of scope.**
  - Add D′, the role door of `land/6852-c8-decode-lines.patch` (`#Builtin.Role(.DecodeBytes) __decode_bytes`), and show that it passes c8's proof grep.
  - Replace "invisible to a reader of Core" with the real cost: three IR type tests on U8 (BuildDyn.jet:711, 804, 1086-1090), the hardcoded encode, and failure of card #6852 c8's text "bind through public traits" and c9 "A user type implements each of those traits".
  - In the situation, cite c8/c9 for "moves such routes onto ordinary impls".
  - State which of the two c8 patches this ballot supersedes.
- **F4 A's prerequisite.** Add to A: "Requires default trait bodies that name `Self` and `[Self]` (today: q1 `Jet.UnknownName` for `Self`; q2 `\`Self\` does not implement \`Decode\`` and `[Self]` unsubstituted), and `-> Self` substitution on builtin impls (noted in codec-move)." Name the card and cost. Cite q3 (O0/O1/O2 `2 3 4`) as evidence that the dispatch itself works. Add the prerequisite to losses.
- **F5 U8 source decode.** Replace the U8 decode body and its loss row. Say that `impl U8.Decode`'s `decode` calls the canonical adapter `codec_decode_integer` (DynCodec.jet:452-508) through the D-CORE-BUILTINS1 role door, so U8 decodes exactly like I8…U64. That covers Number lexemes, the CSV/env text-cell scope and the error text "expected U8, found out-of-range Int". Only `decode_list` is new source. Alternatively, move all ten integer adapters together. Either way, say which one, and why U8 alone in source does not split the IntN family (I8/I9).
- **F6 Streaming.** Specify the streaming hook: for example a private, compiler-known member `__read_list<Source: __DecodeSource>` on Decode, with a default `read_list` and a U8 override that captures and decodes bytes, matching the derived `__read`. Alternatively, say that list streaming stays a capture. State how `impl [T].Decode`'s `__read` reaches it without a type test, and that BuildDyn.jet:1084-1093 is deleted.
- **F7 Encode.** Show `Encode`'s hook signature (`encode_list: fn(items: [Self]) -> Dyn`, or a view), the bodies of `impl [T].Encode` and `impl [T#N].Encode`, and the copy cost for inline `[U8#N]` (today one `ir_list_operand` copy). Also state the fate of `ir_dyn_array_encode`, `ir_dyn_option_encode`, `ir_dyn_object_encode` and `ir_dyn_union_encode`. Show `impl [T#N].Decode` constructing `[T#N]` from `[T]` in source.
- **F8 Instance collision.** Correct the lesson ("knows the exact type" is false today for `[T]` beside `[T#N]`) and B's "Instances carry their full types". Add as a prerequisite for every option's `[U8#N]` claim: p9 (O0 `[2, 0]` and SIGSEGV, O1/O2 ICE `argument 0 s138 of Aggregate/8 ([U8#2]) for parameter Aggregate/24 ([U8])`) and p4/p5 (`__read` ICE "a value of [U8] where [U8] is expected", also with I16).
- **F9 Map keys.** Delete "`impl [String: T].Decode` is legal in every option": it contradicts D-IMPL-STRUCTBOUND1 Rule 1 and U3. Apply A's own mechanism to the sibling instead:
  - `impl [K: V].Decode` with a key bound, either a key hook on Decode/Encode (`decode_key`/`encode_key`, as Swift SE-0320 `CodingKeyRepresentable`) or a Core key trait that only String implements now.
  - No concrete head, and later keys need no overlap.
  - Say that this also resolves the Sema disagreement on `[U8: V]` (TypeRelations.jet:2820-2848 requires the key to be Decode; Serde.jet:289 requires String).
  - Owe q7's Fix line, which suggests `impl [Int:String].Decode`.
- **F10 C's type.** Say that `Bytes` already exists (spec.md:4936-4965, a byte builder with a read cursor, not Codable: q6). Either C makes that type Codable (and the cursor's wire meaning is stated) or C names a new type. Reconcile with D-BUILTIN-METHOD1's "Bytes retains its own type identity". Add a count of Core's Codable `[U8]` fields to C's migration cost.
- **F11 Who overrides, and distinct types.** State that only a type's owner writes its hooks, so `impl U8.Decode` lives only in core.encoding and a user gets ImplOnBuiltinType (codec-move). State that a distinct type's forwarded codec (D-CODABLE-DISTINCT1) does **not** forward the hooks, so `[Level]` with `Level: distinct U8` keeps today's `[1,2]` and refuses base64 (q9, all levels). Otherwise name the wire migration. Give a concrete "packed lists" example or drop the mitigation.
- **F12 Comparisons.** Add sources and the rows in section 1:
  - aeson `toJSONList`/`parseJSONList` (A's precedent in a codec);
  - binary `putList`;
  - the Haskell 2010 Report reason for `showList`;
  - Go `encoding/json` `[]byte` base64 by kind test, `[N]byte` as an array, Marshaler opt-out (D's precedent);
  - Rust std's `min_specialization` (B's precedent);
  - the serde_bytes README quote;
  - Swift `JSONEncoder.dataEncodingStrategy` `.base64`, `[UInt8]` as numbers, Dictionary key tests and SE-0320.
- **F13 B at its strongest.** Credit B's gains: no public hook members, no `Self`-default language work, the rule is one readable impl, Rust std's own model. State tag matching (does `impl [U8]` match a tagged U8 list after `ir_untag`?). Rewrite whyNot as "a specificity order, an overlap soundness review, and a new public power for every trait owner (I8: hooks and specialization would then both exist)". Drop "`[U8#N]` needs a second impl", which A also needs.
- **F14 Cases.** List the cases each option must pin at O0, O1 and O2:
  - `[U8]`; `[U8#N]`; both in one struct and one generic function (p4, p9);
  - `[[U8]]`, `[U8]?`, `[String: [U8]]`, `U8?`, `[I8]` (today's outputs in p1/p3);
  - `[Level]` (q9);
  - generic `load<[U8]>`;
  - the streaming JSON field path, CBOR major type 2, a CSV text cell for `U8` and `[U8]`;
  - comptime decode;
  - the error texts.
- **F15 Tooling.**
  - Fix q7's Fix line (an illegal concrete-head suggestion).
  - Fix q5's refusal for an overlap attempt (`impl Many<Small>.Name` → "takes no type parameters").
  - Fix q9's Fix line (`Level.from_U8` does not exist; `from_int` does).
  - Fix q6's misdiagnosis.
  - Show `decode_list`/`encode_list` in `jet inspect expand --facts derive`, LSP hover and Core docs.
- **F16 Surface.** Add A's prerequisites (F4, F8) to surface losses, D′ (F3) to D's row, and the map-key decision (F9).

### Round 1 findings (R1-R8, sent 2026-11-24 after the 01:08 revision)

- **R1 D′ cites a patch that does not exist.** `land/6852-c8-decode-lines-r2.patch` is not in `$JET_SCRATCH_ROOT/tmp/orch/land/`. The only decode-lines patch there is `6852-c8-decode-lines.patch` (2026-11-23 21:42). That patch gives all eight helpers roles, f32 and dyn included, and is not stacked on seed (b). Fix: either write the r2 patch before the next round, or cite the existing patch and mark the six-helper form as PROPOSED.
- **R2 U8's decode names the wrong door.** The role door (`#Builtin.Role`, Core/Builtins/Builtins.jet:22-53) is the compiler running a Core function. U8's `decode` calling the runtime adapter goes the other way. That is a jet_builtins host operation: "its row in Compiler/Prelude/Core.rows (`dispatcher_row … jet_builtins <name>`) and its typed signature in the compiler's host signature table are the whole operation" (Builtins.jet:17-20). Fix:
  - Write the body as `builtin.codec_decode_u8(tree)`, and name its Core.rows row.
  - Say that the IR scalar arm (BuildDyn.jet:572) keeps the other nine integer adapters, so both paths end in the same `codec_decode_integer` (one meaning, two call sites).
  - Or route all ten integer types the same way.
- **R3 A's streaming hook cannot be written today.**
  - q11: a trait member `dec_many: fn<Source: Src>(source: Source) -> Int { … }` with a default gives `Jet.UnknownType: There's no type called \`Source\``.
  - q12: without a default, the declaration is accepted. But `T.dec_many(Text{…})` through a bound gives "`dec_many` wants `Source` for argument 1, but this is `Text`" (O0; q11 at O0/O1/O2).

  Fix: add a fourth prerequisite, method-level type parameters on trait members that can be called through a bound, and put it in the loss row and the cost estimate. Or make `__read_list` compiler-generated like `__read`, and say how U8's override is found without a type test.
- **R4 C's Core migration is understated.** "Core itself declares no Codable struct with a `[U8]` field" holds only for markers. Encode and Decode auto-derive "when every field qualifies" (spec.md:1063-1065), and Core does not deny `auto_derive`. q10: the exported `core.crypto.Digest256` encodes as `{"bytes":"ypeBEsobvcr6wjGzmiPcTaeG7/gUfE5yuYB3ha/uSLs="}` and round-trips at O0, O1 and O2. Fix:
  - Say that N exported Core structs auto-derive a codec over a `[U8]` field (Digest256, Digest512, …, with a count), and that C changes their JSON from base64 to number arrays.
  - Change C's whyNot and surface to "in Core and in programs".
- **R5 The detail contradicts itself.** "Under A, every container decode helper and both IR byte tests go" conflicts with the three tests named elsewhere (711, 804, 1084-1093). Fix: "all three IR byte tests".
- **R6 `__decode_under` and the c8 claim.** "Every option keeps … `ir_decode_under`", but CoreReach.jet still names `__decode_under`, so c8's proof grep (`__decode_`) cannot pass. Union, Shared and the named types also stay with c8. Fix:
  - Give `__decode_under`'s route under every option: a role, or a `FieldError` method reached by ordinary reachability.
  - Change gain 2 from "meets #6852 c8 and c9" to "does A's part of #6852 c8 and c9 (list, fixed list, option, map, bytes)".
- **R7 The map-key trait is a placeholder.** "The decision on the key trait's name and members goes with this ballot's ratification" leaves the ratified text undecided. Fix:
  - Give the trait's name, members and signatures. For example, `MapKey: trait { key_text: fn(self) -> String, from_key_text: fn(text: String) -> Self [FieldError]! }` (it needs the same `-> Self` substitution as F4), plus `impl String.MapKey` in core.encoding.
  - Give the bound on `impl [K: V].Decode` and `.Encode` in D-IMPL-STRUCTBOUND1's spelling, and say that the bound on K is the map-key rule that review's F3 called new.
  - Give the text of q7's new Fix line.
- **R8 B's fixed-list wording.** "Like A, B needs `impl [U8#N].Decode`" is wrong: A needs the generic `impl [T#N].Decode`, while B needs both `impl [T#N]` and `impl [U8#N]`. Fix the sentence.

### Round 2 finding (R9, sent 2026-11-24 after the 01:29 revision)

- **R9 Cite the count's evidence.** C's technical text says "At least 28 exported Core structs … That count is from probe r1-u8 (batch17 compiler and its Core)", but no path is given, and no file named r1-u8 is under `$JET_SCRATCH_ROOT/tmp/orch`. Fix: cite the probe's path and compiler path. Otherwise, cite q10 and q13 here (q13 bounds Signature, VerifyKey, UDPPacket, Image and JobPayload by `[Encode, Decode]` and prints 5 on batch16) and drop the exact count to "at least five, including …".

## 4. Re-review log

- 2026-11-24, round 0: findings F1-F16 sent to G1Generation-2. Defects to be filed as cards are listed in the review's final result to Main: the p4/p5/p9 instance collision (wrong value, SIGSEGV at O0, ICE at O1/O2), and the misleading texts of q5, q6, q7 and q9.
- 2026-11-24, round 1 (ballot written 01:08:50). Verified against dev 986dd70fd1, `land/6852-c8-codec-seed-{a,b}.{msg,patch}` (e6e448e550 is in dev; (b) deletes `__decode_f32/__decode_dyn` and adds `impl F32.Decode`/`impl Dyn.Decode`, as the ballot says) and new probes q10-q12.
  - Closed: F2, F4, F7, F8, F11, F12, F13, F14, F15 (the texts are owed under every option), F16 (except the R3 prerequisite).
  - F1 closed except R5 and R6. F3 open as R1. F5 open as R2. F6 open as R3. F9 open as R7. F10 open as R4.
  - New: R1-R8.
- 2026-11-24, round 2 (ballot written 01:29:20). Diffed against the round 1 text.
  - R1 closed: `land/6852-c8-decode-lines-r2.{msg,patch}` now exists. It is stacked on seed (b) over e6e448e550, and gives roles to exactly six helpers (DecodeList, DecodeFixedList, DecodeBytes, DecodeOption, DecodeMap and DecodeUnder; I checked the patch's `+#Builtin.Role` lines). D′'s code matches it.
  - R2 closed: `builtin.codec_decode_u8(tree)` with `dispatcher_row adapter jet_builtins codec_decode_u8`. That is the existing row form (Core.rows:66-77, 455-462). The IR scalar arm keeps the other nine.
  - R3 closed: prerequisite four is named from q11 and q12, it is in the losses and the surface, and the cost is now two days [INFERENCE].
  - R4 closed within limits: the auto-derive paragraph is right, and q13 on batch16 confirms five of the named examples. Its "probe r1-u8" has no path (R9).
  - R5, R6, R7 and R8 closed. DecodeUnder is a role under every option. Gain 2 says "A's part of #6852 c8 and c9". MapKey is spelled out with `impl String.MapKey`, the bounds `impl [K: V]<K: MapKey, V: Decode>`, and q7's new Fix text. B's fixed-list sentence is corrected.
  - New: R9 (sourcing only).
  - `Validate.jet` (batch16) on a scratch copy of this revision, with the proposed `reviewPasses.adversarial` added, reports one gap: "posting payload must set draft:false".
- 2026-11-24, round 3 (R9 revision; the previous ballot is kept as `.before-r3`). R9 closed.
  - C cites `$JET_SCRATCH_ROOT/tmp/g1gen2/r1-u8_core_u8_autoderive.jet` on `$JET_SCRATCH_ROOT/tmp/orch/batch17-chain/s1/jet`, plus q13 and q10. I ran that probe on batch16 as well: its only error is "`Secret` isn't `Encode`" (line 29), which matches the ballot's statement that only `#!Codable` Secret is refused, and only for Encode.
  - `reviewPasses.adversarial` is byte-identical to the proposed text.
  - `Validate.jet` (batch16) on the written ballot reports one gap: "posting payload must set draft:false".
  - No new findings.

## 5. Overall verdict

Round 0: **fails** (16 findings). The recommendation (A) is the strongest of the four on the merits. It is aeson's and Haskell's proven mechanism, it keeps D-BUILTIN-METHOD1's no-overlap clause, and its dispatch already works at O0, O1 and O2 (q3). But the ballot missed half the mechanism's sites, misstated the state of #6852 c8, asserted a map head that the sibling ballot refuses, left out the language work A needs, and stated B and D below their strongest.

Round 1: **fails** (8 findings). The inventory, prerequisites, comparisons and cases are now sound. Still open:
- D′ cites a missing patch.
- U8's decode names the wrong door.
- The streaming hook needs generic trait members, which do not work today (q11, q12).
- C's Core migration ignores auto-derived codecs (q10).
- The key trait is a placeholder.
- The c8 gain is overclaimed.

Round 2: **holds within limits** once R9 is fixed. That fix is sourcing only and changes no claim.

Round 3 (final): **holds within limits**. R9 is fixed, and every claim holds or holds within these named limits:
- A's four language prerequisites (q1, q2, q11, q12; one card, about two days [INFERENCE]).
- The `[T]`/`[T#N]` instance-collision fix that every option shares (p4, p5, p9).
- The MapKey bound, which is the map-key rule named as new work in D-IMPL-STRUCTBOUND1's review.
- The tooling texts owed under every option (q5, q6, q7, q9).

| Claim | Round 0 | Round 1 | Round 2 |
|---|---|---|---|
| C1 situation | holds within limits | holds | holds |
| C2 inventory | fails | holds within limits (R5) | holds |
| C3 map head / key trait | fails | fails (R7) | holds within limits (MapKey bound is new work) |
| C4 F32/Dyn impls | fails | holds (seed a/b verified) | holds |
| C5 lesson | fails | holds | holds |
| C6 A dispatch | holds within limits | holds within limits (prerequisites named) | holds within limits (four prerequisites named) |
| C7 U8 decode | fails | fails (R2) | holds |
| C8 streaming hook | fails | fails (R3) | holds within limits (prerequisite four) |
| C9 encode hook | fails | holds | holds |
| C10 overriders, distinct | holds within limits | holds | holds |
| C11 defaults | holds within limits | holds within limits | holds within limits (prerequisite one) |
| C12 B | holds within limits | holds within limits (R8) | holds |
| C13 C | fails | fails (R4) | holds within limits (R9 sourcing; closed in round 3, holds) |
| C14 D | fails | fails (R1) | holds |
| C15 comparisons | fails | holds | holds |
| C16 gains | holds within limits | fails (R6) | holds |
| C17 cases | fails | holds | holds |
| C18 surface | holds within limits | holds within limits (R3 prerequisite) | holds |

Proposed `reviewPasses.adversarial`, which passes `Validate.jet` with only the draft gap left. The validator allows two sentences after the provenance prefix, each at most 32 words.

> Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: ConcreteHeadReview. The adversarial review attacked the recommendation. Probes p1-p9 and q1-q13 ran on batch16; findings F1-F16, R1-R8 and R9 are fixed or named, and A holds within its named prerequisites (q1, q2, q11, q12; p4, p5, p9).

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F2 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F3 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F4 | card | #6852 |
| F5 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F6 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F7 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F8 | card | #6852 |
| F9 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F10 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F11 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F12 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F13 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F14 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| F15 | card | #6852 |
| F16 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| R1 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| R2 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| R3 | card | #6852 |
| R4 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| R5 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| R6 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| R7 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| R8 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
| R9 | no-action | archived: fixed in the ballot text before posting (re-review rounds 1-3) |
<!-- /audit-dispositions -->
