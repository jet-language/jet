# jet expand --facts <lens> — implementation plan

Card #183. Three ratified decisions name `jet expand --facts` as their
transparency surface, and it does not exist anywhere in `crates/jet-driver`
or `Source/`:

- **D-METHODMACRO1(=A)** — "`jet expand --facts inline` shows every decision"
  (per call site: inlined or not).
- **D-REF-SHORTHAND1(=D)** — "exactly one candidate owner … is inferred and
  `jet expand --facts refs` materializes it"; ratified code comment: "every
  field's resolved owner".
- **D-DYNARRAY1(=A)** — compiler-tracked view owners, implicitly the same
  surface (views ride the stored-ref machinery, so their owner facts are ref
  facts).

Those texts fix the spelling `jet expand --facts <lens>`. What they leave
open — bare `jet expand` meaning, lens listing, output wording — is
owner-facing product surface: **gated on D-EXPANDCLI1** (queued). CLI
skeleton work below that doesn't depend on the open sub-questions (fact
collection, semindex plumbing) can proceed; the printed surface locks only
after ratification.

## Architecture — facts are a sema by-product, one source of truth

Rule: a fact is something sema already proved during the normal check pass.
`expand` never runs a second analysis, never asks rustc (I2/I3), and never
computes anything the compile didn't.

1. **Sema records facts in side-tables during checking** — exact precedent:
   `SemIndexEffectFacts` (`crates/jet-sema/src/Sema/Effects.rs:294`,
   returned by `Bundle.rs` `check_bundle` alongside diagnostics). Add
   `ExpandFacts { inline: Vec<InlineFact>, refs: Vec<RefFact> }` in a new
   `crates/jet-sema/src/Sema/Facts.rs`, threaded through the same bundle
   return path.
   - `InlineFact { callee, call_site: Span, decision: Inlined | NotInlined,
     reason, contract: Hint | Always | None }` — recorded where the
     c7methodmacro sema check (CheckerItems/CheckerInline) decides.
   - `RefFact { field, site: Span, owner, how: Inferred | Labeled }` —
     recorded in `Registration.rs`/`CheckerOwnership.rs` where
     `check_stored_ref_fields` resolves an owner; `.view(range)` call sites
     (D-DYNARRAY1/E2305 analysis) emit the same record shape with
     `field = the view binding`.
2. **jet-semindex carries them** — extend `crates/jet-semindex/src/lib.rs`
   `from_checked(bundle, facts)` to take/store `ExpandFacts`; additive JSON
   fields + `SCHEMA_VERSION` bump 1→2. One index feeds `expand`, `semindex
   --json`, future LSP inlay hints (Blueprint north-star) — no parallel
   fact pipeline (I8).
3. **Driver renders lenses** — new `Source/CmdExpand.rs` modeled line-for-line
   on `Source/CmdImpact.rs` (`open(&abs)` → render text or `to_json()` via the
   global `mode.json` `--json` flag). Wire-up: dispatch arm in
   `Source/main.rs`, `CommandSpec` row in `Source/CLI.rs`, and bespoke flag
   ownership in `jet::CLI::owns_flag_vocabulary`.

## v1 lenses

| lens | facts | sequencing |
|------|-------|------------|
| `refs` | every stored-ref field's resolved owner (`Labeled` today; `Inferred` rows appear when c7refshorthand lands); `.view()` owner rows when c7dynarray lands | ships first — explicit `#Ref(owner)` labels exist today, so the lens has real output immediately |
| `inline` | every call site of an `@Inline`/`@InlineAlways` function: inlined or not + reason | populated by c7methodmacro's sema check; lens registered from day one, prints "no inline contracts in this program" until facts exist |

Lens registry is a static table (name, one-line description, fact renderer)
in `CmdExpand.rs` so `--facts` validation, the lens listing, and future
lenses (`effects`, `layout`, E4 derive expansion) are one edit.

## Output

- **Human text (default):** one line per fact, grouped under a lens header,
  `file:line:col` first so editors can jump. Wording is product copy — final
  strings ride D-EXPANDCLI1's ratified option and get snapshot-tested.

```
refs — resolved owners (2 facts)
  src/index.jet:4:5   Index.hot   owner: pool   (labeled @Ref(pool))
  src/index.jet:9:5   Owner.primary owner: incidents (inferred — sole candidate)
```

- **Machine JSON:** existing global `--json` (`mode.json`), same document the
  semindex schema carries — no second encoder. Stable, versioned via
  `SCHEMA_VERSION`.

## Bare `jet expand`

