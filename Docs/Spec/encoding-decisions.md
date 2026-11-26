# Jet encoding contracts

This document states the durable cross-codec contracts for `core.encoding.*`
(JSON, YAML, XML, CBOR, Erlang ETF, base encodings, hex). Exact APIs live in
`Core/Encoding/` and ratified ballot rationale lives in Tower. Use the
[Jet vocabulary](vocabulary.md) for domain terms. (Full historical prose lives
in git history: `git show 20be9d938a5:Docs/Spec/encoding-decisions.md`.)

## Shared value, error, and limit model

- **Typed errors only:** Every decoder and encoder returns typed `#Error`
  values carrying offset/line/column and exact violation cause; malformed input
  never panics or silently coerces.
- **Explicit safety bounds:** Every parser enforces bounded nesting depth,
  container/size limits, and alias-expansion budgets by default (for example,
  YAML nesting depth default is `256` with hostile alias expansion capped) and
  exposes explicit limit records for callers that need tighter or wider bounds.
- **Bounded streaming lifecycle:** Streaming `Reader`/`Writer` codecs pull and
  push incrementally through `Core/IO/` without buffering whole payloads,
  leaving underlying stream positions well-defined on error or EOF.
- **One meaning across tiers (I9):** Every codec executes one shared Jet
  implementation across O0, O1, O2, comptime, and web with identical validation,
  limits, and error codes.

## Per-format invariants

- **JSON:** Strict RFC 8259 syntax (no trailing commas/comments in strict
  mode); preserves exact numeric distinctions (integer vs. float, no silent
  precision loss); canonical output mode sorts object keys deterministically by
  UTF-8 code unit order with shortest round-trip float formatting.
- **XML:** Secure by default—external entities (XXE), DTD entity-expansion
  bombs, and network fetches are disabled. Streaming event readers emit
  `element_start` (with `empty_style` distinguishing empty elements without a
  synthetic `element_end`), text, and `element_end` events with bounded byte
  reuse, plus a safe tree/helper view built on the same parser.
- **CBOR:** RFC 8949 binary interchange supporting deterministic (CDE) encoding,
  explicit tag preservation, indefinite-length limits, and duplicate-key
  rejection in strict mode.
- **Erlang External Term Format (ETF):** Version `131` term codec with explicit
  atom/table and size limits, safe decoding into Jet values without unbounded
  atom allocation.
- **Base encodings & hex:** Strict RFC 4648 base64/base64url/base32 and hex
  codecs reject non-canonical padding bits and illegal characters by default;
  lenient/whitespace-ignoring modes are explicit opt-ins.

## Provider ownership

- `Core/Encoding/` owns every codec's validation, limits, and wire semantics.
  No runtime host, compiler tier, or wrapper duplicates codec policy.
