# Proposal decisions and lifecycle

Status: current preservation note for cleanup #2932. This note records why
completed proposal sources leave `docs/proposals`, where their conclusions now
live, and which designs remain active. It is not a second syntax authority.
The ratified syntax authority is `docs/spec/syntax-decisions.md`.

## Authority and source identity

The cleanup source-only tree is the recovery authority:

- ref: `cleanup-source-2026-09-05-114107`
- tree: `6875b5e07a55900a630f280c9c88edbfca2094b1`
- kind: source-only tree, not a release or build commit
- exact recovery form: `git show cleanup-source-2026-09-05-114107:docs/proposals/<name>.md`

The whole-language laws are in the `## Whole-language audit slate
(2026-09-02)` section of `docs/spec/syntax-decisions.md`. The decisions in
that section are ratified decisions, not proof that their implementation cards
are complete. The implementation phase and card state below are kept separate.
Tower was read through the repository's read-only `tower.mjs` commands. This
worker made no Tower mutation; transfers remain with the integration owner.

The following identities were checked before lifecycle edits. The bytes and
SHA-1 object IDs are the source identities, not identities of a rebuilt or
reformatted document.

| source | bytes | source object ID | lifecycle |
|---|---:|---|---|
| `automatic-build-optimization.md` | 87486 | `71c88c8c70da307b9fb0e2d466740df380ea5b82` | keep active |
| `claims-one-ladder.md` | 12054 | `63fa609a1e8b630c1ce629433d2eeda6bfa49650` | delete; preserve in this note and the canonical slate |
| `developer-experience.md` | 63341 | `b7478acce7b1fe9df1128231616051411356b804` | move intact to `docs/audits/` |
| `dogfood-jet-experience-5-of-5.md` | 21836 | `86d79219f3f6a328b5c89c89651fa45612e30427` | move intact to `docs/audits/` |
| `ecosystem-shape.md` | 72690 | `387b8638c81ebb1f4f786cf300797878225ac934` | move intact to `docs/audits/` |
| `hardening-rig.md` | 26020 | `c515f1d0059a7e0d91ff63ac81dbc1aff1c53e68` | move intact to `docs/audits/` |
| `open-tables.md` | 9625 | `2cbf20dcde4048eb8047326f5ee38d0cea0c1586` | delete; preserve in this note and the canonical slate |
| `records-one-receipt.md` | 12665 | `6c959e6c6db5f699d20fc032366eb2af79dd86a8` | delete; preserve in this note and the canonical slate |
| `rights-one-row.md` | 12424 | `8927d758239ca9ac7c7138a0a229bea507cd823f` | delete; preserve in this note and the canonical slate |
| `shapes-one-fact.md` | 12023 | `10180ed56c30187b41ded12330c9596600b07959` | delete; preserve in this note and the canonical slate |
| `stored-invariant-facts.md` | 13528 | `ce5b4600c0f60a2d1625c8e7a5faac61379d8b37` | keep unresolved design note |
| `streamline-one-repo.md` | 430074 | `8655c657bb0027f0a50252164594337099d6b2b5` | delete completed plan; preserve counts and outcomes here |
| `structure-program-is-a-value.md` | 37498 | `f3c8942b66c29c116da1bc7dfce227b2699d8e1f` | move intact to `docs/audits/` |
| `syntax-lexical-space.md` | 11885 | `3daf63b9d1a490a8dbae4582bc115ec56f0f9bfc` | delete; preserve in this note and the canonical slate |
| `tier-live-dev.md` | 10090 | `df74f77272834f68ec9a2d14b1778afa3dc9e5b2` | delete; preserve in this note and the canonical slate |
| `transactional-rollback-regions.md` | 14529 | `7745476d879ad3ab7fb1c1b5138f8caccaa2c6b2` | keep parked draft |
| `verdict-loop.md` | 12177 | `9cc6ecd434ab9412ca1cbf70fc53c523cd74058e` | delete; preserve in this note and the canonical slate |
| `whole-language-frame.md` | 74750 | `af9a33a9437561f805618ec7d6ecdd6a54877e23` | move intact to `docs/audits/` |
| `yielding-loops.md` | 5277 | `650cfd53156010e81ec79c81868d9c735e30e8c2` | delete completed proposal; preserve in this note and current spec |

## Ratified whole-language slate

