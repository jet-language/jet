# Diagnostics

Vocabulary: [Jet vocabulary](vocabulary.md).

This page is the contract for user-facing diagnostics and their projections. It
covers registry ownership, terminal/LSP/machine rendering, the no-OS E3301
contract, and the checks for changing a diagnostic. The canonical source is
`Compiler/Prelude/Diagnostics.jet`; executable checks include
`tests/cross.rs`, `tests/ui/`, and `tests/cases/diagnostics_coverage/`.

## One registry, many projections

Each diagnostic in
[`Diagnostics.jet`](../../Compiler/Prelude/Diagnostics.jet) is a struct that
implements Core `Diagnostic`. Its fields are the named holes; its constants
hold the stable code, stage, severity (`.Error` by default), timing, status, and
structured-fix metadata; and `what`, `why`, and `fix` each return one string
literal whose `{self.field}` holes the report fills. Preserve the code and its
meaning when changing an impl. `Tools/agent/DiagnosticRows.jet` reads the impls
as source and writes the Foundation rows in `Registry/DiagnosticRows.jet`
(`--check` proves they agree). Retired and reserved codes stay types with their
status. They run in no stage, so they state no `stage`, and `jet explain`
prints none for them (`--json` gives `"stage": null`).

A diagnostic's name is its type path: `jet.UnusedParameter` for the compiler's
own, `web.BadPath` for a library's. The current registered code shapes are `E`
or `L` followed by four digits, `JT` followed by four digits for
source-migration reports, and `E-WORD-WORD` with two or more uppercase words. A
library diagnostic without a `code` constant uses its type path as its code.
Codes are stable identities: do not reuse or renumber one. Lexer, parser, and
semantic checks raise the row that owns the violated rule; a library rule
raises its own type with `reject(Value)`. The CLI, LSP, machine reports, web
pages, and backends project those rows rather than defining a second message or
exposing raw backend text.

Known declaration heads such as `#Import c.<lib>: module` are parsed as items
before unresolved library marker heads are retained as file-level rule facts.

Lexical recovery does not end diagnostic collection: the retained token stream
is parsed so parser-owned teaching accompanies lexer errors. Failed
embedded-Core parses keep the Core source identity and text through
loader-error conversion, just as successful parses do.

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

The fixed headings are `Error [jet.Name]:` and `Warning [jet.Name]:`, naming
the diagnostic type. A report with a span includes its source location and
underline; a report without a span omits that block. Every report keeps `Why:`,
`Fix:`, and `More: jet-lang.dev/e/<CODE>` in that order, with optional detail
before `More:`; a diagnostic whose code is its type path has no `More:` line,
and `jet explain web.BadPath` prints its impl's literals with their holes.
Runtime reports keep their code in the heading. Multiple diagnostics are
separated by one blank line. Lints do not block compilation by default;
`#Allow(jet.Name, "why")` silences one lint or library warning inside its
target, and a package may deny a named lint through its lint policy, but a
diagnostic code is never a policy value. JSON and LSP `code` stays the
registered code.

Every human report renders from its row through
`Compiler/JetFoundation/Source/Diagnostics/Render.jet`: `diagnostic_render`,
`diagnostic_render_all`, and the `Stop` line helpers (one of which reads a
`Stop` line back, for `jet test`). Extra lines go in the
diagnostic's detail. No other source file under `Compiler/`, `Core/` or
`Jetpack/` writes a heading such as `Error [E0102]` or `Stop [{code}]`, or a
`jet-lang.dev/e/` link. Runtime reports cannot reach the registry while they
run, so their texts are generated from the rows by
`Tools/agent/gen-runtime-*-messages.mjs`. The stack-overflow report in the
runtime's assembly stays hand-written, because its handler runs on an exhausted
stack. `Tools/agent/DiagnosticRows.jet --check` fails on a hand-built report,
on a generated or assembly copy whose heading, Why or Fix differs from its
row, and on a generated heading or link that names no code.

Package-budget reports retain the authored callable witness's span and source
snapshot, including witnesses in dependency modules. A named witness must not
be rendered against the entry file merely because enforcement runs there.

A default-error entry report uses `Error [E3015]:` with the registry-owned
What, Why, Fix, and More. The error value's own code remains report data rather
than replacing the diagnostic identity. A handled `Err` printed as a value is
not this unhandled-entry report.

