# 3282 — XML and markup data-parser profile

Date: 2026-09-29. Closer02. Binary: jet-debug-snapshot14 via `~/.cache/jet-luna/safe-jet.sh`.

## Question

What XML and markup data-parsing profile does Core ship (namespaces, entities, external resolution, malformed input, depth and expansion limits)? Does it avoid implicit IO and executable markup? Where does the ratified D-ENCXML1=A law go unimplemented?

## Method

- Read `Core/encoding/xml.jet:1-218` and `crates/jet-codegen/src/Prelude/Core.jet:100-101`. `parse`, `parse_bytes`, `parse_with`, `canonical`, `reader` and `to_string` are Jet source. `decode<T>` is host (`jet_enc_xml_decode`, `EncodingCodecs.rs:379`).
- Read `Docs/spec/encoding-decisions.md:216-335` (D-ENCXML1=A) and `Core/text/html.jet:1-86`.
- Ran in `~/.cache/jet-test-scratch/Closer02/`: `xml_profile_existing.jet` (a copy of the committed `Examples/features/serde/xml_profile.jet`), `xml_min.jet`, `xml_probe.jet` (28 cells), `xml_typed.jet` (15 typed cells) and `html_probe.jet`, each on JIT, interpreter and AOT (`tiers.sh`).

## Evidence: no XML parse path runs on the current binary

| Program | JIT (`run`) | Interpreter (`run --interpret`) | AOT (`build`) |
|---|---|---|---|
| `xml_min.jet`: `xml.parse("<r a=\"1\">t</r>")` | ICE `checked TIR cannot lower to MIR … XmlParser::parse_element: checked arithmetic has no resolved fixed numeric type` | same ICE | (same lowering; the build of `xml_probe` family stops at the same ICE) |
| committed `Examples/features/serde/xml_profile.jet` | ICE `checked TIR cannot lower to MIR at 422..429: show: PatternTest` (the `if err.kind == { .Syntax … }` table) | same | ICE, same text, from `crates/jet-driver/src/Driver/mod.rs:6546` |
| `xml_typed.jet`: `xml.decode<Note>(text)` | ICE `resolved MIR Prelude symbol 'jet_enc_xml_decode' is not registered` | `E0956 core.encoding.xml.decode() requires a type argument isn't supported by the current evaluator yet` | generated Rust fails: `error[E0061]: this function takes 2 arguments but 1 argument was supplied` at `jet_enc_xml_decode::<…Note>(&text)` |

So no cell of the XML profile can be executed today. The committed `xml_profile.jet` has no `.out`, and none was blessed.

## Profile as written in source versus ratified law (source inspection, not execution)

| Cell | `Core/encoding/xml.jet` (shipped source) | D-ENCXML1=A (spec) | Status |
|---|---|---|---|
| Version | well-formed XML 1.0, no DTDs (:43-53) | XML 1.0 5th ed. + Namespaces 1.0 (:218-219) | partial |
| DOCTYPE | rejected, `DTDs are forbidden in Core XML` (:295-296), `XMLReason.ForbiddenDtd` | DOCTYPE and internal declarations are inert preserved data; general refs default `Preserve`, `Reject`/`Resolve(map)` selectable (:236-244) | **diverges** (stricter than law) |
| Predefined and numeric refs | decoded (:47-49) | decoded (:236) | matches |
| Unknown entity | `UnknownEntity` | Preserve by default | diverges |
| Parameter/external entities, XInclude | rejected; never fetched (:47) | `Unsupported`; parser never opens files, URLs, sockets or catalogs (:232-244) | matches in intent |
| Namespaces | "namespace rewriting rejected"; names are raw strings, `expanded_name(name)` returns the name unchanged (:172) | `XMLName{raw,prefix,local,namespace_uri}`, Clark keys, binding validation (:252-257) | **ratified but unimplemented** |
| Tagged tree | `{name, attrs, children}` object (:45-46) | tagged `$xml` tree, `$text`/`$content` (:259-267, :390-414) | **ratified but unimplemented** |
| C14N | `XMLCanonicalMode{Inclusive, Exclusive}`; `canonical(text)` = `to_string(parse_xml(text))` (:82-85, :159) | `xml.canonical(doc, XMLCanonical)` with `Inclusive11` / `Exclusive10`, W3C algorithms (:317-328) | **ratified but unimplemented** |
| Limits | `XMLLimits` with safe depth 256, nodes 1e6, attrs 1024, name 1024, text 16 MiB, entity budgets 0 (:30-41); validated in `parse_with` (:139-150) | same field set, owned by `XMLParseOptions` (:246-250) | shape matches; `parse_with(text, XMLLimits)` differs from the ratified `parse_with(text, XMLParseOptions)` used by `encoding_canonical.jet` |
| Error type | returns `EncodingError`; declares an unused `XMLError{kind: XMLReason(Syntax|Truncated|ForbiddenDtd|UnknownEntity)}` (:54-69) | `XMLError` with 12 reasons (:282-284) | diverges |