All rows in this section were ratified on 2026-09-02. The source proposals
said that their illustrative transcripts did not run and that their designs
were not implemented. The current canonical law is retained, while the
implementation-card state remains explicit.

### Ledger and rights

- **D-LEDGER1 = D, #2501 (`ready`, Main).** The three stores are separate by
  lifetime: static fact-kind schema, checked-program fact instances, and run
  evidence. One typed query contract reads all three for check, test, prove,
  inspect, dev, and receipts. Evidence grade belongs to claims only; rights
  retain gate kind and provenance. Time is a query value, not a schema column.
  Static registration rows remain `&'static`; `FactRegistry` does not get
  folded into `Registry.rs`. A parity test must reproduce every existing tool
  answer through the contract before a tool switches. This keeps a single
  query seam without falsely making all stores one lifetime or one storage
  implementation.
- **D-RIGHTS1 = C, D-RIGHTS-CLI1 = B, D-RIGHTS-DIAG1 = B, #2501 (`ready`,
  Main).** One rights-row walker serves callable rows, block scopes, package
  authority, replayable lowering, comptime lowering, and invocation checks.
  Purity is the empty row; `#Replayable` lowers to a row excluding Time, Rand,
  Net, and IO; comptime holds a Mem-only row. The transaction checker stays
  separate because an irreversible effect owes an undo and `on_commit` and
  `#Undo` cannot be represented by an ordinary rights row. `@irreversible` is
  a declared fact for that checker. A differential matrix must show old and
  new walkers give identical purity, replayable, comptime, and invocation
  verdicts before deletion.
- Invocation spelling is `--allow=<effects>` and `--deny=<effects>`. Roots and
  leaves are accepted; names expand to leaves first and deny wins. Naming one
  effect in both flags is a usage error. Jet consumes these flags before the
  program's flags, while `--` passes a literal program flag. The twenty old
  flags stop with their exact replacements; shell files are not auto-edited.
  `jet eval --pure` stays as a spelling and lowers to the empty row.
- Every rights denial uses one frame containing call, callable chain, scope
  chain, nearest grantable scope, and safety-classed repairs. Codes remain
  distinct, and `denial_kind` distinguishes package, block, purity, and
  transaction cases. A package grant is `needs-review`, never an automatic
  edit. E0750 remains its own vocabulary frame because its repair is the
  effect name. The own-code authority gap remains separately carded as #2511.
  Rejected alternatives were one walker including transactions, retaining
  every private walker, one universal denial code, and repeatable flags; each
  either loses undo semantics, preserves drift, loses tool discrimination, or
  adds a second command grammar.

### Claims and grades

- **D-CLAIM1 = C, D-GRADE1 = D, D-GRADE-POLICY1 = A, #2502 (`ready`, Main).**
  `jet test` runs examples, doctests, property tests, and effect-free contract
  inputs generated from `#Pre`, with a default budget of 1,000 inputs per
  contract. `jet prove` remains the ratified umbrella; only
  `--lens solver` enables solver evidence, and it proves the implication
  `precondition -> postcondition`, never the precondition alone. Both commands
  write one evidence report. `jet fuzz` folds into
  `jet test --grade=generated --iterations=N`.
- Each claim keeps every evidence record. A record is producer, outcome, and
  count. The report may summarize `proved`, `generated(n)`, `examples(n)`,
  `checked`, or `unchecked`, with the narrow order
  `proved > generated(n) > examples(n) > checked > unchecked` used only for a
  package floor. Compiler facts are not claims; gates such as `#Unsafe` retain
  their own rights kind and never receive a claim grade. `policy.claims.min`
  names a count-bearing floor and tightens only; there is no maximum.
- Generation is refused, not guessed, for effectful callables, preconditions
  naming another function, or parameter types without a generator. The report
  says `not generated` and why. A failed generated input keeps the smallest
  input and replays it first. `#Pre`, `#Post`, written examples, property
  tests, doctests, `jet prove`, `.jetproof`, measurements, and budgets remain.
  Card #2509 (the contract example failing on every tier) is the prerequisite
  defect before higher evidence can be trusted.
- The rejected one-command option would silently amend the proof and replay
  slate. Keeping all reports independent would preserve the duplicate answer.
  A total ladder including `static` and `assumed` was rejected because static
  is compiler provenance, assumed hides gate kind, and generated and example
  evidence are not totally ordered.