In this report, journey locations retain columns supplied by their producer.
Unavailable source must not acquire an invented snippet, caret, path, or zero
column. Terminal escaping is display-only: it does not rewrite the error or
journey data. This does not change standalone journey transport's ordinary
text policy.

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

Borrowed-return repairs teach the provenance spelling `-> View<T> from source`
and a view derived from that source, not shared ownership as a substitute.
Build authority repairs use `--allow=Effect` (for example `--allow=Exec`);
action stderr is decoded from the structured report before terminal rendering,
so its line breaks remain actual lines rather than JSON escapes.

Memory sentries (`R0801`–`R0803`) are program-side faults, not unhandled entry
errors. Their `exit:` metadata is rendered from the same exit-code fact that
the returned runtime report carries: 70 under S36 / D-FAIL-EXIT1. Execution
boundaries deliver that report and preserve its status; they do not classify
the fault from its prose. An unresolved diagnostic row remains a host defect
with status 101. No row speaks for it, so it is not a report: it prints one
`internal error:` line that names the code, with no Why, Fix or More.

A native program's fatal signal is reported the same way under `jet run` and
`jet build`. The handler runs on a per-thread alternate stack, so an exhausted
stack is reported too. A SIGSEGV within the guard region below the stack
pointer is a stack overflow: `Stop [E3012]` names the Jet function and its
source line and exits 70. Any other SIGSEGV, SIGBUS, SIGILL or SIGFPE prints
`Fault [SIGNAME]: invalid memory access at address 0x…` (or the signal's own
reason) and a backtrace of the Jet frames (`name at file:line`, from the
image's location table, `.debug_jetline`, which the compiler writes beside
the DWARF from the same rows). It then re-raises the signal with its default action, so the
status is the signal's (128 + number in a shell). Addresses other than the
faulting one are not printed, because they differ between the two tiers.

A release image (`debug_info: split`) holds no location table, so its report
keeps the first line with the address in place of the name, then
`Build <id> (names and lines: jet crash)` and `Backtrace:` with the faulting
address and the return addresses. `jet crash < report.txt` names them from
the side debug file the build filed under that id in `.jet/symbols` and
prints the report the same program built with `debug_info: true` prints, so
a fault reads alike on every tier (D-RT-DEFECT-REPORT1). A report without the
`Build` line is E2114; a build id with no side file in the store is E2115.

## Machine reports and causes

The canonical machine envelope is `jet.report/v3`. Its `cause` array is
nearest-first and contains `code`, `file`, `line`, `col`, and `span` fields.
Repeated codes are not deduplicated. Every field comes from that cause's own
`DiagnosticCause` and originating source snapshot; unknown fields are `null`
and never inherit the root diagnostic's location. If a source snapshot is
unavailable, the projection must not invent a path, line, column, or `0:0`
position. An explicitly supplied byte span remains unchanged.

A report's `file` fields (the root's, each cause's and each `fix_edits`
entry's) follow one rule for every code (D-REPORT-PATH1). A file under the
folder jet ran in is named relative to that folder. Another file of the
project that folder belongs to (its workspace, repository or package root) is
named with `../` steps from it. Any other file keeps its absolute path. A
report therefore resolves from the folder jet ran in, and it does not carry
the machine's directory layout for a project's own files. LSP keeps absolute
`file://` URIs.

Registry-supplied machine next actions must apply to the actual diagnostic,
not leak a registry example's names. E0104's `no_fix_reason.next` directs the
learner to the called function or method's parameters and argument count;
an arity error for `List.pop`, `String.split` or `Map.get` never points to an
unrelated `show` definition.

Bundle checks retain the owning source snapshot when a later phase reports
against a module-local span. In particular, solved-effect boundary diagnostics
belong to the function's declaring module, not the bundle entry file. The
Jet checker attaches the module's source segments when publishing effect
reports; the reference checker attaches the same origin before merging them.
For a transitive unbounded trait dispatch (E0743), the witness is the actual
dispatch call in the callee's module, even when the effect ceiling belongs to
a caller elsewhere. If that witness has no identifiable source module, the
report remains unlocated instead of borrowing the caller's position.
When both the ceiling and its dispatch witness belong to the entry module,
the entry's existing supplied rendering context is retained unchanged.

## No-OS E3301 contract

