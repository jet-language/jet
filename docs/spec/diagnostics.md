# Diagnostics

This is the contributor contract for user-facing diagnostics. It describes the
one source and the checks around it; it is not a second diagnostic catalog.

## Authority and projections

The canonical rows live in
[`Diagnostics.jet`](../../crates/jet-codegen/src/Prelude/Diagnostics.jet).
Each row owns its stable code, stage, severity, timing, What/Why/Fix templates,
named holes, and structured-fix metadata. Preserve the code and its meaning
when changing a row.

Codes use three stable shapes: `E` or `L` followed by four digits, `JT` followed
by four digits for source-migration reports, or `E-WORD-WORD` with two or more
uppercase words. Do not reuse or renumber a code.

The Foundation registry embeds that source as `Registry::DIAGNOSTIC_SOURCE` and
ingests it into `Registry::diagnostic_rows()`. Lexer, parser, and semantic
checks raise the row that owns the violated rule. Backends, the CLI, LSP,
machine reports, and web hosts consume those registered rows instead of
defining another message or falling through to raw backend text.

## Human and machine output

`Error [CODE]:` and `Warning [CODE] (lint_name):` are the stable human
headings. A diagnostic with a span includes the source location and underline;
one without a span omits that block. Every human report keeps `Why:`, `Fix:`,
and `More: jet-lang.dev/e/<CODE>` in that order, with optional detail before
`More:`. Multiple diagnostics are separated by one blank line.
Lints do not block compilation by default. A package may deny a named lint
through its existing lint policy; diagnostic codes are never policy values.

What, Why, and Fix are sentence-cased plain language. Name the user's
operation or value, explain the rule that rejected it, and give an imperative,
specific next step. Familiar non-canonical forms may receive a teaching
diagnostic, but a known case must never be hidden behind a generic error.

Structured fixes are typed metadata. A raise site may provide a source-derived
edit or a suggested edit for the named span; the human Fix sentence is never
parsed to recover an edit. Machine JSON and LSP data carry the registered code,
What/Why/Fix, span, and structured edits without requiring consumers to parse
human prose.
The canonical machine envelope is `jet.report/v3`. Its `cause` array is
nearest-first and contains objects with `code`, `file`, `line`, `col`, and
`span` fields. Repeated codes are not deduplicated. Each field is projected
from that cause's own `DiagnosticCause` and originating source snapshot;
unknown fields are `null` and never inherit the root diagnostic's location.
When a source snapshot is unavailable, the projection must not invent a path
or derived line/column (or a `0:0` position); an explicitly supplied byte span
remains as-is.
Safe edits may be applied automatically; suggested edits remain advisory until
the user accepts them.

## Explain and website output

`jet explain <CODE>` resolves the same registry rows offline through
[`Explain`](../../crates/jet-cli/src/Explain.rs). Keep its CLI and API behavior
on this canonical path.

The website is another projection of that live data:
[`site/generate.jet`](../../site/generate.jet) writes the error index, and
[`tests/diagnostic_pages.rs`](../../tests/diagnostic_pages.rs) renders each
code page from registry rows and snapshots. Website pages are product output,
not an alternate diagnostic source.

## Adding or changing a diagnostic

1. Confirm that the front end owns the rule and that the code is not already
   registered. Add or edit one typed row in `Diagnostics.jet`; new language
   semantics or syntax must already be ratified.
2. At the raise site, provide only the row's named-hole values and any
   structured fix already supported by the registry. Do not hand-write a
   second What/Why/Fix template.
3. Add or update the matching UI snapshot under `tests/ui/` or
   `tests/ui_lint/`. The report must point at the actionable source span,
   preserve structured fields, and contain no raw backend error.
4. Run the focused snapshot and coverage checks, then check
   `jet explain <CODE>`. Test behavior and rendered output where applicable;
   do not add a persistent Markdown mirror.

Coverage checks both directions: every emitted code has one registered row and
snapshot, and every row is emitted or explicitly marked retired or reserved in
the source.

Keep one meaning across AOT, `jet run`, the interpreter, LSP, machine output,
and web. A documentation or rendering change must not mint a new diagnostic or
change compiler semantics.

## Method-call fallback diagnostics

The existing `E0102` method fallback keeps the call span and the registered
What/Why/Fix representation. When a generic type parameter call matches a
known trait method, the checker reports that method's existing `E0104` arity
contract and uses `E0901` when the required trait bound is missing; it does not
invent a method or diagnostic code.

One-pass `Iter` and `ViewIter` values do not hold a cursor. A diagnostic for a
declined cursor-style operation must say that pulling a value consumes the
source. A list-only operation must also say that `.to_list()` consumes the
source and allocates the materialized list. The fix names the loop, lazy
adapter, or materializer that the programmer can use.

Core receiver types have a closed method surface. Their fallback fixes may
name a documented operation or a safe spelling edit, but must not suggest
adding an `impl` to a protected Core type. A user-defined type wins the
reserved-name guard and retains the ordinary user-type fix.
