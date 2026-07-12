# Marker family migration (`#` → `@` two-plane law)

Card: **c7markerfamily** (num 167). Epoch 3, sidequest.
Ratified 2026-07-01: **D-MARKER-FAMILY1=B**, **D-CONTRACTCASE1=A**, **D-MARKERMOVE1=B**.

Binding record (read these, do not re-derive design):
- `docs/spec/syntax-decisions.md` → section "Marker family, CLI flags, syntax wave
  (ratified 2026-07-01)" (lines ~2539–2663) **and** the Ratified table rows dated
  2026-07-01 for D-MARKER-FAMILY1 / D-CAPBUNDLE1 / D-PREPOST1 / D-PERSIST1. These
  rows are the law; this plan is the execution of them.
- `.tower/tower.json` decisions `D-MARKER-FAMILY1`, `D-CONTRACTCASE1`,
  `D-MARKERMOVE1` (all `outcome`/`status` = ratified). **Never edit tower.json.**

Invariants in play: **I7** (every user-typeable sigil lives in `Syntax.rs` with a
decision ID — this migration is mostly that file plus plumbing), **I4** (every new
diagnostic needs a code + what/why/fix + a `tests/ui` snapshot), **I8** (one way to
mean it — the whole point of the law), **R3** (single surface: renaming is a
`Syntax.rs` change + snapshot re-bless), **I5** (examples are the spec).

> Examples are being reorganized into topic dirs concurrently. Every path below
> under `examples/features/**` is illustrative — **re-grep at execution time**,
> reference examples by topic/name not number.

---

## 1. The law (what B decided)

Two marker planes, eye-checkable from the first character:

- `@` **precedes a declaration and states a checkable contract about it** — a
  promise (`@Pure`, `@MustUse`, `@Codable`). `@` **never** appears inside a type or
  expression.
- `#` **instructs the compiler** — changes what compiles, when it runs, what is
  legal in a region, or supplies a compile-time value (`#Unsafe`, `#Test`, `#(Fs)`,
  `[T#N]`, `pkg#1.2.3`, `#Caller()`). `#` **may** appear in type/expression position.
- `$` stays splice-only (unchanged, D-CTMARKER1).

Casing (**D-CONTRACTCASE1=A**): **PascalCase everywhere on the `@` plane**, extending
D-CASING1's `#`-plane rule to `@`. One casing rule for the whole language. Markers
moving from `#` keep their exact PascalCase spelling; the already-registered lowercase
`@` constants (`pre`/`post`/`persist`/bundles) get recased to PascalCase.

Loop-label suffix `@` (`outer@ loop`, D-LOOPLABEL2) is a **different grammatical slot**
(suffix on a name) and is untouched. Contract `@` is a **prefix before a declaration**;
the two never collide.

Current state of the code (verified): the lexer already tokenizes `@` as `TokKind::At`
(`crates/jet-lexer/src/Lexer/Scan.rs:131-132`) — **no lexer change needed**. The
`CONTRACT_*` constants exist in `Syntax.rs` but are **unused**: nothing parses the `@`
plane yet, and the parser currently emits a teaching error `"attributes use #, not @"`
for any `@` in item position (`Parser/Items.rs:771`, `Parser/Statements.rs:1086`). So
this card **builds the `@` contract plane from scratch** in the parser + formatter +
sema, then moves the marker set onto it. It is not a pure rename.

---

## 2. Marker classification (the single source of truth)

Register a classifier in `Syntax.rs` — two `pub const &[&str]` slices plus predicate
fns `is_contract_marker(name)` / `is_directive_marker(name)` — so every dispatch site
(parser, formatter, sema, LSP) asks one place which plane a name belongs to. This is
the I7/R3 chokepoint.

### 2a. Moves `#` → `@` (D-MARKERMOVE1=B), all PascalCase