HTML (`Core/text/html.jet`) offers escaping, bounded entity decoding and span-based `strip_tags` (:14-16, :62). There is no HTML parser and no renderer, so markup is never executed. See `html_probe.jet` below for the executed cells.

## Criteria

1. State namespaces, entities, external resolution, malformed input and limits: **stated from source and spec (table above), but not executed**, because every parse path ICEs.
2. No implicit external IO or executable markup: **true by source inspection**. There is no IO call in xml.jet, and html.jet has no parser. This is not executed for XML.
3. Data parsing versus the excluded GUI renderer: **stated and executed for HTML.** `html_probe.jet` produced identical output on JIT, interpreter and AOT: `escape` gives `&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;`; `strip_tags("<p>hi <script>alert(1)</script><b>there</b></p>")` gives `hi alert(1)there` (a span operation that keeps script text); `strip_tags` of `<a title="a>b">x</a>` gives `b">x`. Defect: `unescape("&lt;b&gt; &amp; &unknown; &#65; &#x42;")` gives `<[98]>[32]&[32]&[117][110]…A[32]B`, rendering plain characters as bracketed byte values. `core.encoding.xml` and `core.text.html` are data and text utilities, and neither renders.
4. Gate missing format profiles: an HTML *parse* profile has no ratified decision. The ballot draft is below. Namespaces, C14N and the tagged tree are **not** missing profiles: they are ratified (D-ENCXML1=A) and unimplemented.
5. Golden `serde/xml_profile` plus a separate card for the D-ENCXML1 gap: **not met.** The golden cannot run (ICE on all tiers), and no implementation card exists (`tower card list | grep -i xml` shows only #3282). The card text is drafted below for Pip to file, since closers make no Tower writes.

## Draft card (for Pip to file): Implement D-ENCXML1 namespaces, tagged tree and C14N in Core XML

- Scope: Core/encoding/xml.jet. Add `XMLName{raw,prefix,local,namespace_uri}` resolution, the tagged `$xml` tree (spec :252-267, :390-414), `XMLParseOptions{entities, limits}` with `parse_with(text, options)`, `XMLError` with the 12 reasons (:282-284), `xml.canonical(doc, XMLCanonical{mode: .Inclusive11|.Exclusive10, comments, inclusive_prefixes})` per :317-328, and DOCTYPE preservation per :236-244.
- Prerequisite defect: the `xml.parse` ICE (`XmlParser::parse_element: checked arithmetic has no resolved fixed numeric type`) and `xml.decode<T>` on all tiers (see Evidence).
- Proof: repair `Examples/features/serde/encoding_canonical.jet` with its golden; `JET_GOLDEN_FILTER=serde/xml_profile` and `serde/encoding_canonical` pass on AOT/JIT/interpreter.

## Ballot draft: D-HTML-PARSE1 — should Core parse HTML documents?

- Question: Core has HTML escaping and a span-based `strip_tags`, but no HTML parser. Should Core parse HTML into a data tree?
- Option A, no Core HTML parser (recommended): keep `core.text.html` as escape, unescape and strip only. Applications that need the WHATWG parse algorithm use an approved package. Beginner: `html.strip_tags(text)` for plain text and `html.escape(text)` for output. Expert: an approved package. Cost: scraping jobs need a dependency.
- Option B, `core.text.html.parse(text) -> DataTree HTMLError!` implementing the WHATWG tree-construction algorithm into the same tagged tree shape as XML, with no script execution, no fetching and bounded depth and nodes. Beginner: `doc :: html.parse(page)`. Expert: limits and events. Cost: a large algorithm to maintain in Core.
- Recommendation: A, because Core stays a data-parser boundary, and the WHATWG algorithm is large and changes over time. The recommendation is not ratified.

## Verdict

**PARTIAL.** Criteria 1-4 are met as an investigation: the profile is stated from source and spec, and the missing HTML-parse profile is gated by the ballot draft. Criterion 5 is unmet: no XML parse path executes on the current binary (defects in the closer report), and the D-ENCXML1 card has to be filed by Pip from the draft above.
