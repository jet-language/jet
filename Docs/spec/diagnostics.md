# Diagnostics

This page is the contract for user-facing diagnostics and their projections. It
covers registry ownership, terminal/LSP/machine rendering, the no-OS E3301
contract, and the checks for changing a diagnostic. The canonical source is
`crates/jet-codegen/src/Prelude/Diagnostics.jet`; executable checks include
`tests/cross.rs`, `tests/ui/`, and `tests/diagnostics_coverage.rs`.

## One registry, many projections

Each row in
[`Diagnostics.jet`](../../crates/jet-codegen/src/Prelude/Diagnostics.jet) owns
its stable code, stage, severity, timing, What/Why/Fix templates, named holes,
and structured-fix metadata. Preserve the code and its meaning when changing a
row. The Foundation embeds this source as `Registry::DIAGNOSTIC_SOURCE` and
loads it through `Registry::diagnostic_rows()`.

The current registered code shapes are `E` or `L` followed by four digits,
`JT` followed by four digits for source-migration reports, and `E-WORD-WORD`
with two or more uppercase words. Codes are stable identities: do not reuse or
renumber one. Lexer, parser, and semantic checks raise the row that owns the
violated rule; the CLI, LSP, machine reports, web pages, and backends project
those rows rather than defining a second message or exposing raw backend text.

## Plain-words rubric

Every active row, and every message builder that fills one, reads in plain
words for the learner who hit it:

- **What** names the specific problem with the names from the learner's code:
  "when `n` is `0`, `sign` reaches the end without a value", not a restatement
  of the rule's internal meaning.
- **Why** gives the reason in one or two plain sentences. A placeholder such as
  "The registered sema rule applies here" is not a reason.
- **Fix** gives a concrete edit or command.
- Human fields carry no internal vocabulary: no decision IDs (`D-…`, `S…`), no
  constant names, no "unit", "source bytes", byte offsets, `key=value` dumps,
  or stage names. That material belongs to `jet explain CODE --verbose` and
  `--json`.

## Human rendering

The fixed headings are `Error [CODE]:` and `Warning [CODE] (lint_name):`. A
report with a span includes its source location and underline; a report without
a span omits that block. Every report keeps `Why:`, `Fix:`, and
`More: jet-lang.dev/e/<CODE>` in that order, with optional detail before
`More:`. Multiple diagnostics are separated by one blank line. Lints do not
block compilation by default; a package may deny a named lint through its lint
policy, but a diagnostic code is never a policy value.

What, Why, and Fix are sentence-cased plain language. What names the rejected
operation or value, Why states the rule, and Fix gives an imperative next step.
A familiar non-canonical form may receive a teaching diagnostic, but a known
case must not be hidden behind a generic error.

Structured fixes are typed metadata. A raise site supplies source-derived or
suggested edits for the named span; the human Fix sentence is never parsed to
recover an edit. Machine JSON and LSP data carry the registered code,
What/Why/Fix, span, and structured edits without requiring consumers to parse
human prose. Safe edits may be applied automatically; suggested edits remain
advisory until accepted.

## Machine reports and causes

The canonical machine envelope is `jet.report/v3`. Its `cause` array is
nearest-first and contains `code`, `file`, `line`, `col`, and `span` fields.
Repeated codes are not deduplicated. Every field comes from that cause's own
`DiagnosticCause` and originating source snapshot; unknown fields are `null`
and never inherit the root diagnostic's location. If a source snapshot is
unavailable, the projection must not invent a path, line, column, or `0:0`
position. An explicitly supplied byte span remains unchanged.

Wrapping a diagnostic appends the nearest report's code, primary span, and
source snapshot, then that report's complete existing cause chain. Existing
causes stay before the appended chain. Batch root-first ordering compares all
three identity fields, keeps independent reports stable, and preserves the
remaining source order when a cycle prevents further progress. The JSON Lines
projection keeps its input order; `clears` counts each other report whose
explicit chain names that diagnostic once, even if the chain repeats it.

Fix safety, applicability, and no-fix reasons share the canonical Foundation
report carriers. A reviewed no-fix next action must contain non-whitespace text
and no Unicode control characters. Diagnostic attachment rejects any primary
or alternative edit before validating the reason. Report-envelope attachment
validates the reason first, then rejects edit/reason coexistence; adding an edit
to a reasoned envelope also fails. These rejected operations leave the prior
report unchanged. JSON emits either `fix_edits` or `no_fix_reason`, never both.