| # spelling | `@` spelling | `Syntax.rs` const | Decision | Position |
|---|---|---|---|---|
| `#Pure` | `@Pure` | `KW_PURE` (`"Pure"`) | S60 / D-EFF2 | fn / trait-method decl **(see §4 gate G1 — type-position use)** |
| `#MustUse` | `@MustUse` | `ATTR_MUST_USE` (`"MustUse"`) | D-MUSTUSE1 | type / fn / method decl |
| `#Codable` | `@Codable` | `ATTR_CODABLE` (`"Codable"`) | D-SERDE4 | struct / enum decl |
| `#Encode` | `@Encode` | `ATTR_ENCODE` (`"Encode"`) | D-SERDE4 | struct / enum decl |
| `#Decode` | `@Decode` | `ATTR_DECODE` (`"Decode"`) | D-SERDE4 | struct / enum decl |
| standalone maturity markers | `#Meta(maturity: .Experimental | .Tested | .Hardened)` | `META_FIELD_MATURITY` | D-MARK-META1 | fn decl |
| `#PublishedSchema` | `@PublishedSchema` | `ATTR_PUBLISHED_SCHEMA` | D-MIGRATE1 | struct decl |
| `#Redact` | `@Redact` | `ATTR_REDACT` (`"Redact"`) | D-DEBUG-REDACT | field decl |
| `#Numeric` | `@Numeric` | `ATTR_NUMERIC` (`"Numeric"`) | D-DIST3 | distinct-type decl **(merges — see §3)** |

### 2b. Recase in place (already `@`, never shipped as `#`, so **no teaching error**)

These constants exist lowercase and unused; recase the VALUE to PascalCase. Feature
sema is separate cards (c8syntaxwave); this card only fixes the spelling + registry.

| const | old value | new value | decision |
|---|---|---|---|
| `CONTRACT_PRE` | `pre` | `Pre` | D-PREPOST1 |
| `CONTRACT_POST` | `post` | `Post` | D-PREPOST1 |
| `CONTRACT_PERSIST` | `persist` | `Persist` | D-PERSIST1 |
| `CONTRACT_BUNDLE_NUMERIC` | `numeric` | `Numeric` | D-CAPBUNDLE1 |
| `CONTRACT_BUNDLE_COMPARABLE` | `comparable` | `Comparable` | D-CAPBUNDLE1 |
| `CONTRACT_BUNDLE_PRINTABLE` | `printable` | `Printable` | D-CAPBUNDLE1 |
| `CONTRACT_BUNDLE_CODABLE_AS_BASE` | `codable_as_base` | `CodableAsBase` | D-CAPBUNDLE1 |

### 2c. Stays on `#` (directives — do **not** touch)

`#Unsafe` `#Impure` `#Reactive` `#Test` `#Bench` `#Todo` `#Tainted` `#Sanitizer`
`#State` `#Transition` `#Caps` `#Grant` `#Transact` `#Target` `#Wasm` `#Js`
`#WasmExport` `#Html` `#Ref` `#UnitFamily` `#SingleUse` `#Layout`
`#Suppress` `#Extern` `#Bindgen` `#Caller` `#(effect …)`.
(`#Uninit` retired by D-UNINIT-SENTINEL1 — spelling moved to the `uninit`
contextual keyword, `name: Type := uninit`; the old marker is now a hard
parse error, E0426.)

