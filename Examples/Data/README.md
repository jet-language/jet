# Data

JSON and other formats, binary data, dynamic values, databases and analysis.

Run a lesson from the repository root, for example `jet run Examples/Data/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [CodableAutoRules](CodableAutoRules.jet) — A plain struct or enum is Codable (Encode + Decode) when every stored field has a wire form; no marker is needed.
- [CodableDefault](CodableDefault.jet)
- [CodableDemand](CodableDemand.jet) — Derive on demand: every plain struct and enum here is Codable, but a generated codec body exists only for a type an Encode/Decode requirement reaches:…
- [DataEventVariants](DataEventVariants.jet) — The twelve canonical `core.encoding.DataEvent` variants, in catalog order.
- [DecodeErrorExit](DecodeErrorExit.jet) — An uncaught `[FieldError]` from a typed decode ends the program with exit 1.
- [DecodeMigration](DecodeMigration.jet) — One canonical typed decode result.
- [Encoding](Encoding.jet) — One unified serialization library.
- [EncodingBase](EncodingBase.jet) — Beginner whole-value base encodings.
- [EncodingBaseExpert](EncodingBaseExpert/) — Base64/base32 decoding is strict RFC 4648 by default; transport-shaped input is admitted only through a named allowance.
- [EncodingBreadth](EncodingBreadth.jet) — Format breadth over one value tree.
- [EncodingCanonical](EncodingCanonical.jet) — Canonical CBOR and XML use the encoding core directly.
- [EncodingStreamTypes](EncodingStreamTypes.jet) — Shared encoding values reachable from Jet.
- [HandCodec](HandCodec.jet) — Hand-written and derived codecs share one public protocol.
- [Migrations](Migrations/) — #PublishedSchema structs + migration blocks.
- [SerdeGeneric](SerdeGeneric.jet) — generic #Codable.
- [ShapeProjection](ShapeProjection.jet) — One shape fact feeds every projection.
- [UnionCodec](UnionCodec.jet) — An anonymous union encodes and decodes through its structural enum, alone and as a struct field, and a decoded union matches by member.
- [Validate](Validate.jet)

## Analysis

- [AnalyticsQuery](Analysis/AnalyticsQuery.jet)
- [DataAnalysis](Analysis/DataAnalysis.jet)
- [DataBridges](Analysis/DataBridges.jet)
- [DataCore](Analysis/DataCore.jet) — Typed Core data slice.
- [DataLoader](Analysis/DataLoader.jet)
- [DataPipeline](Analysis/DataPipeline.jet)
- [DataPlot](Analysis/DataPlot.jet)
- [TypedQuery](Analysis/TypedQuery.jet)

## Binary

- [BinaryCodecs](Binary/BinaryCodecs.jet)
- [BinaryPattern](Binary/BinaryPattern.jet) — Binary patterns — the byte-mode sibling of string patterns.
- [BinaryReader](Binary/BinaryReader.jet) — `binary.Reader` — the "shift" kernel from linear stream parsing (Jai's `shift` primitive), without a dedicated operator.

## Database

- [DB](Database/DB.jet) — core.db uses one typed SQL value.
- [DBCheckedSQL](Database/DBCheckedSQL.jet) — Checked SQL literals feed the single typed core.db path.
- [DBJSONBBoundary](Database/DBJSONBBoundary.jet) — SQLite JSONB stays an opaque value at the database boundary.
- [DBPolicy](Database/DBPolicy.jet) — One explicit scope carries policy and user identity.
- [DBPool](Database/DBPool.jet) — The bounded DB pool exposes acquire, readiness, receipt, and drain.
- [DBReadFootprint](Database/DBReadFootprint.jet) — core.db read/write effect leaves.

## Dyn

- [Arithmetic](Dyn/Arithmetic.jet) — Dyn arithmetic and comparisons are checked while running.
- [BytesDateTime](Dyn/BytesDateTime.jet) — Dyn.Bytes and Dyn.DateTime (owner direction, 2026-10-02): an instant with a fixed UTC offset, and raw bytes.
- [Codecs](Dyn/Codecs.jet) — Dyn.Bytes and Dyn.DateTime in every codec: CBOR has native byte strings and date-time tags (0 and 1), TOML has native offset date-times, YAML has `!!binary`…
- [DecodeBytes](Dyn/DecodeBytes.jet) — A Bytes payload decodes into `[U8]`, built in code or read from CBOR, and a decode the compiler runs at build time reports the same words as the running program.
- [DecodeKinds](Dyn/DecodeKinds.jet) — A decode failure names the Dyn variant it found, and every decoder uses the same word for it: the List and Map decodes, the scalar decodes, and the…
- [DynAccessors](Dyn/DynAccessors.jet)
- [Fields](Dyn/Fields.jet) — A Dyn object grows with dotted fields, like a Python object, and every read is checked while the program runs.
- [Indexing](Dyn/Indexing.jet) — Dyn reads with [key] and [i] like maps and lists, and writes in place on a mutable binding.
- [KindMismatch](Dyn/KindMismatch.jet) — Dyn arithmetic on mismatched kinds stops and names both.
- [MissingField](Dyn/MissingField.jet)
- [WrapAndDecode](Dyn/WrapAndDecode.jet) — Plain values go into a Dyn without wrappers, typed structs go in with Dyn.from, and decode<T> brings typed values back out.

## Formats

- [CBORProfile](Formats/CBORProfile.jet) — core.encoding.cbor profile: RFC 8949 CBOR projected into Dyn.
- [CSV](Formats/CSV.jet)
- [CSVShapes](Formats/CSVShapes.jet) — CSV shape edges: quoted delimiters, embedded newlines and doubled quotes stay inside one field; ragged rows, unterminated quotes, missing columns and…
- [CSVTyped](Formats/CSVTyped.jet)
- [EncodingEetf](Formats/EncodingEetf.jet) — Standalone Erlang External Term Format (version 131): the one-way JSON view and the typed codec.
- [EncodingXMLProjection](Formats/EncodingXMLProjection.jet) — Typed decode + focused tree helpers.
- [EncodingXMLStream](Formats/EncodingXMLStream.jet)
- [TOML](Formats/TOML.jet) — core.encoding.toml — full TOML 1.0: `[table]` headers, arrays, typed scalars.
- [XMLProfile](Formats/XMLProfile.jet) — core.encoding.xml profile: well-formed XML 1.0 data parsing.
- [YAML](Formats/YAML.jet) — core.encoding.yaml — full YAML 1.2 core: block + flow, sequences, typed scalars.

## JSON

- [EncodingJSONLStream](JSON/EncodingJSONLStream.jet)
- [EncodingJSONStream](JSON/EncodingJSONStream.jet)
- [JSON](JSON/JSON.jet)
- [JSONCanonical](JSON/JSONCanonical.jet) — json.canonical is RFC 8785 JCS: the exact bytes to hash or sign.
- [JSONDuplicateKeys](JSON/JSONDuplicateKeys.jet)
- [JSONIntegerFidelity](JSON/JSONIntegerFidelity.jet) — JSON keeps full integer values through dynamic and typed Codable paths, rejects trailing commas, and decodes unit enum wires.
- [JSONLRecords](JSON/JSONLRecords.jet) — JSONL record boundaries over text you already hold (`jsonl.parse`, `jsonl.first`): one complete JSON value per line, `\r\n` accepted, blank lines skipped.
- [JSONParseLimits](JSON/JSONParseLimits.jet) — json.parse_with_limits: the caller chooses the input-size and nesting bounds instead of EncodingLimits.safe() (16 MiB, depth 256).
- [JSONPatch](JSON/JSONPatch.jet) — json.patch: RFC 6902 JSON Patch applied atomically.
- [JSONPointer](JSON/JSONPointer.jet) — json.pointer: RFC 6901 selection over a parsed Dyn.
- [JSONSelective](JSON/JSONSelective.jet)
- [JSONTyped](JSON/JSONTyped.jet) — Typed JSON keeps Decimal and Int number tokens exact.
- [JSONTypedFile](JSON/JSONTypedFile.jet)