Open sub-question in D-EXPANDCLI1. Recommended: bare `jet expand <file>` runs
every lens (grouped sections) — show-everything magic default, `--facts`
narrows. Do not implement bare-expand behavior until ratified; until then it
follows whatever the missing-flag path does naturally.

## Diagnostics

- Unknown lens (`--facts spam`): CLI usage error on stderr, exit
  `USER_ERROR`, listing valid lenses with one-line descriptions — same
  surface as `jet impact`'s arg errors (`Source/CmdImpact.rs` precedent).
  Not an E-code: it never reaches `render_diagnostics`.
- File fails to compile: render the ordinary diagnostics via
  `SemIndexError::Load` handling, exactly as `CmdSemIndex.rs` does; exit
  `USER_ERROR`. No new E-codes anywhere in this card, so no `tests/ui`
  snapshots (I4 untouched).
- Unknown flag / typo'd subcommand: existing E2102/E2101 registry paths cover
  it once the `CommandSpec` row exists.

## Tests

- **Crate integration** — new `tests/expand.rs` mirroring `tests/semindex.rs`
  (`jet_semindex::open` over `examples/features/…` fixtures): refs lens
  returns the owner facts for the existing ref-field examples; inline lens is
  empty-but-valid pre-c7methodmacro, asserts real rows after.
- **Driver surface** — `tests/cli/` snapshot fixtures (`completions_*.txt`,
  `man.txt`) change when the `CommandSpec` row lands; re-bless deliberately.
- **JSON stability** — schema-shape asserts on the v2 fields, same style as
  `semindex_hello_json_shape`.
- **Golden/ui** — untouched (no new syntax, no new E-codes, no fmt work: the
  command adds zero parseable surface, so no formatter round-trip needed).
- Targeted `cargo test --test expand` while iterating; full suite once at
  the end.

## Docs

- `docs/spec/syntax-decisions.md` tooling section: one entry alongside
  D-SEMINDEX1/D-IMPACT1 once D-EXPANDCLI1 ratifies.
- The three ratified decisions' plan sections
  (`jai-adoptions.md` §1 step "jet expand --facts inline", §8 step 4): point
  them here instead of "does not exist".

## Exit criteria

- [x] D-EXPANDCLI1 ratified; printed surface matches the chosen option
      (card #183, built 2026-07-03).
- [x] Ref facts returned from `check_bundle` beside `SemIndexEffectFacts` —
      landed as an additive `refs: Vec<RefFact>` field on
      `SemIndexEffectFacts` itself (`crates/jet-sema/src/Sema/Facts.rs`)
      rather than a separate `ExpandFacts` type; same "no second analysis
      pass" shape the plan called for, fewer moving parts. `inline` needed no
      side table at all — `Source/CmdExpand.rs` reads `Func::is_inline`/
      `is_inline_always` straight off the already-checked bundle.
- [ ] semindex schema v2 carries the facts; `jet semindex --json` unchanged
      except additive fields + version. **Not done this pass** — card #183's
      ratified floor scope was the CLI surface + the two lenses, plain text
      only (no `--json` for `expand` yet); wiring `refs` into the semindex
      JSON document for LSP inlay hints is follow-on work, not blocking.
- [x] `jet expand --facts refs <file>` prints resolved owners — both
      inferred (sole in-scope candidate) and explicitly `#Ref(label)`ed —
      verified against `examples/features/memory/ref_owner.jet` and
      `ref_field.jet`. No `--json` rendering (plain text only, per the
      ratified surface).
- [x] `inline` lens registered; populated immediately (c7methodmacro/
      D-METHODMACRO1 already shipped) — verified against
      `examples/features/contracts/inline_contracts.jet`.
- [x] Unknown lens lists valid lenses (exit 1); broken file shows ordinary
      diagnostics (exit 1, no facts printed).
- [x] CLI coverage green: `tests/cli.rs` `expand_inline_golden` /
      `expand_refs_golden` / `expand_all_golden` / `expand_unknown_lens_golden`
      / `expand_missing_file_is_user_error` /
      `expand_compile_error_reports_ordinary_diagnostics`, fixture
      `tests/fixtures/expand_facts.jet` (no separate `tests/expand.rs` —
      the card brief directed CLI-level coverage instead). `completions_*`/
      `man.txt`/`question_mark_help.txt` re-blessed for the new command.
      `--test cli`, `--test decisions`, `--test truthfulness`, `--test
      semindex`, `--test impact`, `--test ownership`, `--test
      ref_soundness_fuzz`, `-p jet-sema` all green; full suite not run
      (per standing instruction — targeted tests only).