Serde **field + container** markers stay `#` (D-MARKERMOVE1=B: "wire-format machinery,
not promises"): `#Rename` `#Skip` `#Default` `#Flatten` `#RenameAll`
`#DenyUnknownFields` `#Tag` `#Untagged`.

---

## 3. `#Numeric` / `@numeric` reconciliation (assigned to this card)

D-CAPBUNDLE1 shipped an `@numeric` bundle and D-DIST3 shipped a `#Numeric` marker that
do the **same job** (grant same-type arithmetic to a nominal/distinct type). D-MARKERMOVE1=B
resolves the deliberate overlap: **they merge into one spelling, `@Numeric`.** After this
card there is exactly one numeric-capability marker (I8). Concretely:
- Fold `ATTR_NUMERIC` and `CONTRACT_BUNDLE_NUMERIC` to a single constant valued `"Numeric"`
  on the `@` plane (keep one const, delete/redirect the other; update all referents).
- The distinct-type parse path (`Parser/Items.rs:3748`, "optional `#Numeric` attribute")
  and the bundle path must land on the same `@Numeric` recognition.
- Sema arithmetic gating (`Sema/CheckerInfer/binary.rs`, `Sema/Bundle.rs`) must read the
  merged marker; no behavior change, one spelling.

---

## 4. Gates / ambiguities — resolve before writing parser code

G1 and G3 were raised as ballots and are now **ratified 2026-07-02** — the outcomes
below are binding. G2 and G4 keep their defensible defaults; implement and flag in
the card log.

**G1 — RATIFIED: D-MARKERMOVE2=B (whole-move).** `@Pure` is one spelling everywhere,
including the single type-position callback-bound slot: `fn map(items: [Int],
f: @Pure fn(Int) -> Int)`. The plane law gains exactly one carve-out — a contract
marker may prefix a function TYPE to state a bound; "declarations only" reads
"declarations, plus contract bounds on function types". `#Pure` in type position
(`Parser/Types.rs:39,361,741`; `Sema/Effects.rs:534`) migrates with the plane and the
old spelling gets the E0062 teaching error like every other moved contract. Update
E0742/E0745/E0747 message text to the `@Pure` spelling in the same phase.

**G2 — bracket-list form across planes.** Today `#[Codable, RenameAll(camel)]` mixes a
derive (→`@`) and a serde attribute (stays `#`) in one group, and E0999 forbids stacked
`#[…]` lines. Post-move they split: `@[Codable]` + `#[RenameAll(camel)]`. D-MARKER-FAMILY1
option B's illustrative code wrote `@[Codable, RenameAll(Camel)]` (RenameAll under `@`) —
this **contradicts** the binding move list, which keeps serde on `#`. Treat the move list
as binding: serde stays `#`. Therefore the parser must (i) support an `@[A, B]` bracket
group for contract derives, and (ii) **allow an `@[…]` group and a `#[…]` group stacked
on the same declaration** (they are different planes — E0999 must not fire across planes).
Default: implement both; keep E0999 firing only for two `#[…]` lines or two `@[…]` lines.

**G3 — RATIFIED: D-MARKERMOVE3=B (all built-ins move; user derives stay `#`).**
`@Debug`, `@Summarize`, `@Comparable` join `@Codable`/`@Encode`/`@Decode` on the
contract plane as capability promises. User derives (`derive T.Wire { … }` bodies,
applied as `#[Wire]`) remain `#` generation machinery — the built-in/user line IS the
plane line. `split_type_markers` (`Parser/Items.rs:3253`) therefore needs a fixed
built-in contract-derive name set (the six above) routed to `@`, with "any other
name" still parsing as a `#` user derive. Old `#[Debug]`-style spellings of the six
get the E0062 teaching error. Add the three extra markers to every phase's move
tables (registry, parser, prelude, examples, snapshots, docs).

**G4 — D-CLIFLAG1 markers.** D-CLIFLAG1 (card c7cliflag, blocked on this card) mints a
struct-level CLI-derive marker and a field-level doc marker whose spelling "rides"
D-CONTRACTCASE1/D-MARKERMOVE1. Their plane (`@`) and casing (Pascal) are now fixed:
`@Cli`, `@Doc`. **Do not implement the CLI feature here** (separate card), but register the
two `Syntax.rs` constants (`CONTRACT_CLI = "Cli"`, `CONTRACT_DOC = "Doc"`) on the contract
plane so c7cliflag builds against a fixed name. No teaching error (never shipped).

---

## 5. Teaching errors (house pattern: clean break + teaching diagnostic)

Model after `E0053` (`pure` → `#Pure`, `Parser/Items.rs:1154`), `E0059`
(`sanitizer` → `#Sanitizer`), `E0320`/`E2714`. Same four-part shape, `Why:`/`Fix:` voice,
sentence capitalization (`docs/spec/diagnostics.md` "The contract"). Both directions of the
law must teach.

Propose **two parametrized parse-stage codes** (free block confirmed: E0060 taken by C-FFI,
E0061/E0069 taken; use **E0062**, **E0063**):

**E0062** — a **contract** marker written with `#`. Fires when any name in §2a is seen
after `#` (or inside `#[…]`).
```
Error [E0062]: `#Pure` states a contract — write it with `@`, not `#`
 Why: `@` marks a promise about the declaration below it (`@Pure`, `@MustUse`,
      `@Codable`); `#` is for compiler directives (`#Unsafe`, `#Test`). One glance
      at the first character tells a reader which it is (D-MARKER-FAMILY1).
 Fix: write `@Pure fn …` (`@` + the same PascalCase name).
```
Parametrize the name (`@{name}` / `#{name}`) so one code covers all eleven moved markers,
each with its own `tests/ui` fixture.

**E0063** — a **directive** marker written with `@`. Fires when `@` precedes a §2c name.
```
Error [E0063]: `@Test` is a compiler directive — write it with `#`, not `@`
 Why: `#` changes what compiles or runs (`#Test`, `#Unsafe`, `#Caps`); `@` is
      reserved for contracts stated on the declaration below (D-MARKER-FAMILY1).
 Fix: write `#Test("…") { … }`.
```
Keep the generic old-attribute `@` error (`Parser/Items.rs:771`, D-ATTR1 "attributes use
#, not @") only for `@` **not** followed by a known marker ident; a `@`+known-directive
routes to E0063, a `@`+known-contract parses, a `@`+unknown-ident falls through to the
generic message (or a new "unknown `@` contract; did you mean" — optional E0064).

**ui fixtures** (one per moved marker for E0062, a representative few for E0063), place in
`tests/ui/`:
`marker_pure_hash.{jet,stderr}`, `marker_mustuse_hash.*`, `marker_codable_hash.*`,
`marker_encode_hash.*`, `marker_decode_hash.*`, `marker_experimental_hash.*`,
`marker_tested_hash.*`, `marker_hardened_hash.*`, `marker_publishedschema_hash.*`,
`marker_redact_hash.*`, `marker_numeric_hash.*` (E0062);
`marker_test_at.*`, `marker_unsafe_at.*`, `marker_caps_at.*` (E0063).

Register both codes in `docs/spec/diagnostics.md`: add table rows in the E006x block
(~line 122) and, if any long-form section is warranted, one worked entry each.

---

## 6. Sequencing

Run everything through the Nix dev shell (`nix develop -c …`), one at a time. After a
build agent, **re-verify with a real `cargo build` + `cargo test --no-run`** — the
new-diagnostics reminder is a stale mid-build snapshot. Clean `/tmp/nix-shell.*` if disk
looks tight.

### Phase 0 — gates resolved (done 2026-07-02)
D-MARKERMOVE2=B and D-MARKERMOVE3=B ratified — §4 carries the binding outcomes. No
remaining blockers; all phases may proceed in order.

### Phase 1 — `Syntax.rs` registry (I7/R3 first)
Files: `crates/jet-foundation/src/Syntax.rs`.
- Recase §2b constants to PascalCase.
- Merge `ATTR_NUMERIC` + `CONTRACT_BUNDLE_NUMERIC` (§3) to one `"Numeric"` const.
- Add `CONTRACT_CLI`/`CONTRACT_DOC` (§4 G4).
- Add `pub const CONTRACT_MARKERS: &[&str]` (the §2a+§2b+G4 names) and
  `DIRECTIVE_MARKERS` (or derive one from the other), plus `is_contract_marker` /
  `is_directive_marker`. Update the module doc block (lines 1833-1893) to drop
  "spelling pending D-CONTRACTCASE1 / rides D-MARKERMOVE1" — now decided.
- Update doc-comment marker spellings that name moved markers (`#Pure`→`@Pure` etc.).
Test: `nix develop -c cargo build -p jet-foundation`.

### Phase 2 — parser + formatter
Files: `crates/jet-parser/src/Parser/{Items.rs,Statements.rs,Modules.rs,Types.rs}`,
`crates/jet-parser/src/Formatter/{Items.rs,Statements.rs,mod.rs}`.
- **Parse the `@` plane.** Add `At`-token dispatch paralleling the `Hash` predicates:
  `at_pure_fn`/`at_must_use_fn`,
  `at_marker_list`/`at_single_type_marker` (`Items.rs:715,717`; `Modules.rs:387,389`),
  the layout/`published_schema` dispatch in `parse_type_after_markers` (`Items.rs:3286`),
  the `#Numeric` distinct-type path (`Items.rs:3748`), the `#Redact` field path, and the
  `@[…]` bracket group (parallel to `parse_marker_bracket_group`, `Items.rs:3186`).
- Route the moved names to `@`; emit **E0062** when they appear after `#`; emit **E0063**
  when a directive name appears after `@`. Update the generic `@` error sites
  (`Items.rs:771`, `Statements.rs:1086`) to only fire for `@`+non-marker.
- `split_type_markers` (`Items.rs:3235`): Codable/Encode/Decode now arrive from the `@`
  group; serde names now arrive from the `#` group; allow both groups on one type (G2).
- **G1 (D-MARKERMOVE2=B)**: `Parser/Types.rs` callback bound accepts `@Pure fn(T) -> U`; old `#Pure` type-position spelling → E0062.
- **Formatter**: emit moved markers with `@` — `fmt_type_markers`/`fmt_marker`
  (`Formatter/Items.rs:271,288`), the `#Pure`/`#MustUse` fn emitters (`Items.rs:121`),
  and the `rfind("#Pure"/"#Test")` span logic in `Formatter/mod.rs:180-233` (the moved
  ones now search `@`). Directives keep `#`.
Test: `nix develop -c cargo test -p jet-parser`; add a **fmt STABILITY test** covering an
`@`-marked fn + struct (round-trip `fmt(parse(src))` byte-equals `src`, not just
idempotence — see `tests/fmt.rs`; idempotence alone misses dropped tokens). Bless with
`UPDATE_EXPECT=1` only after eyeballing.

### Phase 3 — sema
Files: `crates/jet-sema/src/Sema/{Effects.rs,mod.rs,Registration.rs,CheckerCore.rs,
CheckerCoreLib.rs,Bundle.rs,SchemaMigration.rs,Schema.rs,CheckerInfer/binary.rs}`.
- Update marker-name matches to read the merged/renamed constants (esp. `#Numeric`
  merge, §3, in `binary.rs`/`Bundle.rs`).
- **Respell diagnostic text** that names moved markers in `Why:`/`Fix:` strings —
  `Effects.rs:540` (`#Pure`→`@Pure`), and every emitted string in these files that
  mentions `#Pure`/`#MustUse`/`#Codable`/`#Numeric`. Directive mentions (`#Grant`,
  `#Caps`, `#Tainted`, `#Test`) stay `#`.
- Any plane-law validation that belongs in sema rather than parser (e.g. an `@` marker on
  a non-declaration) lands here; prefer parser-stage where the token position already
  disambiguates.
Test: `nix develop -c cargo test -p jet-sema`; targeted `--test must_use`, `--test pure`,
`--test effects`, `--test unit_family`, `--test taint`.

### Phase 4 — prelude + stdlib respell
Files: `crates/jet-codegen/src/Prelude/CoreLib.rs` (**include_str-embedded — rebuild `jet`
after editing; dead prelude code never warns**), `crates/jet-codegen/src/Prelude/Core.rs`,
`crates/jet-driver/src/Jetpack/components/*.jet`, `docs/reference/syntax-surface.jet`.
- Respell moved markers in embedded/prelude Jet source (`#Pure`→`@Pure`, etc.).
- Rebuild `jet` so the embedded prelude updates.
Test: `nix develop -c cargo build`; `nix develop -c jet run examples/features/01_hello.jet`.

### Phase 5 — examples + expected
Re-grep first: `nix develop -c rg -l '#(Pure|MustUse|Codable|Encode|Decode|
PublishedSchema|Redact|Numeric)\b' examples`. Respell each hit to `@`.
Include the bracket form `#[Codable …]` → `@[Codable] #[…serde…]` split (G2). Expected
outputs (`examples/features/expected/*.out` or the reorganized equivalent) rarely contain
markers but re-check. Known hot examples (verify names — reorg in flight): the `pure`/
`determinism`/`effects` set, `must_use`, `serde_derive`, `serde_generic`, `distinct_types`,
`unit_family`, `display_debug` (`#Redact`), `migrations`/`migrations2` (`#PublishedSchema`).
Maturity is not part of this marker respell: its sole surface is
`#Meta(maturity: .Experimental | .Tested | .Hardened)`.
Test: `nix develop -c cargo test --test golden` (front-end pass + no `unsafe` + expected out).

### Phase 6 — tests/ui + other fixtures re-bless
Respell `#`→`@` for moved markers in every `tests/ui/*.jet` **and** its `.stderr`
(the caret/underline columns shift — re-bless, don't hand-edit), plus `tests/ui_lint/*`,
`crates/jet-sema/tests/must_use.rs`, `tests/{effects,determinism,pure,tir,unit_family,
taint}.rs`, and the CLI transcripts if any name a marker (`tests/cli/man.txt`,
`completions_*`, `published_schema` mentions). Add the new E0062/E0063 fixtures (§5).
Test: `nix develop -c env UPDATE_EXPECT=1 cargo test` **after** reading each diff; then a
clean `nix develop -c cargo test` with no `UPDATE_EXPECT`.

### Phase 7 — docs sweep
Files: `docs/spec/{syntax-decisions.md,spec.md,diagnostics.md,formal-core.md,
stdlib-api-laws.md,roadmap.md}`, `docs/reference/{core-library.md,maturity-tags.md,
syntax-surface.jet}`.
- Respell moved markers to `@`. In `syntax-decisions.md`: move D-MARKER-FAMILY1 /
  D-CONTRACTCASE1 / D-MARKERMOVE1 from any open/gated wording to fully-ratified — the
  "Two follow-up ballots gate implementation … open" paragraph (lines ~2565-2572) and the
  D-CAPBUNDLE1 "`#Numeric` overlap … open D-MARKERMOVE1" note (lines ~2660-2663) are now
  **resolved**; rewrite to state the final law and the merge. (Memory: ratified decisions
  leave the ballot queue; keep only the durable spec/log rows.)
- `diagnostics.md`: E0062/E0063 rows + any moved-marker mentions in existing entries
  (E0742/E0745/E0747 reference `#Pure` as a bound — apply the G1 decision consistently).
- **Do not touch** `.tower/tower.json` (owner-owned) or `editors/**` grammar-repo
  build artifacts. `editors/tree-sitter/*` + `editors/vscode`/`editors/zed` highlight
  files may be updated for the new `@` plane as a **secondary, optional** follow-up — note
  it, don't gate on it.

### Phase 8 — full-suite gate
`nix develop -c cargo build` (clean), then `nix develop -c cargo test` (whole suite,
including `tests/decisions.rs` ratification enforcement and `tests/truthfulness.rs` I6
seam check). Re-run yourself; never trust a subagent "green" (memory). Confirm
`nix develop -c jet run examples/features/01_hello.jet` still prints `hello, world`.

---

## 7. Exit criteria

- `Syntax.rs` carries the plane classifier; every moved marker + recased constant is
  PascalCase on the `@` plane; `#Numeric`/`@numeric` is one merged `@Numeric` (I8).
- Parser accepts the `@` plane in every position the moved markers occupy; `#`+contract
  → E0062, `@`+directive → E0063; both have `tests/ui` snapshots (I4).
- Formatter emits `@` for moved markers; a fmt round-trip (not just idempotence) STABILITY
  test covers an `@`-marked declaration.
- Prelude rebuilt; no moved marker survives as `#` in prelude/examples/tests/docs
  (grep-clean, excluding `Tower/**` and `editors/**` artifacts).
- G1 settled (owner-confirmed or ballot-recorded); G2/G3/G4 defaults implemented and
  logged on the card.
- `nix develop -c cargo test` fully green, hello-world runs, docs match behavior.
- Card log records: law shipped, move list applied, `#Numeric` merge done, G1 resolution,
  G2/G3/G4 defaults + any follow-up ballots raised (Debug/user-derive plane, editor
  grammars).
