# #3289 — Locale and message-catalog composition (evidence, 2026-09-29)

## Question

Is the ratified D-CORE-CATALOG1=A object-local GNU `.mo` catalog (`core.text.catalog`)
present, and how do the catalog jobs compose with the existing text/format owners and
source identity?

## Method

- grep `(?i)mustache|gettext|\.mo\b|struct Template|Catalog` over `Core/` found no match.
  grep `(?i)template|catalog|gettext` over
  `Compiler/JetFoundation/Source/Registry/CoreCallRows.jet` found no match.
- Ran the probe `~/.cache/jet-dev/scratch/Closer05/t3252/probe_catalog.jet`
  (`use core.text.catalog as catalog`) on snapshot14. It fails with
  `Error [E1001]: There is no core module 'core.text.catalog'`.
- Read the owners: `Core/text/fmt.jet:24-27` `plural(n, singular, many)` (English two-form,
  `abs(n) == 1`), `Docs/spec/spec.md:62-67` (the one interpolation grammar `{expr}` with a
  closed selector rail).

## Accounting (criteria 1–4)

| Job | Ratified contract (D-CORE-CATALOG1=A) | Existing owner today |
|---|---|---|
| Locale data vs source identity | Catalog is an explicit value built from caller bytes. The msgid (source text) stays the lookup key and is returned unchanged when an entry is missing | None. `fmt.plural` hard-codes English strings at the call site |
| Placeholders | Translated text flows back through the existing `{…}`/`core.text.fmt` path. The catalog adds no placeholder syntax (no second interpolation grammar) | `{expr}` is compile-time only, so a translated runtime string can't be re-interpolated. A catalog user formats with fmt helpers or explicit concatenation. This limitation must stay explicit and not be patched with a mini-grammar |
| Plural rules | `Plural-Forms` header, evaluated by a bounded expression evaluator. Unsupported formulas are errors | `fmt.plural` covers only a two-form `n == 1` rule. It is not multilingual |
| Fallback | Immutable, caller-ordered chain. No environment lookup | n/a |
| Redaction / secrets | `CatalogError` causes carry offsets and kinds, never unrelated application context | n/a |
| Ambient access | No filesystem, locale environment or global install. The caller reads the bytes | n/a |

Criterion 4 (independent ballot) is met by the ratified D-CORE-CATALOG1=A. Distribution or
format additions (`.po` text, ICU MessageFormat) would need new ballots.

## Verdict

PARTIAL. Criteria 1 to 4 are accounted for from the ratified ballot and the owners that
were read. Criteria 5 to 9 need `Core/text/catalog.jet`, its registry row, and
`Examples/features/text/message_catalog.jet` with embedded `.mo` fixtures. None of these
exists on the current tree or binary. That is an implementation job outside an
evidence-closer's remit.

## Follow-up

An implementation brief as in the card plan: `parse(bytes, limits)` for both endians and
revision 0/1, `chain`, `get`/`get_ctx`/`plural`/`plural_ctx`, and a typed `CatalogError`.
Fixtures should include a three-form `ru`/`pl` formula and the malformed cases (truncated,
offset, charset, plural formula, oversize).
