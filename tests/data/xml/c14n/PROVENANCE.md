# XML C14N corpus provenance

No licensed W3C / Merlin XML Signature vector pack is vendored in this
repository. This corpus is the smallest deterministic whole-document set that
exercises the ratified `xml.canonical` options under D-ENCXML1.

## Standards

- Inclusive11: W3C Canonical XML Version 1.1
  (https://www.w3.org/TR/xml-c14n11/)
- Exclusive10: W3C Exclusive XML Canonicalization Version 1.0
  (https://www.w3.org/TR/xml-exc-c14n/)

Vectors are hand-built from those algorithms for Jet's closed ordinary-DataTree
node schema. They are not copies of a third-party binary fixture pack.

## Coverage

| File stem | Mode | comments | inclusive_prefixes | What it proves |
|-----------|------|----------|--------------------|----------------|
| `incl11_sort_escape` | Inclusive11 | false | [] | NS/attr sort, empty expand, CDATA→text, attr whitespace escape, CR/LF normalize, discard decl/DOCTYPE/S |
| `incl11_with_comments` | Inclusive11 | true | [] | Same infoset with comment kept; PI/comment newline delimiters outside root |
| `excl10_omit_unused` | Exclusive10 | false | [] | Unused xmlns omitted; utilized prefixes kept |
| `excl10_inclusive_prefix` | Exclusive10 | false | `["b"]` | InclusiveNamespaces PrefixList forces unused `b` |
| `excl10_with_comments` | Exclusive10 | true | [] | Exclusive + comments; utilized NS on the using element |

Rejection cases live beside the Rust driver (relative NS URI, unresolved
entity, Inclusive11+inclusive_prefixes, non-document root).

## Driver

`crates/jet-foundation/src/XmlPull.rs` test
`canonical_xml_w3c_whole_document_corpus` loads these UTF-8 files and compares
exact canonical strings. Update expected files only after reading the full
diff against the cited W3C rules.