E3301 is the registered rule for an OS-dependent API selected for a no-OS
target. The exact rendered snapshot is part of the contract and is kept in
`tests/ui/freestanding_e3301.stderr`:

```text
Error [jet.OsApiOnNoOsTarget]: `files.read` is not available on a no-OS target.
  --> tests/ui/freestanding_e3301.jet:5:11
    |
  5 |     _: fs.read("config.txt")
    |           ^^^^
 Why: No-OS targets have no OS; only `core`-level APIs are available.
 Fix: Embed the data at compile time with `@embed("file")`, or select a hosted target.
More: jet-lang.dev/e/E3301
```

`tests/cross.rs` compiles the same `fs.read` fixture with
`compile_no_os`, checks E3301 and the `no-OS` wording, and compares the
byte-exact rendered output. The registry row is E3301 in
`Compiler/Prelude/Diagnostics.jet`; the checker raises it from the no-OS target
environment in `Compiler/JetSema/Source/Sema/Calls/Core.jet`. Hosted and no-OS
behavior therefore share the same diagnostic renderer while the target rule
remains explicit.

## Explain and website output

`jet explain <CODE>` resolves the same registry rows offline through
[`Explain.jet`](../../Compiler/JetCli/Source/Cli/Explain.jet). The website is another
projection of that data: [`Docs/site/generate.jet`](../site/generate.jet) writes
the error index and renders each code page from registry rows and snapshots.
Neither CLI help nor a website page is an alternate diagnostic catalog.

`jet explain <token>` answers a sigil or operator from the syntax dictionary
([`SyntaxDictionaryData.jet`](../../Compiler/JetCli/Source/Cli/SyntaxDictionaryData.jet)) in
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
4. Run `jet Tools/agent/DiagnosticRows.jet --write`. It regenerates every copy
   of the row: the Foundation registry rows, the repair-coverage baseline
   `tests/diagnostics_coverage_baseline.txt`, and the explain page
   `Docs/site/dist/e/<CODE>/index.html`. Never edit those files by hand;
   `--check` (run by the cheap gates) fails when any of them differs from the
   rows, and the generator refuses a change that adds an active row with
   neither a structured edit nor a `no_fix` reason.
5. Run the focused snapshot and coverage checks, then check
   `jet explain <CODE>`. Test rendered behavior where applicable; do not add a
   persistent Markdown mirror.
   Parser parity proofs also compare byte spans, secondary labels, edits, and
   their applicability and safety grades in `tests/selfhost_parser.rs`; matching
   prose alone does not prove that a machine fix edits the right source.

Coverage checks both directions: each emitted code has one registered row and
snapshot, and each row is emitted or explicitly marked retired or reserved in
the source. Keep one meaning across AOT, `jet run`, the interpreter, LSP,
machine output, and web. Documentation and rendering changes must not mint a
new diagnostic or change compiler semantics.

### Running and blessing snapshots

From the checkout root, `jet test Compiler --profile ui` compares UI and lint
snapshots; a positional filter (`jet test Compiler --profile ui <repo-relative-name>`)
focuses the selection, and `JET_CI_EVIDENCE_DIR=<dir>` writes machine-readable
verdicts to `<dir>/ui.json`. `jet test Compiler --profile ui --diagnostic-coverage`
runs the suite's coverage entry instead of its rows: it checks
independently derived emitted and rendered code sets against the owned
exclusions in `tests/diagnostics_coverage_allowlist.tsv` (lowering-only ceiling: 152).
Blessing that entry (the same command with `-u`)
rewrites the allowlist without its stale rows; it never adds one. The emission
scan reads the `.jet` sources under `Compiler/`, `Jetpack/` and `Core/`.

Plain fixtures run `jet check` and record rendered stderr on failure, or
exactly `(no errors)` on success, even when advisory lints exist. Lint fixtures
run `jet check --verbose`: they must succeed and emit at least one warning.
The lint invocation is admitted only after a same-head comparison against
Cargo's `lint_snapshots` reports zero `.warn` mismatches; advisory differences
are not filtered out.
Only the CLI's final summary/explain envelope and volatile root paths are
normalized; diagnostic bodies are retained. Group directories are recursive;
project entries stop recursion. Fixed companions must check clean, build, and
run on each selected tier. Internal fixture directives without a public CLI
capability are `REFUSED`, never silently converted to ordinary checks.

After reviewing a compiler change, bless only the affected fixture:

