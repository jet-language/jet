# Encoding decisions

Use this record when implementing, reviewing, or documenting Jet's encoding
contracts. It defines the shared `DataTree` and bounded reader/writer model,
then the selected wire formats and decoder policies. Executable names and
carriers are registered in [`Core.jet`](../../crates/jet-codegen/src/Prelude/Core.jet)
and sourced from [`Core/encoding`](../../Core/encoding/encoding.jet); the
shared Prelude ABI is in
[`EncodingTypes.rs`](../../crates/jet-codegen/src/Prelude/CoreLib/JetStd/EncodingTypes.rs)
and stream behavior is in
[`EncodingStream.rs`](../../crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingStream.rs).
Use the vocabulary in [Jet vocabulary](vocabulary.md). Conformance evidence
lives in [`tests/encoding_corpus.rs`](../../tests/encoding_corpus.rs),
[`tests/encoding_parity.rs`](../../tests/encoding_parity.rs), and the
encoding examples under [`Examples/features/serde`](../../Examples/features/serde/).

The rules below describe durable contracts and their reasons. Decision IDs are
citations; a narrower decision controls the public surface where it says so.

## Shared value and error model

Use one `DataTree` for untyped values. Its closed variants are `Null`, `Bool`,
`Int`, `Float`, `Text`, `Array`, and `Object`; objects preserve insertion
order, and integers and floats remain distinct. A codec must reject malformed
input instead of fabricating a value. Base64, base32, and hex are scalar byte
helpers, not `DataTree` adapters. This keeps one tree walker and one set of
format adapters while preserving the distinction between text serialization
and byte alphabets.

Use one adapter identity per format for whole-value operations and bounded
reader/writer operations. The streaming formats are JSON, JSONL, CSV, XML,
and CBOR. Reader/writer access is a mode of the same codec, not a second
library; scalar base encoders do not become readers or `DataTree` adapters.
This gives beginners a whole-value path and experts a bounded path without
duplicating parsers, trees, or error rules. (D-ENCSTREAM1=A)

Keep shared carriers in `core.encoding`: `EncodingLimits`, `EncodingError`,
`EncodingCause`, `EncodingFormat`, `EncodingErrorKind`, `DataEvent`, and
`DataTree`. Refer to them through `use core.encoding as encoding`. Keep
format handles and format-only options/events in
`core.encoding.json`, `core.encoding.jsonl`, `core.encoding.csv`,
`core.encoding.xml`, and `core.encoding.cbor`. `FileReader` and `FileWriter`
remain `core.files` types; do not add duplicate re-exports. (D-ENCSTREAM-SURFACE1=A)

`EncodingLimits` has these fields:

- `buffer_bytes`, `max_depth`, `max_item_bytes`,
  `max_total_bytes: ?Int`;
- `max_expansion_depth` and `max_expansion_bytes`.

`EncodingLimits.safe()` is `{buffer_bytes: 65536, max_depth: 256,
max_item_bytes: 16777216, max_total_bytes: None, max_expansion_depth: 32,
max_expansion_bytes: 8388608}`. Legal inclusive ranges are
`4096..16777216` for `buffer_bytes`, `1..4096` for `max_depth`,
`1..1073741824` for `max_item_bytes`, `None` or `0..Int.max` for
`max_total_bytes`, `0..256` for `max_expansion_depth`, and
`0..1073741824` for `max_expansion_bytes`. Validate fields in declaration
order before any I/O. Reject the first invalid field with kind `Limit`, byte
offset `0`, an empty path, and a reason naming the field, value, and range.
Expansion limits apply to XML entities; other codecs retain the fields but do
not consume those budgets, so one limits type serves every adapter.

`max_total_bytes` counts wire bytes. `max_item_bytes` counts decoded bytes
retained for one scalar, record, key, or canonical-object-sort buffer. At
every allocator observation, codec-owned live heap is at most

```text
buffer_bytes + max_item_bytes + max_expansion_bytes
    + (256 * max_depth) + 65536 bytes
```

Codec-owned memory includes parser/writer state, wire buffers, the current
partially decoded item, canonical object-sort storage, and entity-expansion
storage. It excludes consumed file-handle storage and values already returned
to caller ownership. Counting-allocator tests cover adversarial nesting,
items, and entities at every legal limit endpoint. (D-ENCSTREAM-SURFACE1=A)

`EncodingError` contains `format: EncodingFormat`, `kind: EncodingErrorKind`,
`byte_offset: Int`, `line: ?Int`, `column: ?Int`, `path: String`,
`reason: String`, and `cause: ?EncodingCause`. `EncodingCause` is a `Clone`
+ `Eq` snapshot `{kind: String, os_code: ?Int, message: String}`. It is not
`IOError` and carries no host handle. `EncodingError` and `EncodingCause`
derive `Clone` and `Eq`; `cause` is `Val` only for kind `IO`, otherwise
`None`, and `cause()` returns that snapshot.

Display exactly as

```text
<Format> <Kind> at byte <offset>[, line <line>, column <column>][, path <path>]: <reason>
```

Omit absent clauses, and keep cause text separate. `byte_offset` is a
zero-based wire-byte offset; textual line and column are one-based; `path` is
the best-known `DataTree` path or empty. Store the first terminal error and
return an equal clone from every later method. Return clean EOF as stable
`None` only after structural validation and trailing-input checks. Runtime
parse failures are Core values, not compiler diagnostics; wrong methods or
argument types reuse existing Core type diagnostics. A new compiler
diagnostic still requires `diagnostics.md` and a UI snapshot under I4.
(I4; D-ENCSTREAM-SURFACE1=A)

<a id="d-encstream-surface1--public-streaming-encoding-surface"></a>
## Bounded reader and writer lifecycle