## No-OS E3301 contract

E3301 is the registered rule for an OS-dependent API selected for a no-OS
target. The exact rendered snapshot is part of the contract and is kept in
`tests/ui/freestanding_e3301.stderr`:

```text
Error [E3301]: `files.read` is not available on a no-OS target.
  --> tests/ui/freestanding_e3301.jet:5:13
    |
  5 |     _ :: fs.read("config.txt")
    |             ^^^^
 Why: No-OS targets have no OS; only `core`-level APIs are available.
 Fix: Embed the data at compile time with `@embed("file")`, or select a hosted target.
More: jet-lang.dev/e/E3301
```

`tests/cross.rs` compiles the same `fs.read` fixture with
`compile_no_os`, checks E3301 and the `no-OS` wording, and compares the
byte-exact rendered output. The registry row is E3301 in
`crates/jet-codegen/src/Prelude/Diagnostics.jet`; the no-OS compilation path is
`Source/lib.rs::compile_no_os`. Hosted and no-OS behavior therefore share the
same diagnostic renderer while the target rule remains explicit.

## Explain and website output

`jet explain <CODE>` resolves the same registry rows offline through
[`Explain`](../../crates/jet-cli/src/Explain.rs). The website is another
projection of that data: [`Docs/site/generate.jet`](../site/generate.jet) writes
the error index and [`tests/diagnostic_pages.rs`](../../tests/diagnostic_pages.rs)
renders each code page from registry rows and snapshots. Neither CLI help nor a
website page is an alternate diagnostic catalog.

`jet explain <token>` answers a sigil or operator from the syntax dictionary
([`dictionary.rs`](../../crates/jet-foundation/src/Syntax/dictionary.rs)) in
plain words: every meaning by position (`^` before a name moves it; between two
numbers it raises to a power), an example for each, what happens afterwards, and
related codes. The REPL `?` and LSP hover show the same text. The registry
constant and owning decision appear only with `--verbose`.

## Adding or changing a diagnostic

1. Confirm that the front end owns the rule, the code is not already registered,
   and the language rule is ratified. Add or edit one typed row in
   `Diagnostics.jet`.
2. At the raise site, provide only the row's named-hole values and any
   structured fix supported by the registry. Do not hand-write a second
   What/Why/Fix template.
3. Add or update the matching snapshot under `tests/ui/` or `tests/ui_lint/`.
   It must point at the actionable source span, preserve structured fields, and
   contain no raw backend error.
4. Run the focused snapshot and coverage checks, then check
   `jet explain <CODE>`. Test rendered behavior where applicable; do not add a
   persistent Markdown mirror.

Coverage checks both directions: each emitted code has one registered row and
snapshot, and each row is emitted or explicitly marked retired or reserved in
the source. Keep one meaning across AOT, `jet run`, the interpreter, LSP,
machine output, and web. Documentation and rendering changes must not mint a
new diagnostic or change compiler semantics.

## Latency and runtime-query boundaries

D-DEV3 is a wall-clock promise: a save gives diagnostic feedback in well under
200ms. `tests/dev_latency.rs` measures `Examples/features/collections/wordcount.jet`
with `BUDGET_MS: u128 = 200`, uses five timed rounds after warm-up, and keeps
watch coalescing below 5ms with idle polling at most 10ms. This is the user
latency contract, not a claim that every machine has one fixed runtime.

Layout infeasibility is deliberately different. `tests/layout.rs` records that
an infeasible constraint is a runtime query followed by a panic, not a static
diagnostic. Do not document it as a compile-time error or add a diagnostic row
for it without changing that contract and its tests.

## Method-call fallback diagnostics

The existing E0102 method fallback keeps the call span and registered
What/Why/Fix representation. When a generic type-parameter call matches a known
trait method, the checker reports that method's existing E0104 arity contract
and uses E0901 when the required trait bound is missing; it does not invent a
method or diagnostic code.

One-pass `Iter` and `ViewIter` values do not hold a cursor. A declined
cursor-style operation must say that pulling a value consumes the source. A
list-only operation must also say that `.to_list()` consumes the source and
allocates the materialized list. The fix names the loop, lazy adapter, or
materializer available to the programmer.

Core receiver types have a closed method surface. Their fallback fixes may
name a documented operation or a safe spelling edit, but must not suggest
adding an `impl` to a protected Core type. A user-defined type wins the
reserved-name guard and retains the ordinary user-type fix.