```sh
jet test Compiler --profile ui tests/ui/example.jet -u
```

Blessing creates or updates that fixture's snapshot from actual tool output.
A lower diagnostic count is refused unless the reviewed invocation names the
fixture in `--allow-shrink=tests/ui/example.jet`. A sharded profile refuses `-u`.
Expected files are read-only in ordinary runs.


## Latency and runtime-query boundaries

D-DEV3 is a wall-clock promise: a save gives diagnostic feedback in well under
200ms. `Compiler/JetLSP/Tests/Integration/DevLatencyTests.jet` measures `Examples/features/collections/wordcount.jet`
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

## Compiler ownership consumers

Native lowering and code generation detach copyable scalar IDs from owner
projections before requesting write access to that owner. A detached scalar
does not allocate or duplicate an aggregate. This keeps parameter debug binds,
result storage writes, reachability requests, sentry exits, and pattern-cache
block switching within the existing read/write loan contract.

Compiler comparisons against an already-bound optional ID or interpolated
symbol use parenthesized value expressions, not capture patterns. Decision
hashing passes byte ranges as `View<U8>` and projects only runtime-part variants
present in the canonical runtime inventory.

Prepared host Shared transfers validate the borrowed tree before committing
the owner alias. After that commit, adoption updates the original carrier
through write views; it does not copy containers, finalizers, or cleanup roots.
An adoption failure after alias commitment is a consumed failure, never a
rejection that pretends the original Source alias is still active.

Recursive host type-shape growth detaches the needed struct payload at a call
boundary before growing its owner vector. Compiler carriers whose tables move
into a separate program owner restore their empty moved fields before the
remaining carrier is borrowed again.

Boolean values returned by `Never!` helpers consume their checked failure
carrier before entering logical joins.

An internal IR build failure preserves its underlying reason and
source-byte span in the existing ICE report and exits 101 under I2.

An exhaustive value-form Result dispatch checks its subject once, including
when coverage visits an `.Err` arm before an `.Ok` arm. The ordinary condition
walk reuses the already checked carrier and its elaborated expression; coverage
must not repeat ownership effects and manufacture an earlier-loop E0121.
Native Jet already hoists dispatch subjects into one checked compiler local.
The regression restores the original Core service rollback body and executes
an Err-first taking subject with a single-use owner and a call counter.

Converting a checked String failure payload into the canonical Prelude `Err`
record also retains that checked message. The record checker still supplies
private defaults and applies visibility, field-type and view-escape rules;
it must not re-infer a taking call inside the synthesized message field.
Native Jet constructs the default-error record before checking its fields.
The same runtime regression exercises both outer and branch-local single-use
owners in deferred Err arms before the new taking dispatch.
When lowering such a value-if, the Err literal's absent success slot takes the
live sibling's checked carrier type, not the enclosing function's return type.

Hosted Rust emission binds caller-owned return storage before compacting local
names. A return place is not an ordinary owned local that declaration folding
may initialize with an internal reuse marker. Its preserved declaration and
writes pass through the existing return-slot binder, which clears and refills
the caller's buffer without an undeclared `__jet_return_reuse` reference.
The compiler SHA256 hex-builder regression executes populated, empty and
populated-again refills through the generated return-slot entry points.

Hosted Rust derives follow the actual native carrier's capabilities: owned
FileReader and FileWriter streams provide neither Rust Clone nor Debug, even
when their opaque checked type has no generic arguments. Enclosing records
must not gain those structural derives from an empty argument list. Checked
source value hooks, such as Clock's semantic fork and Debug, remain available.
Retained native config and Encode helpers use the canonical Dyn carrier; the
retired DataTree Rust type is not restored as an alias.

Source-hook capability queries snapshot only the type identity, key and span
before a write-access call on the owning emission context. The hook lookup does
not require copying a full type definition or its field/variant lists, and
the checked program rows remain unchanged.

Manifestless script binding searches keep first-match root, entry-file and
import-reference indices. Missing roots or files return normally; missing
references append the existing E0003 diagnostic and continue to the next
admission. These statement exits are outside finite value-loop fallbacks, whose
exhaustion route cannot carry a multi-statement diagnostic block.
The standalone regression
`tests/fixtures/r19_metadata_and_dependency_search.jet` exercises all three metadata
snapshots, unchanged definition payloads, absent roots/files, duplicate
first-match candidates and continuation after a missing reference.