### Shapes and the live tier

- **D-SHAPE-ONE1 = A, D-SHAPE-PROJECT1 = A, #2503 (`ready`, Main); #2510 is
  `done`.** The final decision is deliberately recorded as **A**, although the
  source proposal recommended C. Every format reads the same field facts, and
  database rows and FFI layout join now. The proposal's rival-review objection
  is preserved: a field diff alone has no table identity or transaction
  contract, and FFI previously shared only layout and skip. The ratified A
  outcome resolves that objection at the decision level; it must not be
  silently rewritten back to C.
- `args.decode<T>()` works anywhere through the existing `core.args` builder,
  not a second parser. `env.decode<T>()` remains unchanged. `Config.merge`
  applies command value, then environment value, then field default; a present
  invalid value is `FieldError`, never absence. Help and diagnostics are byte
  identical to `fn run(args: T)`. One shape fact owns defaults, skips, docs,
  short names, discriminants, per-format renames, and published-schema
  migration; unsupported projections ignore a fact while retaining it on the
  shape. The second JSON tree and closed format enum are removed from the
  design. Database migration still needs its table and transaction identity
  contract; this decision does not invent one.
- **D-LIVE1 = A, D-LIVE-PERSIST1 = A, #2504 (`ready`, Main).** Saving swaps
  changed functions by default. `#Persist` values survive. A changed
  `#PublishedSchema` type migrates a resident value through the same step
  functions used by decode when a migration covers it; otherwise the ratified
  announced restart occurs and names the migration to write. A renamed pinned
  binding resets its state and says so. Every save prints swapped functions,
  kept values, migrations, or restart. `jet dev --restart` and
  `dev: .{ swap: false }` refuse swapping. A long-lived edit test with callers
  mid-execution on JIT and interpreter engines is a precondition before the
  default flips. The rejected alternatives reset `#Persist` state or retain
  flag/auto-detection behavior.

### Verdict and records

- **D-VERDICT-LOOP1 = D, D-CLI-ONE1 = A, #2505 (`ready`, Main).** The source
  census counted 901 report rows, 762 active rows, and 83 rows with
  `fix_edits`. Each row must eventually have either a typed `no_fix_reason` or
  edits. The reason is mutually exclusive with edits and has kind
  `behavior`, `design`, or `ambiguous`, plus a reviewed next action or link.
  Coverage baselines are per plane and never decrease; edit coverage and
  reason coverage remain separate. A new uncovered row fails the registry,
  and a plane closes only when all rows are covered.
- All `--json` commands, including infrastructure failures, use one
  `jet.status/v1` object around the ratified `jet.report/v2` rows. The five
  safety classes and one-object-per-line rule remain. `jet fix` applies only
  safe classes by default, and rights grants remain skipped unless reviewed.
  `jet inspect` has one ledger view per plane (`types`, `rights`, `claims`,
  `shapes`, `structure`, `build`, `gates`) at package, file, live, and replay
  scopes. Six ledger-view names retire (`facts`, `authority`, `unsafe`,
  `guarantees`, `report`, `dossier`); task actions keep their existing names,
  and help maps old names to new homes for one edition. The global guard that
  would block on the 679 uncovered rows, a no-coverage guard, and a second
  `jet ledger` command were rejected.
- **D-RECORD1 = B, D-RECORD-SPELL1 = D, D-TIMETRAVEL2 = B, #2506 (`ready`,
  Main).** Artifacts retain their ratified bytes and codecs. One
  `.jet/records/index.jsonl` uses target input SHA-256, tool, and engine as a
  shared identity and links consumed and produced records. `jet dev` captures
  the safe clock-only replay by default within 256 MB and 200 records per
  project; release never captures without the flag. Sensitive capture keeps
  typed consent, a digest over target/authorities/hosts/streams/limit/
  disclosures/destination, mode `0600`, and the non-TTY skip message. One
  reload is at most one artifact. `jet build --verify` rebuilds from receipt
  inputs and reports identical bytes or the first differing input. The trust
  store remains its own store and is only listed by the index.
- The current spelling is `jet prove TARGET --save ARTIFACT`,
  `.jet/records/saved/<artifact_id>`, and `--unsave <id>`. A saved claim uses a
  stable function/kind/ordinal anchor, replays before doctests, and reports
  passed, failed, diverged (E3623), or vacuous with the next action. Two saves
  of one artifact are one claim. The source proposal used the stale spellings
  `--keep`, `--unkeep`, and `.jet/records/kept`; those spellings are corrected
  here to the canonical ratified law rather than preserved as current API.