Use synchronous blocking calls for backpressure. `next` reads only until one
item or a terminal state. `write` returns only after accepting the item within
the bound and flushing underlying bytes when needed. No thread, task, channel,
callback, hidden queue, `WouldBlock`, or partial-success status exists. `flush`
pushes bytes but does not validate closure. `finish` validates closure, flushes,
is required for successful output, and is idempotent after success. A `write`
after `finish` returns kind `State`; dropping a handle closes it, and dropping
unfinished output never claims success. See the
[Bounded buffering law](spec.md#bounded-buffering-law) for the
cross-primitive classification. (D-ENCSTREAM-SURFACE1=A)

Expose codec-native opaque handles. JSON/JSONL/CSV/CBOR reader and writer
structs have private state and derive none of `Codable`, `Copy`, or `Clone`;
XML uses the exact tagged `DataTree` schema in the XML section. Every reader
has the current-signature form

```jet
pub fn next(&self) -> Item? encoding.EncodingError!
```

Every writer has

```jet
pub fn write(&self, item: Item) encoding.EncodingError!
pub fn flush(&self) encoding.EncodingError!
pub fn finish(&self) encoding.EncodingError!
```

`&self` is the edit access because each call advances state; callers keep a
handle in a changeable binding. Constructors consume file handles with `^`:

```jet
reader(input: ^files.FileReader,
       limits: encoding.EncodingLimits{encoding.EncodingLimits.safe()})
    -> Reader encoding.EncodingError!
writer(output: ^files.FileWriter,
       limits: encoding.EncodingLimits{encoding.EncodingLimits.safe()})
    -> Writer encoding.EncodingError!
```

`json.writer` alone adds `canonical: Bool{false}`. Invalid limits or setup I/O
return `EncodingError`; the consumed handle closes by RAII, so a failed
constructor neither leaks nor returns it. (D-MEM1; D-ENCSTREAM-SURFACE1=A)

The format-specific item rules are these:

- JSON reader/writer uses `DataEvent` and accepts exactly one root. CBOR uses
  the same event algebra and rejects tags, non-text map keys, bignums, and
  values outside `DataTree` as `Unsupported`, without coercion. The event
  variants are `Null`, `Bool(Bool)`, `Int(Int)`, `Float(Float)`,
  `Text(String)`, `Bytes([U8])`, `ArrayStart`, `ArrayEnd`, `ObjectStart`,
  `Key(String)`, and `ObjectEnd`.

The Prelude carrier may also carry `Number(String)` for exact-token JSON
decoding; this preserves number text without adding a `DataTree` variant.
The event list above remains the public reader/writer algebra for this
decision.

- A JSON writer rejects `DataEvent.Bytes` with kind `Unsupported` and reason
  `JSON cannot encode Bytes; encode bytes as Text explicitly`. It rejects a
  `Float` whose value is NaN or positive or negative infinity with kind
  `Unsupported` and reason `JSON cannot encode a non-finite Float`. Validate
  an event before accepting or emitting its bytes; earlier accepted events may
  already have flushed. Canonical and noncanonical JSON use this same
  rejection rule. Canonical JSON buffers and sorts each object within
  `max_item_bytes`.
- JSONL readers return `?DataTree`, writers accept `DataTree`, and each
  non-empty record contains one complete value.
- CSV readers return `?CSVRow`, writers accept `[String]`, and each record
  follows RFC 4180, including quoted newlines. `CSVRow` has `fields: [String]`
  and the one-based physical opening `line`. Delimiter, `header`, and
  `skip_blank` options have the same defaults and semantics as
  `core.encoding.csv.rows`.
- XML readers and writers use the XML event/node algebra, item type,
  lexical-preservation law, expanded names, parse/render options, and
  field-by-field `XMLError` projection below. Safe parse options never open
  external identifiers, expand only an explicit in-memory map, and charge
  shared expansion budgets. Chunk boundaries cannot change events or errors.
  Collecting events reconstructs the structurally equal whole tree; lexical
  evidence belongs only to the XML fields below.
```jet
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() (EncodingError | IOError)! {
    input :: files.open("catalog.json") ?? return
    reader :: json.reader(^input, limits: encoding.EncodingLimits.safe()) ?? return
    loop event in reader {
        if event == .Key("item") { print("item") }
    }
}
```


The public names are new, while `json.events(DataTree)` remains the existing
String-path transcript. Pull events exist only through `json.reader` and use
`DataEvent`; renaming `json.events` or changing its return type requires an
edition-migration decision with source rewrite, a deprecation window, and
old-edition behavior. A return-type-only overload is not allowed because I8
requires one unambiguous call. Reader and writer types are non-Codable state
handles and cannot be copied. `EncodingFormat` is exhaustive in v1: adding a
format variant is source-breaking for exhaustive matches and requires an
edition-migration decision plus a generated rewrite, while adding
format-specific handles alone does not change the enum. `DataEvent` changes
only when an owner decision changes `DataTree`. The beginner path remains
whole-value calls; the expert path makes ownership, event types, offsets,
namespace/entity rules, limits, flush/finish, deterministic output, and
backpressure explicit; both paths share one parser, tree, error law, and event
algebra. (I8; D-ENCSTREAM-SURFACE1=A)

## XML representation and security

Represent XML as one lossless namespace-aware ordinary `DataTree`, not an
`XmlDocument` or `XMLEvent` type. Use XML 1.0 Fifth Edition and Namespaces in
XML 1.0 as the floor. `parse(String)` accepts Unicode text whose declaration
is absent or names UTF-8; its identity guarantee is `String` equality, not
original file bytes. `parse_bytes([U8])` recognizes UTF-8, UTF-8 BOM,
UTF-16LE BOM, and UTF-16BE BOM, validates an XML declaration against the
detected encoding, and rejects every other encoding with `XMLError`.

Preserve an unchanged source byte sequence from `to_bytes` when every token is
unchanged and the requested encoding/BOM equals the detected input
encoding/BOM. If that pair still matches but tokens changed, reuse each
unchanged valid raw-byte token independently and render each changed or
constructed token in the same encoding; do not buffer a subtree or whole
document. If the output encoding/BOM differs, reuse no raw byte token and
transcode every token consistently. Forbid mixed-encoding concatenation. A
parser never opens files, URLs, sockets, catalogs, external subsets, or
system/public identifiers. (D-ENCXML1=A)

Validate and decode predefined and numeric references. Declared general
references default to `Preserve`; `Reject` refuses them; `Resolve(values:
[String:String])` expands only names in the explicit in-memory map as
character data. Never reparse replacement strings as XML markup: `<`, `>`,
`&`, quotes, and entity-looking text stay characters and render escaped.
Internal declaration replacement text remains inert preservation data.
Parameter entities, external parsed entities, and replacement-text markup
expansion are `Unsupported`; cycles and limits still reject. External
identifiers remain inert data.

`xml.XMLParseOptions` owns entity policy and `xml.XMLLimits` owns
`max_depth`, `max_nodes`, `max_attributes_per_element`, `max_name_bytes`,
`max_text_bytes`, `max_entity_declarations`, `max_entity_depth`, and
`max_entity_replacement_bytes`. `safe()` supplies the versioned defaults.
Exceeding a limit is an error, never truncation. (D-ENCXML1=A)

Use `XMLName{raw, prefix, local, namespace_uri}` and ordered
`XMLNamespace{prefix, namespace_uri, quote, lexical}`. Default namespaces
apply to elements, never unprefixed attributes. Validate `xml`/`xmlns`
bindings and duplicate expanded attributes. A Clark key is `local` for no
namespace and `{namespace_uri}local` otherwise. Codable projection uses child
Clark keys and attribute keys prefixed with `@`.

An element has simple content exactly when every child is text, CDATA, or a
resolved `entity_ref`; concatenate decoded character values into `$text`
(`$text: ""` for empty content) and omit `$content`. Otherwise it has mixed
content: omit `$text` and set `$content` to the exact `DataTree.Array` of every
tagged child in encounter order, including text-like nodes. Repeated child
Clark keys become arrays in encounter order only in ordinary child-key
projection; never regroup `$content`. XML names cannot begin with `@`, `$`, or
`{`, so control keys do not collide. Existing `#Rename("key")` and
`#DenyUnknownFields` apply. (D-ENCXML1=A)

`XMLError` is exactly

```text
XMLError{
    kind: XMLReason,
    byte_offset: ?Int,
    line: ?Int,
    column: ?Int,
    path: String,
    reason: String,
}
```

`XMLReason` is the closed enum `InvalidEncoding`, `Malformed`,
`MismatchedTag`, `InvalidName`, `Namespace`, `DuplicateAttribute`, `Entity`,
`EntityCycle`, `Limit`, `Canonicalization`, `Shape`, and `Unsupported`.
For source-backed errors, `byte_offset` is the zero-based original offset for
`parse_bytes` or UTF-8 offset for `parse`; line and column are one-based
Unicode-scalar positions. Constructed/source-less validation, shape,
rendering, and canonicalization errors use `None` for all three locations,
not a colliding numeric sentinel. `path` remains the best-known Clark-name or
index path. `parse`, `parse_bytes`, `parse_with`, `canonical`, `decode`, and
event folding return `Result`; they do not emit diagnostics or partial trees.
Diagnostic codes and CLI rendering are downstream gates. (D-ENCXML1=A)

Keep lexical evidence token-local. `XMLLexical` is the ordinary `DataTree`
object `{raw_text: Text|Null, raw_bytes: Array<Int>|Null, semantic: DataTree}`.
Validate every `raw_bytes` integer in `0..255`; parsed input has exactly one
non-`Null` raw field, while constructed tokens have both raw fields `Null`.
`semantic` is the lexical-free semantic value of that token. Element nodes
carry `open_lexical` and `close_lexical` separately; empty-element syntax has
both fields `Null`. Document whitespace, text, CDATA, comment, processing
instruction, declaration, doctype, entity reference, attribute, and namespace
nodes carry only their own token lexical evidence.

A token slice is valid only when its current lexical-free token value deeply
equals `semantic`. Editing a token invalidates only that token; a parent has no
subtree raw slice. `to_string` may concatenate valid `raw_text` tokens with
recursively rendered Unicode children. `to_bytes` may reuse each valid
`raw_bytes` token independently when selected encoding/BOM matches the source;
a changed token is rendered in that encoding while neighboring valid tokens
remain reusable. If selected encoding/BOM differs, render/transcode every
token and reuse no raw bytes. An `element_start` event can therefore emit
complete opening-token evidence immediately and `element_end` can emit
complete closing-token evidence without subtree buffering. Ignore false
snapshots and raw slices; untouched parse-render identity remains exact.
(D-ENCXML1=A)

Canonicalize the resolved semantic infoset, never lexical slices. `xml.XMLCanonical`
configures `xml.canonical`; `Inclusive11` means W3C Canonical XML 1.1 and
`Exclusive10` means W3C Exclusive XML Canonicalization 1.0. `comments` selects
the standard with-comments or without-comments form; `inclusive_prefixes` is
legal only for `Exclusive10`. Canonicalize the whole document, not arbitrary
node sets or XML Signature transforms. Emit UTF-8 with LF line endings and no
BOM, XML declaration, or DOCTYPE. Expand empty elements; turn entity and
character references into characters; sort namespace declarations and
attributes exactly by the selected W3C algorithm; and follow that algorithm's
escaping and whitespace normalization. An unresolved entity, a relative
namespace URI forbidden by the selected standard, or a non-document root
returns `XMLError`. (D-ENCXML1=A)

The beginner path uses `xml.parse`, `xml.to_string`, `xml.decode<T>`, safe
limits, preserved entities, and ordinary local names without namespace or DTD
ceremony. The expert path audits expanded names, exact byte encoding, lexical
evidence, explicit entity resolution, limits, events, C14N mode, comments, and
inclusive prefixes. Helpers and Codable remain views over the same tagged
tree and events, not another tree. (D-ENCXML1=A)


## XML nodes, events, limits, and byte reuse

Use these reader events in document order:
`document_start`, `declaration`, `document_whitespace`, `doctype`,
`element_start`, `text`, `cdata`, `entity_ref`, `comment`,
`processing_instruction`, `element_end`, and `document_end`.
`element_start` carries the name, ordered namespaces, ordered attributes,
`empty_style`, and opening-token lexical evidence only. `element_end` carries
the name and closing-token lexical evidence only. Document whitespace and leaf
events carry only their token-local fields. Events preserve order without
subtree buffering. Folding a complete sequence yields the exact whole-tree
semantic `DataTree`; unfolding that tree yields the same semantic events. Raw
chunks carry the same `XMLLexical` evidence and limits; no separate event
representation exists.

The reader and writer use current Jet signatures:

```jet
xml.XMLReader.next(&self) -> encoding.DataTree? encoding.EncodingError!
xml.XMLWriter.write(&self, item: encoding.DataTree) encoding.EncodingError!
```

The reader returns events in source order. The writer validates one complete
event before accepting it, enforces document/declaration/doctype/element
state, and emits no bytes for a rejected item; `flush` and `finish` follow the
shared lifecycle law. Folding starts at `document_start`, turns declaration,
doctype, and leaf events into their `$xml` node forms, nests
`element_start` through its matching `element_end` (or closes an empty event
immediately), and finishes only at `document_end`, yielding the whole-value
document object. Unfolding performs the inverse key-for-key projection. For
every valid whole tree, `fold(unfold(tree))` is deep-equal including order and
lexical evidence; for every valid complete event sequence,
`unfold(fold(events))` is event-for-event deep-equal. Invalid order, duplicate
document events, post-end input, mismatched end names, or incomplete finish
returns `State` for writer state misuse and `Syntax` or `Truncated` for reader
wire failures under the shared projection law. (D-ENCSTREAM-SURFACE1=A;
D-ENCXML1=A)

Project XML reader/writer failures field by field. Reader/writer I/O bypasses
`XMLReason` and produces `EncodingErrorKind.IO` with a populated handle-free
`EncodingCause`; every `XMLReason` projection has `cause: None`.
`InvalidEncoding` maps to `Syntax`. `Malformed` maps to `Truncated` only when
clean underlying EOF occurs before required XML closure; otherwise it maps to
`Syntax`. `MismatchedTag`, `InvalidName`, `Namespace`, `DuplicateAttribute`,
`Entity`, and `Shape` map to `Syntax`; `EntityCycle` and `Limit` map to
`Limit`; `Canonicalization` and `Unsupported` map to `Unsupported`. The
projected format is XML, and path and reason copy unchanged. A present
`XMLError.byte_offset` copies unchanged; an absent offset maps to shared
`EncodingError.byte_offset` zero while line and column remain `None`; present
line and column copy unchanged. (D-ENCXML1=A; D-ENCSTREAM-SURFACE1=A)


Public XML values use only `encoding.DataTree` variants `Null`, `Bool`, `Int`,
`Text`, `Array`, and `Object`. Encode byte sequences as `DataTree.Array<Int>`
with each integer in `0..255`; do not add a `DataTree.Bytes` variant. Every
object has exactly its listed keys, every listed key is required even when its
value is `Null`, and unknown or missing keys reject. `XMLName` is
`{raw:Text,prefix:Text|Null,local:Text,namespace_uri:Text|Null}`.
`XMLLexical` is
`{raw_text:Text|Null,raw_bytes:DataTree.Array<Int>|Null,semantic:DataTree}`.
`$xml` is text with these closed values: `document`,
`document_whitespace`, `declaration`, `doctype`, `element`, `namespace`,
`attribute`, `text`, `cdata`, `entity_ref`, `comment`, and
`processing_instruction`.

The complete node schemas are:

```text
 document {$xml:"document",encoding:Text|Null,bom:DataTree.Array<Int>,children:DataTree.Array<DataTree>}
 document_whitespace {$xml:"document_whitespace",value:Text,lexical:XMLLexical}
 declaration {$xml:"declaration",version:Text,encoding:Text|Null,standalone:Bool|Null,lexical:XMLLexical}
 doctype {$xml:"doctype",name:Text,public_id:Text|Null,system_id:Text|Null,internal_subset:Text|Null,lexical:XMLLexical}
 element {$xml:"element",name:XMLName,namespaces:DataTree.Array<DataTree>,attributes:DataTree.Array<DataTree>,children:DataTree.Array<DataTree>,empty_style:Text,open_lexical:XMLLexical,close_lexical:XMLLexical|Null}
 namespace {$xml:"namespace",prefix:Text|Null,namespace_uri:Text,quote:Text,lexical:XMLLexical}
 attribute {$xml:"attribute",name:XMLName,parts:DataTree.Array<DataTree>,normalized_value:Text|Null,quote:Text,lexical:XMLLexical}
 text {$xml:"text",value:Text,lexical:XMLLexical}
 cdata {$xml:"cdata",value:Text,lexical:XMLLexical}
 entity_ref {$xml:"entity_ref",name:Text,resolved_value:Text|Null,lexical:XMLLexical}
 comment {$xml:"comment",value:Text,lexical:XMLLexical}
 processing_instruction {$xml:"processing_instruction",target:Text,value:Text,lexical:XMLLexical}
```

Document children preserve every token in order. They may contain document
whitespace, comments, or processing instructions before or after the root;
they have at most one declaration before every other child, except leading
document whitespace is forbidden before a declaration; at most one doctype
follows the declaration and precedes the root; and exactly one root element
exists. No other node tag is legal there. Document whitespace is legal only as
a document child outside the root, and its value contains one or more XML S
scalars from U+0020, U+0009, U+000D, and U+000A. It preserves prolog and
epilog spacing and trailing newlines. Element `namespaces` contains only
namespace nodes, `attributes` only attribute nodes, and `children` only
element, text, CDATA, entity-reference, comment, and processing-instruction
nodes. `empty_style` is exactly `"empty"` or `"explicit"`: an empty element
has no children, `close_lexical: Null`, and one empty-element opening token;
an explicit element has non-`Null` `close_lexical`. `quote` is exactly
`"single"` or `"double"`. Attribute parts contain only text or entity-reference
nodes. `normalized_value` is `Null` exactly when any entity-reference part is
unresolved; otherwise it is the concatenation of text values and resolved
entity values after XML 1.0 attribute whitespace normalization. A `Null`
namespace prefix denotes the default declaration. Validate XML names,
namespaces, document order, comments, CDATA, processing instructions,
doctypes, entities, encoding/BOM, and limits according to the security law;
reject rather than repair. (D-ENCXML1=A)

This nested shape makes parent/child inspection and Codable projection direct
while preserving order and token-local evidence without subtree buffering.
The lossy `{name, attrs, children, text}` shape is not accepted; no
compatibility alias retains it, and no compiler dependency or external crate
is part of this contract. The node algebra, helper signatures, ownership and
backpressure, diagnostic projection, conformance corpus, canonicalization
vectors, and manual-tree migration use the laws above. (D-ENCXML1=A)


For a parsed token, exactly one of `raw_text` and `raw_bytes` is non-`Null`;
for a constructed token, both are `Null`. `raw_bytes` members are integers
`0..255`. `semantic` is deeply equal to the exact lexical-free payload:
declaration, doctype, namespace, attribute, document whitespace, and leaves
use their object without lexical evidence; an element opening uses
`{name,namespaces,attributes,empty_style}`; an element closing uses `{name}`;
and document start uses `{encoding,bom}`. Remove nested lexical fields
recursively before comparison. A malformed lexical object, two raw forms,
an out-of-range byte, a wrong semantic key/type, or an unequal snapshot is not
trusted: deterministic rendering proceeds from semantic fields. Canonical
output always ignores raw forms, so a stale raw value cannot override an edit.
(D-ENCXML1=A)

The public event item is exactly `encoding.DataTree`; do not add an `XMLEvent`
type or a `DataTree` variant. Every event is an object with exactly its listed
required keys and no extras. `$xml_event` has these closed values and payloads:

```text
 document_start {$xml_event:"document_start",encoding:Text|Null,bom:DataTree.Array<Int>}
 document_whitespace {$xml_event:"document_whitespace",value:Text,lexical:XMLLexical}
 declaration {$xml_event:"declaration",version:Text,encoding:Text|Null,standalone:Bool|Null,lexical:XMLLexical}
 doctype {$xml_event:"doctype",name:Text,public_id:Text|Null,system_id:Text|Null,internal_subset:Text|Null,lexical:XMLLexical}
 element_start {$xml_event:"element_start",name:XMLName,namespaces:DataTree.Array<DataTree>,attributes:DataTree.Array<DataTree>,empty_style:Text,open_lexical:XMLLexical}
 text {$xml_event:"text",value:Text,lexical:XMLLexical}
 cdata {$xml_event:"cdata",value:Text,lexical:XMLLexical}
 entity_ref {$xml_event:"entity_ref",name:Text,resolved_value:Text|Null,lexical:XMLLexical}
 comment {$xml_event:"comment",value:Text,lexical:XMLLexical}
 processing_instruction {$xml_event:"processing_instruction",target:Text,value:Text,lexical:XMLLexical}
 element_end {$xml_event:"element_end",name:XMLName,close_lexical:XMLLexical}
 document_end {$xml_event:"document_end"}
```

Namespace/attribute arrays and enum values obey the same closed node laws. An
empty element emits `element_start` with `empty_style: "empty"` and no
`element_end`; explicit syntax emits a matching `element_end`. The writer
accepts exactly one `document_start`; an optional declaration; an optional
doctype; one root with a LIFO stack of expanded names; XML-legal document
whitespace, comments, and processing instructions; matching explicit ends;
and one `document_end` only after the stack closes. It then rejects every
further item. Reject wrong keys, types, or tags; illegal child classes;
duplicate or out-of-order declaration, doctype, root, or end events;
empty-style ends; mismatched expanded names; post-end items; and `finish`
before `document_end`. Object-shape/content errors are `Syntax`; call-order
after terminal or finish is `State`. Folding and unfolding replace
`$xml_event` with the corresponding `$xml` form and preserve order and
lexical evidence. (D-ENCXML1=A)

`XMLLimits` is exactly

```text
{max_depth:Int,max_nodes:Int,max_attributes_per_element:Int,
 max_name_bytes:Int,max_text_bytes:Int,max_entity_declarations:Int,
 max_entity_depth:Int,max_entity_replacement_bytes:Int}
```

`XMLLimits.safe()` is
`{max_depth:256,max_nodes:1000000,max_attributes_per_element:1024,
max_name_bytes:4096,max_text_bytes:16777216,max_entity_declarations:1024,
max_entity_depth:32,max_entity_replacement_bytes:8388608}`. Legal inclusive
ranges, in declaration order, are `1..4096`, `1..1000000000`, `0..1000000`,
`1..1048576`, `0..1073741824`, `0..1000000`, `0..256`, and
`0..1073741824`. Require `max_entity_depth <= max_depth` and
`max_entity_replacement_bytes <= max_text_bytes`. Validate declaration-order
fields and then the first failing cross-field pair before I/O. Count decoded
UTF-8 bytes for byte limits, and count nodes, attributes, and entities as
exact nonnegative integers. Use arbitrary-precision nonnegative arithmetic
for counters and prospective additions; a crossing returns `Limit` before
retaining the item, allocating, or truncating. (D-ENCXML1=A)

Use these reader and writer constructors:

```jet
xml.reader(input: ^files.FileReader,
           limits: encoding.EncodingLimits{encoding.EncodingLimits.safe()},
           xml: xml.XMLParseOptions{xml.XMLParseOptions.safe()})
    -> xml.XMLReader encoding.EncodingError!
xml.writer(output: ^files.FileWriter,
           limits: encoding.EncodingLimits{encoding.EncodingLimits.safe()},
           xml: xml.XMLRenderOptions{xml.XMLRenderOptions.safe()})
    -> xml.XMLWriter encoding.EncodingError!
```

`xml.XMLParseOptions` is
`{entities: xml.XMLEntityPolicy = .Preserve,
limits: xml.XMLLimits{xml.XMLLimits.safe()}}`. `XMLEntityPolicy` is
`.Preserve`, `.Reject`, or `.Resolve([String:String])`. `xml.XMLRenderOptions`
is `{encoding: xml.XMLEncoding = .UTF8,
lexical: xml.XMLLexicalPolicy = .PreserveValid}`. `XMLEncoding` is `.UTF8`,
`.UTF8BOM`, `.UTF16LE`, or `.UTF16BE`; `XMLLexicalPolicy` is `.PreserveValid`
or `.Deterministic`. Canonicalization remains the separate
`xml.canonical(..., xml.XMLCanonical)` operation, not a writer option.
Validate shared `EncodingLimits` in field order, then XML limits in their
field order, then entity-map keys/values or render enums. The first failure
wins and projects through `EncodingError`. (D-ENCSTREAM-SURFACE1=A;
D-ENCXML1=A)

Both limit objects remain active; neither silently overrides the other. The
effective depth ceiling is
`min(encoding.max_depth, xml.max_depth)`. Wire buffering and total input use
`encoding.buffer_bytes` and `max_total_bytes`. One retained event/tree item
and canonical-sort storage use `encoding.max_item_bytes`. Element, node, name,
text, attribute, and declaration counts use their XML fields. Entity expansion
uses
`min(encoding.max_expansion_depth, xml.max_entity_depth)` and
`min(encoding.max_expansion_bytes, xml.max_entity_replacement_bytes)`;
`max_entity_declarations` remains XML-only. Crossing either contributing
bound reports `Limit` naming the actual bound first crossed. The allocator
ceiling is the shared encoding ceiling; XML limits may reduce work but never
enlarge it. Whole-value `parse_with` applies XML limits alone because it has
no reader `EncodingLimits`; a reader applies both.

Reader emits one `document_whitespace` event per maximal contiguous XML S
sequence outside the root. Do not coalesce across a comment, processing
instruction, declaration, doctype, or root boundary. Fold maps it key-for-key
to the node and unfold maps each node back one-for-one, preserving prolog and
epilog spaces, CR/LF spelling, and a final newline. Reject it inside an
element, before a declaration, or after `document_end`. `to_string` and
`to_bytes` render its value exactly when lexical evidence is valid; otherwise
render the validated XML S value. Canonicalization discards document
whitespace outside the document element as required by the selected C14N
standard. (D-ENCXML1=A)

`parse_bytes` records one source encoding and BOM in the document's
`encoding`/`bom`. The renderer chooses reuse from those two bounded facts and
each token snapshot; it never scans or buffers the whole document. Reuse is
allowed only when `XMLRenderOptions` encoding/BOM exactly matches the source.
A valid unchanged token writes raw bytes directly; a changed or constructed
token writes deterministic bytes in the selected encoding, with memory bounded
by the active reader/writer buffer plus one token/item. If encoding/BOM differs,
transcode every token and ignore raw bytes. A declaration with `encoding: Null`
is legal for every supported encoding/BOM. A non-`Null` encoding must
case-insensitively name the selected family (UTF-8 or UTF-16); deterministic
rendering uses canonical `UTF-8` or `UTF-16`, and a conflict returns
`InvalidEncoding` before any bytes, even with raw evidence. Untouched
`parse_bytes` plus matching render options is byte-for-byte identical,
including document whitespace; token-local edits preserve unchanged-token
bytes without whole-document buffering. (D-ENCXML1=A)

`max_nodes` counts each accepted `$xml` object in the folded tree: document,
declaration, document whitespace, doctype, element, namespace, attribute,
text, CDATA, entity reference, comment, and processing instruction.
`XMLName` and `XMLLexical` helper objects are not nodes. In streaming,
`document_start` accounts for the document node; declaration, document
whitespace, doctype, `element_start`, and leaf events account for their node
plus namespaces and attributes carried by `element_start`; `element_end` and
`document_end` add zero. Check each prospective increment before retaining or
emitting the event.

`max_depth` counts element nesting only: document is depth 0, the root is
depth 1, each child is parent depth plus one, and non-elements do not change
depth. `max_attributes_per_element` counts attribute nodes and excludes
namespace declarations. `max_name_bytes` applies separately to the UTF-8
length of every non-`Null` `XMLName` `raw`, `prefix`, `local`, and
`namespace_uri`; declaration version/encoding, doctype name/public/system
identifiers, namespace prefix/URI, entity name, and processing-instruction
target each receive their own check. No concatenated or normalized form may
bypass a component check.

`max_text_bytes` is both a per-field and whole-document ceiling. Count UTF-8
bytes for document whitespace; text, CDATA, and comment values; processing-
instruction values; doctype internal subsets; attribute text parts and
resolved entity values; and every other free-text payload not governed by
`max_name_bytes`. Do not double-count `normalized_value`, lexical
raw/snapshot fields, or a resolved value repeated in `normalized_value`. Each
field must fit the limit, and arbitrary-precision cumulative document bytes
must also fit.

`max_entity_declarations` counts general and parameter declarations in the
internal subset, even when policy later rejects use. `max_entity_depth` counts
the root replacement at depth 1 and each recursive replacement at its parent
depth plus one; preserved or unresolved references consume depth 0.
`max_entity_replacement_bytes` is both per-replacement and cumulative: each
decoded replacement length and the arbitrary-precision sum of materialized
replacement bytes in one parse must fit. Charge repeated expansion each time
it is materialized; do not charge rejected or preserved inert text. Every
counter uses prospective arbitrary-precision addition and returns `Limit`
before allocation or retention on a crossing. (D-ENCXML1=A)

## XML helper view

Expose focused helpers over the closed XML tree rather than a second document
model. The exact helper surface is:

```jet
decode<T: Codable>(text: String,
                   options: XMLParseOptions{XMLParseOptions.safe()})
    -> T [FieldError]!
decode_bytes<T: Codable>(bytes: [U8],
                         options: XMLParseOptions{XMLParseOptions.safe()})
    -> T [FieldError]!
root(document: DataTree) -> DataTree XMLError!
expanded_name(node: DataTree)
    -> (raw: String, prefix: String?, local: String, namespace_uri: String?) XMLError!
attribute(element: DataTree, name: String) -> String? XMLError!
content(element: DataTree) -> [DataTree] XMLError!
```

Select attributes by local name or Clark name, never by prefix. `attribute`
returns `normalized_value`; a found attribute with an unresolved entity returns
`XMLError` kind `Entity`. `content` returns exact child nodes in source order.
Keep `XMLError` and source locations in these helpers. Typed decode projects
parse, projection, and Codable shape failures into one `[FieldError]` list;
the root path is empty and nested paths use the same field/index segments as
every other codec. Do not add another query or wrapper type. (D-ENCXML-PROJECTION1=A)

```jet
use core.encoding.xml as xml

#Codable
struct Catalog { book: [Book] }
#Codable
struct Book {
    #Rename("@id") id: String
    title: String
}

fn run() (XMLError | [FieldError])! {
    source :: "<catalog><book id=\"7\"><title>Hi</title></book></catalog>"
    document :: xml.parse(source) ?? return
    root :: xml.root(document) ?? return
    name :: xml.expanded_name(root) ?? return
    print(name.local)
    print((xml.attribute(root, "id")) ?? "missing")
    children :: xml.content(root) ?? return
    loop child in children {
        child_name :: xml.expanded_name(child) ?? return
        print(child_name.local)
    }
    catalog :: xml.decode<Catalog>(source) ?? return
    wire :: xml.to_bytes(document, xml.XMLRenderOptions.safe()) ?? return
    copy :: xml.decode_bytes<Catalog>(wire) ?? return
    print(catalog.book.len())
    print(copy.book[0].title)
}
```

The node sequence remains ordered and lossless, including document type,
comments, CDATA, and entity references:

```jet
use core.encoding.xml as xml

fn inspect() (XMLError | [FieldError])! {
    source :: "<!DOCTYPE p [<!ENTITY legal 'ok'>]><p a='1'>hi <b>x</b><!--c--><![CDATA[<&]]>&legal;</p>"
    document :: xml.parse(source) ?? return
    root :: xml.root(document) ?? return
    name :: xml.expanded_name(root) ?? return
    print(name.local)
    print((xml.attribute(root, "a")) ?? "missing")
    children :: xml.content(root) ?? return
    print(children.len())
}
```

The child order in `children` is `text`, `element`, `comment`, `cdata`,
`entity_ref`; the first two printed values are `p` and `1`, and the child
count is `5`.


## JSON numbers and canonical output

Keep one JSON number tokenizer for dynamic and typed decoding. Untyped
`json.parse` maps integral numbers to exact `Int` and fractional numbers to
`Float`. Typed `json.decode<T>` and `data.json<T>` retain each valid number
token until the target consumes it. `Decimal` parses the token directly,
preserving sign, exponent, and lexical scale (`12.340` remains `12.340`).
The exact integer destination is arbitrary-precision `Int` under D-INTBIG1 and
D-TYPE2-NUM1; `JetBigInt` is internal storage, not a user-facing decode
target. Exact `Int` rejects non-integral fractions. Neither target falls back
to quoted text or round-trips through binary64.

Whole-value and stream decoding share the tokenizer, projection, limits, and
field-error vocabulary. Invalid JSON, non-finite input, target mismatch,
fixed-width overflow, and digit/exponent-limit failures remain ordinary JSON
or `[FieldError]` values. Canonical output uses the separate JSON
canonicalization law; this decision adds no second writer. (D-JSON-EXACTNUM1=A;
D-INTBIG1; D-TYPE2-NUM1)

Use RFC 8785 JSON Canonicalization Scheme for hashing and signing. It must not
depend on incidental renderer behavior: `json.canonical` is the JCS contract,
while `json.to_string` and `json.to_string_pretty` remain ordinary renderers
and are not hashing contracts. Canonicalization is recursive, emits UTF-8
without BOM or trailing LF, emits no insignificant whitespace, preserves array
order, rejects duplicate object keys, never normalizes Unicode, never coerces
Bytes, numbers, or text, and never emits NaN or infinity.

A canonicalization failure is `encoding.EncodingError` with format JSON, no
cause, byte offset 0, no line or column, the exact `DataTree` path, and the
option's kind and reason. The public operation is

```jet
canonical(data: encoding.DataTree,
          limits: encoding.EncodingLimits = encoding.EncodingLimits.safe())
    -> String encoding.EncodingError!
```

Validate `EncodingLimits` in field order before traversal. Retain
`buffer_bytes` and expansion fields in the shared type but do not consume
them. `max_depth` bounds `DataTree` container nesting with root-container
depth 1; `max_total_bytes`, when `Val(n)`, bounds final UTF-8 output bytes;
`max_item_bytes` bounds aggregate live canonical-object workspace. Render
arrays directly into one final output allocation and buffer object members for
sorting. Excluding caller-owned `DataTree` and the returned `String` after
transfer, codec-owned live heap is at most
`current_output_bytes + max_item_bytes + (256 * max_depth) + 65536` bytes.
Return `Limit` before retaining crossing output or workspace bytes. The
aggregate workspace is the sum of encoded keys, UTF-16 comparison keys,
separators, and not-yet-emitted canonical child bytes across all simultaneously
open nested objects; bound the aggregate, not each object independently. The
reader/writer path buffers through `ObjectEnd`, rejects duplicate keys or an
invalid value before accepting/emitting that buffered object, and follows the
same terminal and allocator law. Whole-value and reader/writer bytes are
identical for one event tree.

Under the shared streaming surface, `json.writer(..., canonical: true)` uses
the identical serializer, key comparator, domain checks, errors, and output
bytes. (D-JSONCANON1=A)

The input must be I-JSON. Bool and Null use their ordinary spellings. Text is
preserved scalar-for-scalar: U+0008, U+0009, U+000A, U+000C, and U+000D use
`\\b`, `\\t`, `\\n`, `\\f`, and `\\r`; other U+0000..U+001F use lowercase
`\\u00xx`; quote and reverse solidus are escaped; every other scalar emits
unchanged; slash never escapes. Reject invalid Unicode. Ordinary Jet `String`
cannot construct a lone surrogate.

Sort object keys recursively by unsigned UTF-16 code units of the raw,
unescaped key, with the prefix-shorter key first. Use the RFC 8785 frozen
ECMAScript `Number::toString` / ECMA-262 7.1.12.1 Note-2 shortest-
round-tripping binary64 algorithm, verified against RFC 8785 Appendix B.
`Float -0.0` emits `0`; finite `Float` uses that algorithm; NaN and either
infinity reject `Unsupported` with reason
`JCS cannot encode a non-finite Float`. Admit a `DataTree.Int` only when
IEEE-754 roundTiesToEven conversion to binary64 represents the same
mathematical integer: `9007199254740992` is admitted, while `9007199254740993`
is not. Serialize an admitted integer with the same ECMAScript algorithm.
Reject a non-representable integer with `Unsupported` and reason
`JCS requires Int exactly representable as IEEE 754 binary64; encode this integer as Text`.
Reject Bytes with `Unsupported` and reason
`JSON cannot encode Bytes; encode bytes as Text explicitly`. Reject duplicate
keys with `Syntax` and reason `JCS requires unique object keys`. Do not use a
locale, host formatter, Unicode normalization, decimal pre-rounding, or
external crate. (D-JSONCANON1=A; I8)

The breaking return and byte correction use edition `2027`; edition `2026`
keeps its compatibility signature and prototype bytes. `jet fix` is the
migration authority: it rewrites the canonical call and fallible handling,
reports affected hashing/signing fixtures, and leaves an explicit limits
argument only where the source already carried one. The beginner path is one
`json.canonical(data)` call for ordinary interoperable `DataTree`; the expert
path makes input domain, UTF-16 order, number algorithm, errors, buffering,
bytes, and edition transition auditable. Both whole-value and reader/writer
paths invoke one semantic writer. Independent evidence covers RFC 8785
vectors, number and key-order boundaries, invalid values, limits, chunk
boundaries, cross-tier byte parity, terminal errors, and edition migration.
Compiler and runtime code remain std-only; new compiler diagnostics remain
I4-gated, while runtime `EncodingError` text follows the shared law.

```jet
use core.encoding as encoding
use core.encoding.json as json

fn run() {
    data :: json.parse("{\"b\":-0.0,\"a\":1e30}") ?? panic("parse")
    canonical :: json.canonical(data) ?? panic("canonical")

    print(canonical)
}

// {"a":1e+30,"b":0}
// json.canonical(json.parse("9007199254740993"))
// JSON Unsupported at byte 0, path $: JCS requires Int exactly representable as IEEE 754 binary64; encode this integer as Text
```

## CBOR binary interchange

Use CBOR (RFC 8949) as the canonical binary interchange format. An IETF
standard with a deterministic profile gives stable bytes for hashing, signing,
and content-addressed stores, while native binary strings and a clean
extension model outweigh MessagePack's ecosystem edge; one format preserves
I8. The whole-value verbs are `parse`, typed `decode<T>`, `to_bytes`, and
`to_bytes_canonical`; CBOR has no `to_string`. (D-ENCBIN1=A)
```jet
use core.crypto as crypto
use core.encoding.cbor as cbor

#Codable
struct Tick { count: Int }

fn run() {
    tick :: Tick{ count: 1 }
    bytes :: cbor.to_bytes(tick) ?? panic("encode")
    back :: cbor.decode<Tick>(bytes) ?? panic("decode")
    hash :: crypto.sha256(cbor.to_bytes_canonical(tick) ?? panic("canonical"))
    _ :: back
    print(hash)
}
```


The exact namespace is `core.encoding.cbor`. It exports `CBOROptions`,
`CBORError`, `CBORErrorKind`, `parse`, `decode`, `to_bytes`, and
`to_bytes_canonical`; shared `DataTree` remains `core.encoding.DataTree`.
Use these current-signature forms:

```jet
parse(bytes: [U8],
      options: cbor.CBOROptions{cbor.CBOROptions.safe()})
    -> encoding.DataTree cbor.CBORError!
decode<T: Codable>(bytes: [U8],
                   options: cbor.CBOROptions{cbor.CBOROptions.safe()})
    -> T [FieldError]!
to_bytes<T: Codable>(value: T) -> [U8] cbor.CBORError!
to_bytes_canonical<T: Codable>(value: T) -> [U8] cbor.CBORError!
```

`parse` decodes one complete item to `DataTree`. Typed `decode<T>` uses the
same checked CBOR event/value engine directly into `T`. `to_bytes` emits
preferred interoperable CBOR; `to_bytes_canonical` selects RFC 8949 section
4.2.1 Core Deterministic Encoding in that encoder. Do not add a canonical-mode
enum or flag: one hash/signature spelling has one byte law. `DataTree`
satisfies `Codable`, so both byte verbs accept it without another overload or
encoder. There is no CBOR `to_string`; binary `parse` and `to_bytes` are the
analogues of text `parse` and `to_string`. (D-ENC-CBOR-SURFACE1=A; I8)

Map Codable `[U8]` to CBOR major type 2 and decode it back; never degrade it to
an integer array. Other Codable lists use major type 4, structs/maps use major
type 5 with text keys, strings use major type 3, and scalar mappings follow
Jet types. `parse` into `DataTree` rejects a major-type-2 byte string as
`Unsupported` because D-ENC-DYN1 has no lossless public `DataTree` case;
callers needing bytes use `decode<[U8]>` or a Codable field. Reject tags,
bignums outside `Int`, non-text map keys, duplicate text keys,
undefined/unsupported simple values, and all values outside `DataTree`; never
coerce. Typed `decode<T>` accepts only shapes admitted by `T`'s one Codable
schema. (D-ENC-DYN1; D-ENC-CBOR-SURFACE1=A)

`CBOROptions` is exactly
`{max_depth:Int,max_items:Int,max_bytes:Int,require_canonical:Bool}`.
`safe()` is
`{max_depth:256,max_items:1000000,max_bytes:1073741824,
require_canonical:false}`. Legal ranges are `1..4096`, `1..1000000000`, and
`0..1073741824` for the first three fields. Validate in field order before
reading input. `require_canonical: false` accepts every otherwise-supported
RFC 8949 encoding, including valid section 4.2.3 length-first map order as
ordinary noncanonical input; `true` requires section 4.2.1 Core form.

A root scalar has depth 0; a root array, map, or indefinite string has depth
1; entering each nested array, map, or indefinite string adds one.
`max_items` counts each encoded data item exactly once: scalar and container
items, map keys under the same scalar/container rule, map values, and each
definite chunk inside an indefinite text or byte string. A map key has no
additional surcharge and break bytes add zero. Count a tag before rejecting
it.

`max_bytes` first bounds input length and then independently bounds peak live
requested allocation. Count actual requested capacities before allocator
calls: byte/text payload capacity in bytes; array capacity times
`size_of(DataTree)`; map capacity times `size_of((String,DataTree))` plus key
capacities; decode-stack capacity times frame size; and error/path `String`
capacities. A reallocation prospectively replaces its old capacity charge with
the new one; freeing subtracts it. Exclude input-slice storage and allocator
metadata. Use arbitrary-precision arithmetic for every counter and capacity
product; return `Limit` before allocation or retention on a crossing, so
integer overflow cannot wrap. Tests pin logical accounting and a
counting-allocator ceiling of `max_bytes` plus allocator metadata for the exact
allocation sequence.

`CBORError` is exactly
`{kind:cbor.CBORErrorKind,byte_offset:Int,path:String,reason:String}`.
`CBORErrorKind` is the closed enum `Syntax`, `Truncated`, `Unsupported`,
`Limit`, `TypeMismatch`, `TrailingData`, and `NonCanonical`. `byte_offset` is
the zero-based byte beginning the failing item, or input length when required
bytes are missing; encoder/type errors use 0. The path root is `$`; an array
index appends `[<unsigned-decimal>]` with no leading zero except 0, and a text
map key appends `[<JSON-string>]`, including quotes and JSON escapes with
lowercase `\u00xx`. Examples are `$[0]`, `$["payload"]`, `$["a.b"]`, and
`$["x\"y"]`; no bare `.key` form exists. Before a key decodes, the map path
is its container prefix; otherwise use the deepest known prefix. The reason
names the rejected major/additional value, target expectation, limit, or
canonical rule. `parse` and `decode` reject trailing bytes with
`TrailingData`. Do not return a partial tree or typed escape; runtime failures
are Core values, not compiler diagnostics. Wrong static argument or target
types reuse existing Core diagnostics; a new compiler diagnostic requires
Diagnostics registration and I4 tests/UI.

Default `to_bytes` is deterministic for one Jet value within one toolchain but
promises only valid preferred-CBOR interoperability, not cross-version hash
identity: structs follow Codable field order and `DataTree` objects preserve
semantic order. Emit definite lengths, shortest integer and length arguments,
UTF-8 text, direct byte strings, and preferred Float representation; never
emit tags or indefinite containers. `to_bytes_canonical` is the one
hash/signature mechanism. It emits definite lengths, shortest integer and
length arguments, preferred shortest Float width preserving the Jet value,
canonical NaN `0xf97e00`, preserved signed zero, and rejects duplicate encoded
keys. RFC 8949 section 4.2.1 Core ordering sorts keys by pure unsigned
bytewise order.

When `require_canonical` is true, validate original bytes rather than a
re-encoded approximation. Reject length-first-only ordering, indefinite
lengths, non-shortest arguments, non-preferred floats or NaNs, duplicate keys,
and every other Core-profile violation with `NonCanonical` at the first
offending item. Normal parsing accepts noncanonical but valid supported
encodings and returns the same semantic value. Neither mode accepts a value
outside the `DataTree`/Codable laws.

Edition `2026` retains legacy `encode` and DataTree-returning `decode`. Edition
`2027` exposes canonical `to_bytes` and `parse` names and shared typed errors;
edition `2028` removes deprecated entries. `jet fix` plus an explicit edition
upgrade is the migration authority. There is no permanent alias,
return-type-only overload, or second canonical encoder.

CBOR reader/writer handles use the codec-native pull contract, the same
`DataEvent`/Codable engine, field-by-field `CBORError` projection into shared
`EncodingError`, and the same deterministic writer mode. Reader/writer mode
cannot bypass the DataTree, limits, error, or canonical laws. Only
`json.writer` has a `canonical:` constructor argument; CBOR canonical bytes
use `to_bytes_canonical`. The beginner path is `to_bytes(value)` and typed
`decode(bytes)`; the expert path exposes tree values, exact errors/offsets/
paths, bounded decoding, canonical validation, native bytes, and RFC output.
All paths share one engine and Codable mapping. (D-ENC-CBOR-SURFACE1=A;
D-ENCSTREAM-SURFACE1=A)

Project whole-value CBOR errors into the shared stream error law:
`CBORError` `Syntax` maps to `EncodingErrorKind.Syntax`, `Truncated` to
`Truncated`, `Unsupported` to `Unsupported`, and `Limit` to `Limit`.
`TypeMismatch`, `TrailingData`, and `NonCanonical` map to `Syntax`. Copy
format, `byte_offset`, path, and reason; line and column remain `None`, and
cause remains `None`. Underlying file I/O bypasses `CBORError` and maps
directly to `EncodingErrorKind.IO` with its handle-free `EncodingCause`. No
`CBORErrorKind` maps to `State`; reserve `State` for writer lifecycle/order
misuse. Terminal clone/equality follows the shared reader/writer law.
(D-ENC-CBOR-SURFACE1=A; D-ENCSTREAM-SURFACE1=A)


```jet
use core.encoding as encoding
use core.encoding.cbor as cbor

#Codable
struct Packet { id: Int, payload: [U8] }
#Codable
struct Header { id: Int }

fn run() {
    packet :: Packet{ id: 7, payload: [222, 173] }
    bytes :: cbor.to_bytes(packet) ?? panic("encode")
    copy :: cbor.decode<Packet>(bytes) ?? panic("decode")
    stable :: cbor.to_bytes_canonical(packet) ?? panic("canonical")
    data :: cbor.parse(cbor.to_bytes(Header{ id: 7 }) ?? panic("header"), cbor.CBOROptions.safe()) ?? panic("parse")
    _ :: copy
    _ :: stable
    _ :: data

    malformed :: [0xA2, 0x62, 0x69, 0x64, 0x07, 0x67, 0x70, 0x61, 0x79, 0x6C, 0x6F, 0x61, 0x64, 0x42, 0xDE]
    failed :: cbor.decode<Packet>(malformed) ?? panic("expected failure")
    _ :: failed
}

// Err([FieldError{ path: "[\"payload\"]", reason: "CBOR Truncated at byte 15: CBOR byte string declares 2 bytes but input ended after 1" }])
```

## Strict base encodings

Use deterministic RFC 4648 encoders: standard base64 emits padding,
base64url emits no padding, and base32 emits uppercase padding. The strict
decoder law is independent of backend behavior. In edition `2027`, strict
base64, base64url, and base32 reject ASCII and non-ASCII whitespace, the other
alphabet, lowercase base32, missing/excess/interior padding, impossible
encoded lengths, and non-zero unused bits. Therefore
`decode(encode(bytes))` succeeds and `encode(decode(text)) == text` for every
accepted strict input. Encoders never wrap lines or gain policy flags.
(D-ENCBASE-STRICT1=A)
The common strict law also governs options A, B, and C's
`decode_canonical` and `decode_url_canonical` functions. The allowance API
below is the A surface; its named arguments are the only transport
deviations.


The strict matrix is:

- Standard base64 accepts `Zg==`, `Zm8=`, `Zm9v`, and empty text; it rejects
  `Zg`, `Zg===`, `Z=g=`, `Zg==\n`, URL `-`/`_`, and `Zh==`.
- Base64url accepts `Zg`, `Zm8`, `Zm9v`, and empty text; it rejects `Zg=`,
  `Zg==`, whitespace, standard `+`/`/`, lengths congruent to one modulo four,
  and `Zh`.
- Base32 accepts `MY======`, `MZXQ====`, `MZXW6===`, `MZXW6YQ=`, `MZXW6YTB`,
  and empty text; it rejects `MY`, `my======`, `MY=======`, `M=Y=====`,
  `MY======\n`, `0`/`1` aliases, and `MZ======`.

RFC 4648 final-quanta rules permit only zero, one, or two trailing `=` in
base64 as required, and only zero, one, three, four, or six in base32 as
required.

The exact edition-2027 APIs are:

```jet
pub fn encode(bytes: [U8]) -> String
pub fn decode(text: String,
              allow_whitespace: Bool{false},
              allow_missing_padding: Bool{false})
    -> [U8] String!
pub fn encode_url(bytes: [U8]) -> String
pub fn decode_url(text: String,
                  allow_whitespace: Bool{false},
                  allow_padding: Bool{false})
    -> [U8] String!

pub fn encode(bytes: [U8]) -> String
pub fn decode(text: String,
              allow_whitespace: Bool{false},
              allow_missing_padding: Bool{false},
              allow_lowercase: Bool{false})
    -> [U8] String!
```

The first group belongs to `core.encoding.base64`; the second group belongs to
`core.encoding.base32`. Return the first failure as
`invalid <base64|base64url|base32> at byte <N>: <reason>`. `N` is the
zero-based byte offset in original UTF-8 input; EOF failures use original input
byte length. Hex digits are uppercase. Exact reasons are `ASCII whitespace is
not allowed`, `byte 0xNN is not in the <standard base64|URL-safe base64|base32>
alphabet`, `padding is not allowed`, `padding may appear only at the end`,
`expected <N> padding characters`, `unexpected padding`, `encoded length
cannot represent whole bytes`, and `non-zero unused bits`.

Examples of the exact contract are:

```text
base64.decode("Zg")
  -> invalid base64 at byte 2: expected 2 padding characters
base64.decode("Zg==\n")
  -> invalid base64 at byte 4: ASCII whitespace is not allowed
base64.decode("Zh==")
  -> invalid base64 at byte 1: non-zero unused bits
base64.decode_url("Zg==")
  -> invalid base64url at byte 2: padding is not allowed
base32.decode("my======")
  -> invalid base32 at byte 0: byte 0x6D is not in the base32 alphabet
base32.decode("MZ======")
  -> invalid base32 at byte 1: non-zero unused bits
```

Named allowances are the only accepted transport deviations. `allow_whitespace`
recognizes only ASCII bytes `0x09`, `0x0A`, `0x0B`, `0x0C`, `0x0D`, and `0x20`
at any position, including before, between, or after padding. Remove them
while retaining an origin-offset map; other Unicode whitespace remains
forbidden. `allow_lowercase` maps base32 `a..z` to `A..Z` after whitespace
removal and accepts mixed case; it never maps `0`/`1` aliases.

For standard base64, `allow_missing_padding` accepts exact RFC padding or no
`=` with cleaned length modulo four equal to 0, 2, or 3; modulo one fails at
original EOF, and partial/interior/excess padding and non-zero unused bits
still fail. Base32 analogously accepts exact padding or no `=` with cleaned
length modulo eight equal to 0, 2, 4, 5, or 7; remainders 1, 3, or 6 fail at
EOF. Base64url is canonically unpadded; `allow_padding` additionally accepts
exact RFC-required zero, one, or two trailing `=` for the data length, never
partial/interior/excess padding.

Apply options in this order: scan original bytes left-to-right, classifying
selected-alphabet bytes, recognized `=` tokens, and allowed whitespace; return
the first disallowed whitespace or byte that is neither in the alphabet nor
`=`; remove allowed whitespace while preserving origins; fold allowed base32
lowercase; validate every recognized `=` in the alphabet-specific padding
phase and report the first offending original `=` offset; validate cleaned
length and padding at original EOF; validate unused bits at the original final
data-symbol offset; decode. Never report `=` as a non-alphabet byte: the
padding phase owns padding-not-allowed, interior, partial, and excess-padding
errors. Options relax only their named forms and never change error precedence
or offsets.

Use one std-only parser and table-driven RFC 4648 vectors across AOT,
`comptime`, `jet eval`, REPL, and `jet dev`; parity tests compare every option
combination and malformed corpus case byte-for-byte. Edition `2026` uses one
compatibility parser across those tiers and accepts the historical union with
the same bytes everywhere, never backend drift. A stricter omitted-argument
default is a breaking change in edition `2027`; named strict defaults and
allowances apply there. `jet fix` preserves named allowances while migrating
edition-2026 input and reports data no allowance can preserve. The beginner
path always round-trips encoder output and rejects ambiguous text at the exact
byte; the expert path has narrow named interoperability controls on the same
mechanical decoder, not a second permissive codec. (D-ENCBASE-STRICT1=A)

```jet
use core.encoding.base64 as base64
use core.encoding.base32 as base32

fn run() {
    body :: "Zg=="
    token :: "Zg=="
    text :: "my======"
    raw :: base64.decode("Zg==") ?? panic("decode")
    print(raw.len())
    bad :: base64.decode("Zg==\n") ?? panic("expected failure")
    _ :: bad
    // Err("invalid base64 at byte 4: ASCII whitespace is not allowed")

    mime :: base64.decode(body, allow_whitespace: true) ?? panic("mime")
    url :: base64.decode_url(token, allow_padding: true) ?? panic("url")
    legacy_id :: base32.decode(text, allow_lowercase: true, allow_missing_padding: true) ?? panic("base32")
    bytes :: base32.decode(text, allow_whitespace: true, allow_missing_padding: true, allow_lowercase: true) ?? panic("base32")
    _ :: mime
    _ :: url
    _ :: legacy_id
    _ :: bytes
}

// `jet fix --edition=2027` preserves named relaxations, then audits forms no allowance can preserve.
```

## Byte-to-text and hex

Keep `String.from_bytes` strict: return a typed decode failure at the first
invalid UTF-8 sequence and never substitute text. Keep
`String.from_bytes_lossy` as the explicit lossy spelling; it replaces each
invalid sequence with U+FFFD. Both use one Prelude implementation across
execution tiers.

Keep `core.encoding.hex.decode` strict and exact. Reject surrounding or
internal whitespace, odd-length text, and non-hex characters; do not trim or
silently repair input. These runtime parse/decode failures are typed Core
values, not compiler diagnostics. Executable examples and goldens snapshot
the observable rejection text. (D-BYTESDECODE1=A)

## Evidence and migration contracts

Use external encoding corpora under [`tests/fixtures/encoding`](../../tests/fixtures/encoding/)
and their `MANIFEST.tsv` rows for URL, license, and SHA-256 metadata.
[`tests/encoding_corpus.rs`](../../tests/encoding_corpus.rs) verifies each
manifest before use. Label local hostile oracles `local`; they are not
independent RFC corpora. Checked examples cover whole-value breadth, base
allowances, reader/writer lifecycle, and shared types; their goldens are under
[`Examples/features/expected/serde`](../../Examples/features/expected/serde/).
The matched Python fixture is
[`encoding_json_stream.py`](../../Examples/features/serde/encoding_json_stream.py).
It uses `json.dump(..., sort_keys=True, separators=(",", ":"))` and
`json.load` for the same canonical file round trip. The parity test must
exercise the Jet fixture on AOT, resident JIT, and the forced interpreter;
E0956 and E0953 are tier failures, not accepted parity results.


Static migration diagnostics `L2001` and `E2002` for `cbor.encode` use
[`Diagnostics.jet`](../../crates/jet-codegen/src/Prelude/Diagnostics.jet), the
diagnostic registry, snapshots in
[`tests/ui/cbor_encode_deprecated`](../../tests/ui/cbor_encode_deprecated/) and
[`tests/ui/cbor_encode_removed`](../../tests/ui/cbor_encode_removed/), and
[`tests/encoding_edition.rs`](../../tests/encoding_edition.rs). Runtime
parse/decode failures remain typed Core values, not compiler diagnostics.
