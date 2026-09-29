# #3252 — Bounded runtime template rendering (evidence, 2026-09-29)

## Question

Is the ratified D-CORE-TEMPLATE1=A renderer (`core.text.template`) present, and how does
the runtime-template job relate to the existing text and format owners?

## Method

- Searched Core for any template renderer: grep `(?i)mustache|gettext|\.mo\b|struct Template|Catalog`
  over `Core/` found no match. grep `(?i)template|catalog|gettext` over
  `Compiler/JetFoundation/Source/Registry/CoreCallRows.jet` found no match.
- Ran a probe, `~/.cache/jet-test-scratch/Closer05/t3252/probe.jet`
  (`use core.text.template as template`, `template.parse_html(...)`), on snapshot14 (result below).
- Read the owners: `Core/text/html.jet:16-86` (`escape`, `escape_quote`, `attr_escape`,
  `text_escape`, `unescape`, `strip_tags`), `Core/text/fmt.jet` (number/pad/plural helpers),
  `Docs/spec/spec.md:62-89` (compile-time `{expr}` interpolation, the closed selector rail,
  and the checked `HTML{"…"}` / `SQL{"…"}` heads; runtime `String` to checked text is E0149).

## Accounting (criteria 1–4)

| Aspect | Ratified contract (D-CORE-TEMPLATE1=A) | Existing owner today |
|---|---|---|
| Grammar | `{{name}}`, `{{{raw}}}`, `{{#s}}`/`{{^s}}`/`{{/s}}`, dotted paths, comments. No partials, lambdas, includes or eval | None at runtime. `{expr}` interpolation and `HTML{"…"}` are compile-time only, and their template text can't come from runtime data |
| Missing values | Typed `TemplateError.MissingField` with byte span and line/column | n/a |
| Partials / recursion | Excluded. Recursion bounded by `max_depth` | n/a |
| Escaping | HTML mode escapes `& < > " '`. Triple braces are raw and are not a sanitizer. Text mode is unescaped and rejects triple braces | `html.escape` / `attr_escape` escape `& < > " '` (`escape_quote(text, true)`, html.jet:18-29). `text_escape` escapes only `& < >`. A renderer should reuse these rather than add a second escaper |
| Trust context | HTML escaping is not a JS/CSS/URL-context guarantee (stated limitation) | Same limitation. There is no context-aware escaper in Core |
| Data view | `render<T: Encode>` walks the Encode field view. No JSON stringify/parse round trip | The `__jet_Encode`/DataTree traversal exists in `Core/encoding/encoding.jet` |
| Unknowns kept | Engine cost of reused templates versus interpolation, and dormant-import cost: unmeasured | n/a |

Criterion 4 (independent ballot before any added grammar) is already met by the ratified
D-CORE-TEMPLATE1=A. Anything beyond it (partials, filters, lambdas) needs a new ballot.

## Execution evidence

`safe-jet.sh run ~/.cache/jet-test-scratch/Closer05/t3252/probe.jet` →
`Error [E1001]: There is no core module 'core.text.template'`. The same probe without the
template import (`probe_catalog.jet`) → `Error [E1001]: There is no core module 'core.text.catalog'` (#3289).

## Verdict

PARTIAL. Criteria 1 to 4 are accounted for above from the ratified ballot and the owners
that were read. Criteria 5 to 9 need the renderer, and it doesn't exist: no
`Core/text/template.jet`, no registry row, no `Examples/features/text/template_render.jet`,
no perf pair. Implementing it is a new Core module plus a compiler registry row, which is
outside an evidence-closer's remit. This card needs an implementation worker.

## Follow-up

An implementation brief exactly as in the card plan (Changes 1 to 5). Reuse `html.escape_quote`
for HTML mode. Walk the Encode view directly. Limits are depth, output bytes and steps.