- Backward stepping is a deterministic replay jump. `back N` is a thin verb
  over jump. It is gated on facts in the repository: every replay fixture in
  `tests/debug.rs` must jump to every event with byte-identical locals across
  repeated jumps and both adapters, and `jet debug` must pass adapter
  conformance. Cards #2461 and #2463 own the primitive and reverse-step
  design. Until those facts hold, replay is forward-only through
  `jet prove --replay`. The source proposal's earlier gate requiring a shipped
  release and a divergence rate is stale and is not the current law.
- The rejected record alternatives were one new container (which reopens
  closed proof/replay codecs), links inside every artifact (which changes
  artifact identity), or no default capture. A new top-level replay command
  and `jet run --replay` were rejected because `jet prove --replay` is the
  ratified consumer. Permanent backward stepping without evidence was rejected.

### Open tables and lexical space

- **D-OPENTABLE1 = D, #2507 (`ready`, Main).** Generate a table from a Jet twin
  when one exists: effect roots/leaves, marker sites, fact reads, encoding
  formats, taint sinks, transaction irreversibility, Core dispatch and
  ambient routes, and the export classifier. Tables with no twin keep their
  one Rust home under a guard: the lexer keyword map, CLI inventory, report
  enums, and tier list. `@irreversible` is declared on `FS.Write`, `Net`,
  `Exec`, and `FFI`. A census must account for all 622 dispatcher rows that
  cannot be derived; every exception becomes a declared fact or a card before
  switching. The bootstrap order is Syntax first, effects/reservations,
  Core routes/modules, then derives. `core.tasks` is the spelling; the
  `core.task` alias dies. The proposal also recorded 145 ambient routes and
  34 documented Core domains without dispatcher rows. Generating every table
  was rejected as a lexer bootstrap cycle; generating only two tables leaves
  drift; fixing aliases by hand does not enforce one home. The phantom
  `KW_SWITCH`/`Stmt::Switch` defect is separately carded as #2512.
- **D-RAWSTR1 = A, D-TRAILCOMMA1 = A, D-SEMI1 = A,
  D-MARKERSHAPE1 = B, D-MARKERARGS1 = A, #2508 (`ready`, Main).** Backticks
  are raw ordinary `String` literals. A maximal opening run of N backticks
  closes at the next maximal run of exactly N; other runs are content.
  Backslashes, braces, quotes, indentation, line endings, and CRLF bytes are
  literal; no interpolation or escape runs. If non-all-space content starts
  and ends with an ASCII space, one space is trimmed from each end. The empty
  string remains `""`.
- Every comma list accepts a trailing comma, and `jet fmt` writes it for a
  multiline list and removes it for a one-line list; a trailing comment makes
  the form multiline. Explicit semicolons produce the existing E0373 with a
  behavior-preserving line-break edit, then parsing resumes at the same
  boundary. A corpus census is required before enforcement. The marker stack
  law stands: one bare rule, one `#[A, B]` list for several, E0999 plus the
  canonical rewrite for a one-item list or adjacent bare stack. `#HTML` now
  takes checked `Path`; `jet fix` rewrites `#HTML("x")` to
  `#HTML(Path{"x"})`.
- The lexical audit measured 230 lexical mechanisms and 79 silhouette rows.
  It corrected earlier false claims: `#Every("03:00")` is compile-time
  checked, `Test`/`Todo`/`Pure` are contextual identifiers, and the phantom
  switch is a defect rather than a new syntax decision. The rejected raw
  alternatives (`r"..."`, hash-prefixed raw strings, and `Raw{...}`),
  layout-only comma rules, lint-only semicolon enforcement, and bare-marker
  stacking remain recorded as rejected alternatives.

## Completed non-report dispositions

### Collecting loops

