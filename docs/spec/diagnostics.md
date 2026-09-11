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