`yielding-loops.md` was ratified on 2026-07-26 by
D-ARROW-CONTROL1=A, D-LOOPEVAL1=A, D-LOOPSTATE1=A, and D-COMPREHENSION1=A.
The current law is one `loop` controller: effect-only loops have no arrow;
finite source loops use `->` and eagerly return `List<T>` in iteration order;
braces only group a multiline body. Source clauses nest left to right, guards
filter, `next` omits an item, ordinary loop values come from compatible break
payloads, collecting loops reject payload breaks, dot exits are retired, and
ownership, effects, and failures remain visible. `break(name)` and
`next(name)` target an enclosing loop. Required diagnostics cover non-finite
yields, unit or missing items, incompatible types, invalid payloads, missing
loop targets, retired dot exits, statement/value misuse, and ownership/failure
violations. The canonical homes are the collecting-loop entry in
`docs/spec/vocabulary.md` and the existing loop laws in
`docs/spec/syntax-decisions.md`. The proposal source can therefore be
retired after this note records its rule and migration details.

### Streamline one repository

The completed plan selected D-EXAMPLES-SUITES1=A, D-DOCS-GC1=A,
D-REPO-LAYOUT1=C, and D-DOCS-SSOT1=A. Card #2233 (`c06nrxwx`) is `done`,
completed 2026-08-27. Its measured inventory was 681 `.jet` files, with 488
mapped to 52 suites, 185 retained special entries, and 8 duplicate/deleted
entries; the target inventory was 237 entries. The docs census was 131 files
and 133,567 lines: 64 kept and 67 removed (37 direct deletes and 30
fold-then-delete sources). The completed docs GC recorded 67/67 removals or
folds, and existing examples were left intact while suites were added. The
proposal listed 18 exact ignore patterns, but the ratified C result means no
layout or `.gitignore` change is implied here. The original plan is deleted
only after these counts, gate decisions, and card identity are preserved.

## Protected final reports moved intact

These are reports or audits, not disposable working proposals. Their contents
are not distilled into this note and must remain byte-for-byte intact at their
new paths:

- `dogfood-jet-experience-5-of-5.md` ->
  `docs/audits/dogfood-jet-experience-5-of-5.md`; it points to the dated
  dogfood source report, parent #2386, its finding ledger, and hostile
  closeout #2394.
- `whole-language-frame.md` -> `docs/audits/whole-language-frame.md`; its
  eight-element audit, alternatives, and disposition table are the complete
  cross-domain report.
- `structure-program-is-a-value.md` ->
  `docs/audits/structure-program-is-a-value.md`; its six D-STRUCT ballots and
  evidence are preserved intact. Card #2052 (`c0b6kkbv`) is `done`, as are
  defects #2053, #2054, and #2055.
- `developer-experience.md` -> `docs/audits/developer-experience.md`; the
  nine-domain census, census JSON references, decision table, and active DX
  work are preserved intact. Representative decisions are ratified; card
  #2423 remains `building` while the other listed DX implementation cards are
  `done`.
- `ecosystem-shape.md` -> `docs/audits/ecosystem-shape.md`; all 19 rows are
  decided (`19 decided`, `0 open`). Representative implementation cards
  #532, #587, #609, and #610 are `done`.
- `hardening-rig.md` -> `docs/audits/hardening-rig.md`; the verified gate and
  dashboard report is preserved intact. D-HARDENING-GATE1=A and card #2339
  (`c00acag6`) are `done`; its owner gate remains `14d / 10M / 100 / 8`.

The destination files retain the source identities listed above. Their
relative links are not rewritten by this worker; the integration owner owns
outside-reference migration.

## Active proposal survivors

- `automatic-build-optimization.md` remains an active proposal. Its status
  says nothing is implemented. Parent card #2514 is `planning`; related
  decision/card work includes #2528 (`deciding`) and the ready/done cards in
  its own report. The assets
  `docs/proposals/automatic-build-optimization/mockups/terminal.html` and
  `report.html` remain untouched.
- `stored-invariant-facts.md` remains an unresolved design note. Card #2140
  (`c0earl6i`) is `done` only as a design note: it records no code, no
  ratified syntax, and no selected spelling. Predicate-plane propagation,
  negative cases, erasure, and tier-parity questions remain owner work.
- `transactional-rollback-regions.md` remains a parked draft. It is explicitly
  not a ballot: ordinary `?` never rolls back, `#Transact` is a separate rail,
  and open questions remain about scope, `^T`, spelling, values, and effects.
  Its unposted D-TXN draft and source evidence remain in place.

No active proposal was deleted because it was old, dirty, or mentioned by a
report. Newly added proposal files and sibling-owned document trees are not
part of this lifecycle pass.
