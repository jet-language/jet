# #7066 pre-build adversarial review: DSL/library extraction of `migration`, `protocol`, `validate`; a constant-time compiler plugin; external FFI libraries

Date: 2026-11-27. Scope: card #7066, criterion 1 (the design review before the build).
Mode: read-only review of the live tree. No compiler was built or run. Every count
below comes from reading the source in this session: `tower.mjs card show`, file
reads, and `fd | awk` line counts over `Compiler/`, `Core/`, `Jetpack/`, `Tests/`,
`Examples/` and `Tools/`.

Inputs: `node Tools/Tower/tower.mjs card show 7066` and `card show 7046`;
`Docs/Audits/2026-11-27-cognitive-budget-orthogonality-audit.md` (O3, O4, O12,
O36, O39); `Compiler/JetParser/Source/Parser/Items.jet`; `Compiler/Prelude/Effects.jet`;
`Core/Crypto/`; and `Docs/Spec/spec.md` (§1300–1308 validate, §1378–1497 constant
time, §1576–1624 migrations, §4371 `FieldRef`, §5461–6030 FFI).

## 0. Verdict up front

Criterion 1 asks for a review "proving zero compile-time safety or diagnostic
loss". As the card is written, this review **cannot** prove that. It finds
concrete losses or unproven premises in four of the five extractions. The
binding outcome is:

| Part | Verdict | Why (short) |
|---|---|---|
| 1a `validate`/`check` → library `Validate` trait | **PROCEED, amended** | It already lowers to an ordinary method. The card's signature drops error accumulation and collides with the live `Validate` builder name. |
| 1b `migration` + verbs → `#7046` DSL | **BLOCK → ballot** | `rename`-from and `remove` name fields that exist only in the schema snapshot, so `FieldRef` cannot check them. `#7046` produces values, not type-attached roles. `via` cannot be unreserved. |
| 1c `protocol`/`client`/`server` → library | **BLOCK → ballot** | It is already a source-to-source expansion over core typestate plus `#SingleUse`. Moving it needs an item-generating mechanism, which `#7046` does not provide. |
| 2 constant-time checking → `Core/Crypto` compiler plugin | **REJECT as specified → ballot** | Only the ICE-only validators could move. The user-facing sub-language and the masking transform must stay in core. A plugin adds a new hook API and a risk of silently skipping the check, and removes no user-facing concept. |
| 3 FFI bridges → external libraries over a minimal C ABI | **PROCEED for the 22 non-C/Rust languages; BALLOT for C, Python and effect spelling** | Binders already live mostly in Jetpack. Moving C out contradicts O39 ("Core ships C and Rust"). Package-declared effect leaves are a new, forgeable authority mechanism. In-process Python is a new safety surface. |

Every gap below appears either as a binding criterion for #7066 (§5) or as a
ballot (§6).

## 1. Live baseline (exact counts)

### 1.1 The "12 keywords"

None of the 12 words is reserved by the lexer. There is no `TOKEN_KW_` token for
any of them; a search for
`TOKEN_KW_(MIGRATION|RENAME|ADD|REMOVE|CHANGE|VIA|PROTOCOL|CLIENT|SERVER|STATE|VALIDATE|CHECK)`
in `Compiler/` returns 0 matches. Each word is recognised contextually as
`TOKEN_IDENT` plus lookahead:

- `migration`: `Parser/Declarations/Migrations.jet:38-40` (`migration <Ident> {`).
- `rename`/`add`/`remove`/`change`/`via`: inside the migration body,
  `Migrations.jet:52-85`.
- `protocol`: `Parser/Declarations/Protocols.jet:24-26` (`protocol <Ident>`).
- `validate`: `Parser/Items.jet:2207` (`validate {` in a struct body).
- `check`: a builtin only inside a `validate` block (`PackageFiles.jet:209-214`).

Consequence: unreserving the words frees **zero identifiers** for users. The real
saving is grammar productions, AST variants, `JET_KEYWORD_LIST` rows
(`Compiler/JetFoundation/Data/Syntax/PackageFiles.jet:417-494`; the relevant rows
are 444-450 and 476-485), `SyntaxDictionaryData.jet` rows (`JetCLI/Source/CLI/SyntaxDictionaryData.jet:70-80`),
and spec text. The audit's "taxes every program" claim (O3 cost line) is
false for identifiers; it is true only for LSP completion and learning surface.

Per-word status:

| Word | Registry | Removable? |
|---|---|---|
| `migration`, `rename`, `add`, `remove`, `change` | `KW_MIGRATION`/`KW_RENAME`/`KW_ADD`/`KW_REMOVE`/`KW_CHANGE`, `PackageFiles.jet:216-233` | Yes, if 1b lands. |
| `via` | `KW_VIA`, `PackageFiles.jet:243` | **No.** It is also the live effect pass-through `-[via f]>` (`Parser/Items.jet:1410-1430`, `parser_parse_opt_func_effects`). Unreserving it would break effect rows. |
| `protocol`, `client`, `server` | `KW_PROTOCOL`/`PROTO_CLIENT`/`PROTO_SERVER`, `EffectsSurface.jet:76-81` | Yes, if 1c lands. |
| `State` | `KW_STATE`, `EffectsSurface.jet:51-60` | It is not grammar. It is the **retired** `#State` marker name, kept so that E0175 teaches the replacement. Removing it deletes a teaching diagnostic, which is a diagnostic-loss decision. |
| `validate`, `check` | `KW_VALIDATE_BLOCK`, `VALIDATE_CHECK_FN`, `PackageFiles.jet:194,214` | Yes, if 1a lands. |

The honest count is **10 grammar words removable**, **1 not removable**
(`via`), and **1 retired-marker teaching name** (`State`) whose removal needs its
own ruling. `state` (`KW_STATE_DECL`, the typestate list) is the surviving
mechanism and must stay. Part 1c depends on it.

### 1.2 Compiler footprint of the surfaces in Part 1

| Item | Lines / files |
|---|---|
| `.Migration(`/`{` match arms in `Compiler/` | 349 lines / 123 files |
| `.ProtocolDecl(`/`{` match arms in `Compiler/` | 346 lines / 123 files |
| `Parser/Declarations/Migrations.jet` | 109 |
| `Parser/Declarations/Protocols.jet` | 100 |
| `Sema/Registration/Protocols.jet` | 157 |
| `Sema/Registration/Validate.jet` | 262 |
| `Sema/Registration/StateTypes.jet` / `StateFacts.jet` | 298 / 431 (these stay; they are typestate) |
| Repo `.jet` files using `migration X {` | 17 |
| Repo `.jet` files using `protocol X {` | 6 |
| Repo `.jet` files using `validate {` | 7 |
| UI `.jet` fixtures named `*migration*` / `*protocol*` / `*validate*` | 12 / 2 / 5 |

Diagnostics in play and the files under `Tests/` and `Examples/` that name each
code: E0910 7, E0911 2, E0150 25, E0151 9, E0153 9 (all Unicode-data or census
noise; no UI snapshot), E0174 9, E0175 8.

**E0153 has no UI snapshot.** `Tests/diagnostics_coverage_allowlist.tsv:8` reads
`E0153 left-only Tower #6752`. Its text also says "if this persists, file a
bug", so in substance it is an internal error presented as a user error. Parity
cannot be proved against a baseline that has no snapshot.

### 1.3 The constant-time pipeline (Part 2)

The pipeline has five layers, not one "IR check":

| Layer | Source | Size | Kind |
|---|---|---|---|
| Sema rules E0737, E0738, E0739; interprocedural callee walk; the derived `equal`/`compare` rule | `JetSema/Source/Sema/Taint.jet` (shared with `Secret`/`Credential`/`PII`/`Input`) | 3474 lines | **User diagnostics.** I3 requires them to stay in sema. |
| Masked-branch and masked early-return lowering | `JetIR/Source/BuildMask.jet` | 338 | **Code transform.** It changes the generated code. |
| Fixed-bit marking and flow | `JetIR/Source/IR.jet:401-405` (`slot_fixed`), `Timing.jet` `ir_timing_marked` and `ir_timing_fixed` | — | Core IR data |
| IR Lint fixed rule, run after every optimiser pass | `JetIR/Source/Lint.jet:877-890` `ir_lint_fixed`; called at `:798` | rule within 1785 | ICE only |
| Selected-instruction validator | x86-64: `JetIR/Source/Timing.jet` `ir_timing_x64_row`/`ir_timing_x64_check`, 234 lines, called from `X64Select.jet:6573`. wasm32: `JetWeb/Source/Web/Timing.jet`, 212 lines, `web_timing_check`. | 446 | ICE only |
| Selection gating | `X64Select.jet:5564` and `:6158` gate the fast path and the emission log on `ir_timing_marked(f)`; `JetWeb/Source/Web/Select.jet:120-131` does the same | — | **Selection behaviour** |
| Host admission | `JetRuntime/Source/HostTiming.jet` (66), `IR/Build.jet:2439-2450` (`jet_rt_timing_admit`), E0744 | — | Runtime plus a user diagnostic |

`Core/` lines that use `#Secret` or `#Scrub`: Core/Crypto 80 lines in 8 files;
**Core/Net 11 lines in 3 files; Core/DB 1 line in 1 file.** The check is not
confined to `Core/Crypto`. 58 `.jet` files under `Tests/` and `Examples/` use
`#Secret`, `#Scrub` or `timing: .Fixed`. A code-string search finds E0737 in one
case, `Tests/Cases/Taint/fixed_timing_untagged_callee_is_refused_at_every_tier.case`.
E0744 has `Tests/UI/FixedTimingUnconfirmedTarget/`. The census
(`Tests/diagnostics_census.tsv:440-447`) marks E0737, E0738, E0739 and E0744 as
`ui`, but this session found no UI fixture naming E0738 or E0739 by code. The
UI coverage of those two must be confirmed before a parity claim.

A plugin mechanism does exist, but it is a different thing. `JetPlugin`
(`Plugin/Interface.jet`, `Link.jet`), `Sema/Calls/Plugin.jet` and
`IR/BuildRoutes.jet:6902-6926` (`ir_plugin_load`) load **WebAssembly components
called by the program at run time**. The tree has no compiler-pass plugin API
and no IR hook API.

### 1.4 The FFI footprint (Part 3)

- `Compiler/Prelude/Effects.jet:82-116`: `FFI` is an enum with **24** language
  cases. The header says this list feeds `Tools/Agent/CoreSourceRows.jet`
  (`#FFI(<lang>)` menu, `Core/Compiler/Lang.jet`) and that Jetpack's
  `BinderDescriptor.jet` is checked against it. Effects.jet lines 1-13 say an
  effect exists **exactly when written** in the Prelude or Core lists. That
  makes the vocabulary closed.
- `FFI.<Lang>` references across Compiler, Core and Jetpack: 247 lines in 84 files.
- Spec FFI section: `spec.md:5461-6030`, 569 lines, with 22 per-language
  subsections plus data-schema binders.
- Compiler-resident FFI code, in lines:
  - JetCLI: `AuditForeign` 366, `Bindings` 140, `CHeaderBind` 441,
    `ForeignBind` 492, `ForeignLink` 881, `ImportBindings` 576, `RustForeign` 887.
  - JetDriver: `ForeignDeps` 299, `ForeignProviders` 1452, `RustForeign` 410,
    `RustForeignSource` 296.
  - The rest: `JetIR/BuildForeign` 1234, `JetSema/Calls/Binder` 596,
    `JetParser/Declarations/Foreign` 258, `JetRuntime/FFICallback` 294.
- Already in Jetpack: `PackageModel/Source/Foreign/C/HeaderBinder.jet` (556),
  `Binders/Python.jet` (203), `Bridge/BinderDescriptor.jet` (1014), plus
  per-language binders and their tests. `CHeaderBind.jet:1-5` states that the
  compiler's C bind runs **through** `jet_pkg_model`'s `pkg_cbind_bind_files`.
  So the header parser is already library code. The compiler holds the host half
  and the link orchestration.
- Python today is a **supervised out-of-process worker** that carries scalars
  only (`Binders/Python.jet:5,33-44`; `spec.md:5604-5625`). No CPython C-API,
  GIL or refcount code runs in-process today.
- `Close` already exists twice. One is the trait `Close: trait { close: fn(^^self) Never! }`
  (`Compiler/Prelude/Traits.jet:251-255`). The other is the Prelude marker
  `#Close(close)` (`JetSema/Source/Sema/PreludeRows.jet:34`), used on
  `extern rust` returns (`spec.md:5511`) and parsed in `Parser/Declarations/Foreign.jet:204`.
  This is a pre-existing I8 duplicate.
- `ForeignBridge` does not exist anywhere in `Compiler/`, `Core/` or `Docs/Spec`.

### 1.5 Dependency status

#7046 (the unified `DSL` trait) is in phase `ready`. Criterion 1 is met;
**criteria 2-6 are open**, including the lexer/parser capture and the sema
four-step handshake. #7066 has `blockedBy: []`. Every Part 1 and Part 3 DSL
claim rests on #7046 machinery that has not shipped.

## 2. Part 1: Language surfaces to DSL/traits

### 2.1 `validate`/`check` → `Validate` trait (O12)

**What exists.** `Sema/Registration/Validate.jet:1-8` already rewrites the block
into an ordinary pure method, `impl T { validate: fn(value: T) -> T [FieldError]! }`.
`check` collects one `FieldError{path, reason}` per failing rule, and generated
decode calls `T.validate(decoded)` (`sema_serde_items_struct_decode_body`).
This makes it the most library-ready of the three.

**Attacks.**

1. **Name collision.** `Validate` is already a compiler-owned name:
   `TYPE_VALIDATE: "Validate"` (`PackageFiles.jet:198`). It is the
   outside-context builder `Validate.over(s)` (`spec.md` §1300; the builder is
   `__JetValidate`). A trait named `Validate` cannot share that name with a
   builder type. The two must be merged or one renamed, and that is a public API
   decision.
2. **Wrong signature on the card.** The card writes
   `validate: fn(self) ValidationError!`. `ValidationError` exists nowhere in
   `Compiler/`, `Core/` or `Docs/Spec` (0 matches). The live contract is
   accumulating `[FieldError]`, and `decode<T>` returns `T ![FieldError]`
   (`spec.md:1608`). A single `ValidationError` either:
   - drops accumulation, which is a diagnostic regression, since users get the
     first failure instead of all of them; or
   - duplicates `[FieldError]`, which is a second mechanism (I8).

   It must stay `[FieldError]`. O12's `fn(self) -> Unit ![FieldError]` is
   closer, but it changes the return from `T` to `Unit`. That is fine for a
   trait, but the decode chain must then return the decoded value itself.
3. **Field-path checking.** `at: field` is a bare sibling name resolved by sema.
   A library `check` must take `FieldRef<Self, T>`. `spec.md:4371-4384` says
   only the compiler builds a `FieldRef` and a bare field is accepted where one
   is expected. So `check(cond, at: price, "…")` keeps its spelling and keeps
   compile-time field checking. This part is sound.
4. **Implicit accumulator.** Today `check` pushes to a hidden `__errors`
   (`Validate.jet:42`). A library function has no hidden local, so one of two
   things must happen:
   - The function threads `^errors`, which is an ergonomic loss to
     `check(^errors, cond, at: f, msg)`.
   - The trait method returns a list built with an ordinary builder.

   The lower-cost spelling must be shown in the golden example before
   ratification.
5. **Purity.** Today purity comes from the generated method being `pure`. The
   trait row must declare an empty effect row, so an impl that logs or does I/O
   is refused. Without it, validators gain effects silently.
6. **Decode auto-run.** Decode must dispatch on `T: Validate`, which is an owned
   trait like `Close` (`JetFoundation/Source/Registry/OwnedTraitRows.jet:82`).
   This is one compiler-known role, not a special case, provided it goes in
   `OwnedTraitRows` and not in an ad-hoc name check.

**Parity proof required.** Every current UI fixture for validate (5) and every
`validate {` user (7 files) migrates. The same `[FieldError]` paths and reasons
must appear on O0, O1 and O2.

### 2.2 `migration` → DSL (O3)

**What exists.** Parser: `Migrations.jet` (109 lines). The check compares the
current shape with the snapshot under `.jet/cache/schema/` (E0910/E0911,
`spec.md:1576-1606`). The converter resolution order is inline `via` → `impl New.From(Old)`
→ E0910. Decode runs a silent historical-shape chain (`spec.md:1613-1624`).

**Attacks.**

1. **`FieldRef` cannot check this (soundness).** O3 says migrations would be
   "checked by `FieldRef`". That premise is false.
   - `rename old -> new`: `old` is **not** a field of the current type.
   - `remove legacy_id`: `legacy_id` is likewise not a current field.

   `FieldRef<Model, T>` names only current stored fields (`spec.md:4372`). The
   only authority for historical fields is the snapshot, a build input read from
   disk. So the check must stay in a component with access to the snapshot (sema
   or the driver). It cannot live in a library's typed data.
2. **#7046 has the wrong shape.** The #7046 handshake is `#run Type.parse(raw)`,
   then holes, then a fold to a **value** (card #7046 criterion 3). A migration
   is not a value; it is a role attached to a `#Codable(published)` type that
   the generated decode must find. A library spelling therefore still needs:
   - a compiler-known attachment, such as an owned `Migrate` trait;
   - comptime file-system access to the snapshot, together with cache-key
     tracking of that file.

   Without the cache key, cold and warm builds disagree. The compiler keeps the
   E0910 logic either way. The net saving is the 5-verb grammar plus one AST
   variant, about 349 match-arm lines, against a new comptime-FS dependency.
3. **`via` stays a keyword** because of `-[via f]>` (§1.1). The phrase
   "unreserving 12 keywords" is therefore unachievable as written.
4. **Diagnostic spans.** E0910 currently points at the operation's field token
   (`MigrationOp.Rename{from_span, to_span}`, `Migrations.jet:59`). DSL diagnostics
   must reach the same span granularity through #7046's hole-span machinery.
   For foreign (non-hole) tokens, #7046 has not yet shown that it can.
5. **LSP rename regression.** Today the `to` field of `rename` is a parsed name.
   In a DSL body it becomes foreign text, unless the body is written as `FieldRef`
   holes. Renaming a struct field with the LSP would then stop updating the
   migration. The verbs must use Jet holes for current-field names.

**Strongest peer designs** (cited from general knowledge, not re-read this
session):

- Django migrations and Alembic: migrations are generated data checked against a
  stored model state, which is the same snapshot dependency Jet has.
- Serde's `#[serde(alias, default)]`: per-field attributes and no history.

The Django/Alembic precedent supports keeping the snapshot comparison in the
toolchain. A cheaper alternative to a DSL is per-field markers: `#Was(old_name)`,
`#Default(expr)`, and a type-level `#Removed(field)` row.

- **Gain:** three of the five verbs collapse into markers that already have a
  home, and `remove` becomes a type-level row.
- **Cost:** history spreads across fields.

This goes on the ballot.

### 2.3 `protocol`/`client`/`server` → library (O4)

**What exists.** `Sema/Registration/Protocols.jet:1-7,25-59`. Before
registration, a `protocol` is rewritten through `parse_source` into two
`#SingleUse` structs, `Name.Client` and `Name.Server`. Their `state` lists run
one step per message, and their methods are typed by state. Linearity comes
**entirely** from:

- `#SingleUse`;
- typestate (E0150 out-of-order use, E0151 unknown state, E0174 illegal move).

All of these are core mechanisms that stay.

**Attacks.**

1. **The extraction is sound only if a library can emit items.** The expansion
   produces types and impls. #7046 produces a value. Neither #7046 nor any
   ratified mechanism in the tree lets a library generate nominal types with
   `state` lists and impl blocks. The closest live machinery is
   `Item.TemplateLoop` (`Parser/Items.jet:287-289`, `Sema/Registration/UserTemplates.jet`).
   It must be checked to see whether it can host this; that is unverified. An
   item-generating macro would be a new user-facing mechanism, which I8's
   1-in-1-out rule forbids unless it deletes more than protocol.
2. **Linearity is preserved if the generated code is the same.** The session
   guarantee comes from `#SingleUse` plus typestate, which the library would
   reuse. Parity is therefore provable by fixture:
   - `protocol_bad_endpoint` and the 2 protocol UI fixtures;
   - the E0150 out-of-order case;
   - a forgotten-close (`#SingleUse` not consumed) case on O0, O1 and O2.
3. **E0153 is the evidence gap.** The library has no baseline for expansion
   failure (§1.2). The build must turn E0153 into a real user diagnostic that
   names the failing message line and has a snapshot. Otherwise it must be
   reclassified as an ICE (exit 101, I2).
4. **`State` (the retired marker).** It belongs to O4's "+State", but deleting
   it removes the E0175 teaching path (8 files reference E0175). That is a
   ruling on retiring a teaching diagnostic, separate from protocol.

**Peers.** Rust's `session_types` crate and Ferrite encode sessions as type-level
lists (`Send<T, Recv<U, Eps>>`). Scribble generates endpoint APIs from a global
protocol through an **external code generator**. Jet's design is the Scribble
model done in-compiler. The type-level-list model needs no item generation, but
it needs variadic or recursive generic types and produces poor error messages.
That is a ballot alternative, not a free win.

## 3. Part 2: Constant-time checking → `Core/Crypto` compiler plugin (O36)

### 3.1 What can move at all

From §1.3:

- **Must stay in core.**
  - The sema rules E0737, E0738 and E0739 (I3: sema owns checks).
  - Their interprocedural walk, inside the 3474-line `Taint.jet`, which is
    shared with the taint tags the card keeps.
  - The masking transform in `BuildMask.jet`, which changes the generated code.
  - The `slot_fixed` IR data.
  - The selection gating at `X64Select.jet:5564` and `Web/Select.jet:131`.
  - Not inlining a `#Scrub` routine where its result is public (`spec.md:1471`).
  - Host admission and E0744.
- **Could move.** The ICE-only validators:
  - `ir_lint_fixed` (one rule inside `Lint.jet`);
  - `ir_timing_x64_check` with its table (`Timing.jet`, 234 lines);
  - `web_timing_check` with its table (`Web/Timing.jet`, 212 lines).

  That is under about 500 lines of internal checking.

**The user-facing concept count does not change.** `timing: .Fixed`, masked
branches, E0737/8/9, `#Scrub` and `policy.harden` all remain in the spec and the
compiler. O36's stated cost, "a full sub-language inside the core spec", is not
reduced by moving the validators. The extraction adds a public IR-hook API, which
is a new mechanism under I8 and deletes no user-facing one.

### 3.2 Soundness attacks on a plugin validator

1. **Silent skip.** The audit's proposal reads "a plugin that `Core/Crypto`
   depends on". A user package that declares its own `timing: .Fixed` tag
   without depending on Core/Crypto would get **no** instruction validation. The
   same applies to Core/Net (11 lines) and Core/DB (1 line) if their dependency
   edge changes. That is exactly the "proof that silently skips" that AGENTS.md
   forbids. The fix: the core decides activation from the presence of any fixed
   slot, and a missing validator is a hard error. But then the core still knows
   the whole feature, which defeats the extraction.
2. **The optimiser contract stays in core.** IR Lint must run "after each of its
   passes" (`spec.md:1467`). Any core pass that turns a masked select back into
   a branch breaks the guarantee. The core optimiser must keep respecting
   `slot_fixed` regardless of the plugin. The plugin can detect a violation; it
   cannot prevent one.
3. **ABI freeze.** A pass-level hook exposes `IROp`, the slot columns and the
   `X64EmitRow` emission log (`Timing.jet:21-27`) as a public, versioned
   contract. Today these are free to change. GCC plugins and LLVM pass plugins
   are the cautionary peers: an unstable internal ABI behind a "plugin" API.
   rustc lints (clippy) avoid this by linking against internals that are
   explicitly unstable.
4. **Cold-compile cost.** Today an unmarked function costs one `ir_timing_marked`
   scan (`Timing.jet:31-36`) and allocates nothing (`Timing.jet:10-11`). The only
   live plugin host is a WebAssembly component runtime (`JetPlugin`). Calling it
   per function, per pass, with IR serialised across the component boundary,
   works against the 30M LOC/s target. Any hook must short-circuit in core on
   `!ir_timing_marked(f)` before crossing into a plugin.
5. **I9 across targets.** The x86-64 and wasm32 tables live in different
   packages (JetIR and JetWeb). A plugin must cover both, and every future
   target (aarch64 DIT), or that target silently loses validation.

### 3.3 The minimal hook, if the owner still wants it

The card requires this to be specified. This is the smallest surface that
preserves today's guarantees:

```
TimingValidator: trait {            // proposed; name subject to ballot
    // After every optimiser pass in verify mode, and once before selection.
    // Called only for f with ir_timing_marked(f) == true (core short-circuits).
    check_ir: fn(self, f: IRFunctionView, fixed: [Bool], pass: String) -> [IRLintFinding] Never!
    // After selection, per target, with that target's emission log.
    check_selected: fn(self, f: IRFunctionView, fixed: [Bool], target: TargetId, log: SelectedLogView) -> [String] Never!
}
```

Core keeps these obligations:

1. It computes `fixed` (`ir_timing_fixed`), so the plugin never re-derives
   taint.
2. Any finding is an ICE (exit 101) that names the function, the instruction
   and the line, as today (`spec.md:1484`).
3. If a marked function exists and no validator is registered for the selected
   target, the build is a hard error, never a skip.
4. The views are read-only, so the plugin cannot mutate IR.
5. The views are pure and effect-free (`Never!`, no `FS` or `Exec`), so
   validation stays deterministic and cacheable.

**Zero-regression proof for `Core/Crypto` (binding if Part 2 proceeds).**

- The plugin must reproduce byte-identical findings, with zero findings on
  green code, for:
  - the 80 Core/Crypto lines;
  - the 11 Core/Net lines;
  - the 1 Core/DB line;
  - all 58 Tests/Examples files.
- Each target must be run: x86-64 O1/O2 and wasm32.
- Seeded-mutation evidence is also required: inject a `DIV` or a secret branch
  past sema with a test-only IR mutator, and show the plugin raises the same ICE
  that `ir_timing_x64_check`/`web_timing_check` raise today.
- `jet perf` cold-compile numbers are required before and after, on a program
  with zero fixed slots and on Core/Crypto.

**Recommendation.** Keep the validators in core. Get the cognitive win O36 wants
without a plugin: move `spec.md:1378-1497` into a `Docs/Spec/Reference/` page so
the core spec only points to it. That changes only documentation and adds no
mechanism. This goes on the ballot.

## 4. Part 3: FFI bridges → external Jet libraries over a minimal C ABI (O39)

### 4.1 Premise check

- **The card contradicts its source finding.** The card says C, Python and Rust
  all move out. O39 says "Core ships C and Rust. Other languages are packages."
  Removing C from first-party while keeping "minimal C ABI primitives" in the
  compiler leaves the header binder as the only thing that moves. It is already
  Jetpack library code (§1.4). The owner must pick one.
- **Most binders are already library code.** `HeaderBinder.jet`,
  `Binders/Python.jet`, the data-schema binders and the per-language binder
  tests are in `Jetpack/PackageModel`. What remains in the compiler is:
  - the host and CLI halves (`jet inspect bind …`);
  - link orchestration (`ForeignLink` 881, `ForeignProviders` 1452,
    `ForeignDeps` 299);
  - Rust's cargo bridge (`RustForeign` ×3, about 1593 lines);
  - IR lowering (`BuildForeign` 1234), sema (`Binder` 596) and the callback
    runtime (`FFICallback` 294).

  The real extraction is the **orchestration**, not the parsing.

### 4.2 Minimal compiler C ABI primitives (what must stay in core)

| Primitive | Why a library cannot do it | Live anchor |
|---|---|---|
| `#Layout(c)` / `#Layout(c, align(N))` | Layout is a codegen fact | `PackageFiles.jet:362-378` |
| A C function-pointer type with an explicit calling convention (C/SysV/Win64) | ABI lowering | `BuildForeign.jet` |
| An extern symbol with a link name, plus a **typed link request** (library, kind, provenance) that the driver resolves | Only the driver links. A library cannot inject linker flags without a compiler API. | `ForeignLink.jet`, `ForeignProviders.jet` |
| C→Jet callback trampolines: which task and context run them, and the policy for panics across C frames (abort, never unwind) | Runtime-owned. I1. | `JetRuntime/FFICallback.jet` |
| An `#Unsafe` gate on every raw extern call, plus the root `FFI` effect being recorded **by the primitive itself** | The authority floor must not depend on the library telling the truth | `spec.md:5547-5568`, `Effects.jet:145-154` |
| Target layout facts available at comptime (`#build.target.layout.*`) | A header binder needs `long`/`size_t`/alignment per target | `Spellings.jet:45` |

### 4.3 Attacks

1. **Package-defined effect leaves (`c.FFI`, `python.FFI`) are a new, forgeable
   authority mechanism.**
   - `Effects.jet:1-13`: effects exist only when written in the Prelude or Core
     lists. Allowing packages to declare child effects changes:
     - manifest `--allow`/`--deny` validation;
     - authority grants (E0749, E1220, E1221; `spec.md:5554-5563`);
     - reflection.
   - Forgery: a package could declare `python.FFI` and claim to be the Python
     bridge. Leaves must be namespaced by **package identity**, not by a name the
     package chooses.
   - Floor: the root `FFI` must be recorded by the core primitive (§4.2), so a
     library can only *refine*, never *erase*.
   - Spelling: the card's `c.FFI` inverts today's `FFI.C("<lib>")` and drops the
     per-library key that grants depend on. `allow: [FFI.C("c")]` refusing `m`
     is live, specified behaviour. The library-keyed grant must survive the
     cutover.
2. **The vetted-gate boundary moves.**
   - `Effects.jet:145-149`: "Core's gates are vetted and sealed, so calling Core
     reaches none."
   - Bridges that move out of Core/compiler expose their internal `#Unsafe`
     gates to every caller. Programs and packages under `deny: [Unsafe]` or
     `-[!Unsafe]>` that call generated bindings today may start failing, or the
     project must invent a "vetted third-party library" seal. Sealing a package
     is a new trust mechanism and needs an owner ruling.
3. **C header parsing in a Jet library.** The current binder skips and reports
   what it cannot map (`spec.md:5534-5537`). Each of these needs an explicit
   rule and a fixture before claiming parity:
   - target-dependent types (`long`, which is LP64 on Unix and LLP64 on
     Windows; `size_t`; `wchar_t`);
   - bitfields;
   - `__attribute__((packed/aligned))` and `#pragma pack`;
   - anonymous unions and structs;
   - flexible array members;
   - variadics;
   - function-like and value macros (constants come from macros in most real
     headers);
   - enums with negative or above-`int` values;
   - `#include` search paths and system headers;
   - `_Static_assert`.

   Zig `@cImport` and Swift's ClangImporter embed clang. Rust `bindgen` needs
   libclang. A pure-Jet parser that skips macros will bind fewer symbols than
   peers; the build must state that as a measured gap, not a parity claim. C++
   already needs an absolute clang path (`spec.md:5594`), so a C++ library needs
   `Exec` at bind time. The comptime and `build: fn` effect policy for spawning
   toolchains must be specified.
4. **Python: in-process versus worker.** Today's design is out-of-process
   (§1.4). The card's mention of "first-class Python" and of GIL and refcount
   safety implies in-process CPython embedding, which opens these issues:
   - **GIL × Jet tasks.** A GIL-holding call blocks every Jet task that touches
     Python. PyO3 models this with a `Python<'py>` token. Jet needs an
     equivalent capability parameter (`^PyGIL`), or every Python call becomes
     serialising and process-global.
   - **Refcounts × I1.** Every `PyObject*` must be a non-Copy handle with
     `impl Close` (decref). Borrowed versus new references
     (`PyList_GetItem` versus `PyList_GetSlice`) must be distinguished in the
     binding's types, or the result is leaks (an I1 violation) or a
     use-after-free.
   - **Free-threaded CPython (3.13t)** removes the GIL assumption completely.
   - **Interpreter finalisation order** at process exit, and crash isolation: a
     segfault in an extension module kills the Jet process. Today's worker
     contains it.

   Recommendation: keep the worker as the default (fails closed, crash
   isolation) and make in-process an explicit expert opt-in under `#Unsafe`.
   The perf gate matters because the worker pays IPC per call, so any Python
   interop cell against PyO3 fails the peer floor. The ballot must state which
   mode the perf cell measures.
5. **`#Close` / `impl Handle.Close`.**
   - Two mechanisms exist today for one meaning: the `#Close(close)` marker and
     the `Close` trait (§1.4), which violates I8. The cutover must delete the
     marker form, and `Foreign.jet:204`, and keep `impl Close`.
   - `close: fn(^^self) Never!` is **infallible**. Many C close functions
     report failure: `fclose` returns `EOF` on a flush failure, and
     `sqlite3_close` returns `SQLITE_BUSY`. Wrapping these in `Never!` means
     either silently discarding the error (a correctness loss) or panicking.
     The bridge contract needs a rule: a fallible close is exposed as an
     explicit `finish: fn(^^self) -> Unit E!`, and `Close` is reserved for the
     best-effort drop path. The ruling goes on the ballot.
   - A `#Layout(c)` struct that wraps a raw pointer must not derive structural
     Copy or Clone. Today `Close`-implementing types are already excluded
     (`Sema/Ownership/Copy.jet:230-349`). The bridge must require `impl Close`
     on every owning handle so that rule applies.
6. **Build and link orchestration.** Today the driver runs cargo and pkg-config,
   computes content-addressed bridge identities (`spec.md:5480-5483`) and
   resolves link flags. After extraction the library needs:
   - a typed "request link" API;
   - a way to feed bridge identity (toolchain and target bytes) into the build
     cache key.

   Without these, a toolchain change silently reuses a stale archive, which
   breaks a property the spec currently guarantees. E0703/E0704/E0705 (Rust)
   and E3201/E3208 (C) must either remain compiler diagnostics raised from
   library-supplied facts, or move into the library with registered codes and
   snapshots (I4).
7. **I9 and comptime.** Foreign calls never run at comptime (`spec.md:5494-5495`).
   A binder that runs at comptime (`#run`) to parse headers is fine, but the
   *bound calls* must still be refused at comptime with the same diagnostic on
   every tier.

## 5. Binding cutover list (criteria for #7066)

The card cannot close while any item below remains open.

**Part 1a: `validate`**

1. Resolve the `Validate` builder versus trait name collision (`PackageFiles.jet:198,200,202`)
   by ballot outcome. Migrate every `Validate.over(s)` caller.
2. The trait signature keeps `[FieldError]`. `check` takes `FieldRef<Self, T>`.
   The trait method carries an empty effect row.
3. Decode dispatch comes from an `OwnedTraitRows` row, not an ad-hoc name check.
   Delete `Sema/Registration/Validate.jet`'s block rewrite, `Parser/Items.jet:2207`,
   `KW_VALIDATE_BLOCK` and `VALIDATE_CHECK_FN`, their `JET_KEYWORD_LIST` rows
   (`PackageFiles.jet:484-485`) and their `SyntaxDictionaryData.jet` rows.
4. Migrate all 7 `validate {` files and all 5 validate UI fixtures. The same
   `FieldError` paths must appear on O0, O1 and O2. Update `spec.md` §1300-1308
   and the Habits/Examples goldens.

**Part 1b: `migration` (only if the ballot picks an extraction option)**

5. Keep `KW_VIA` and the `-[via f]>` parse (`Parser/Items.jet:1424`). Correct
   the card text from "12 keywords" to the real count.
6. Historical field names are checked against the schema snapshot with the same
   E0910 spans (`MigrationOp` `from_span`/`to_span`). The snapshot is a tracked
   cache-key input.
7. Delete `Item.Migration` (349 match-arm lines in 123 Compiler files),
   `Migrations.jet`, `KW_MIGRATION`/`KW_RENAME`/`KW_ADD`/`KW_REMOVE`/`KW_CHANGE`
   and their keyword-list rows (`PackageFiles.jet:445-449`). Migrate 17 files
   and the 12 UI fixtures; E0910/E0911 snapshots must be byte-identical or
   carry a stated reason. Update `spec.md` §1576-1624.

**Part 1c: `protocol` (only if the ballot picks an extraction option)**

8. Name the item-generating mechanism, or the type-level-list design, and pass
   it through the I8 1-in-1-out rule.
9. Turn E0153 into a snapshot-covered user diagnostic, or reclassify it as an
   ICE. Remove its `diagnostics_coverage_allowlist.tsv` row.
10. Delete `Item.ProtocolDecl` (346 match-arm lines in 123 files),
    `Protocols.jet` in both the parser and sema, `KW_PROTOCOL`/`PROTO_CLIENT`/`PROTO_SERVER`,
    the keyword rows (`PackageFiles.jet:480-482`), the formatter's
    `jfmt_fmt_protocol_decl` and the `ASTCodec` row. Migrate 6 files and 2 UI
    fixtures.
11. Session parity on O0, O1 and O2 needs:
    - out-of-order send (E0150);
    - unknown state (E0151);
    - an unconsumed `#SingleUse` endpoint;
    - a wrong-direction send.
12. Handle `State` (the retired `#State` and E0175) by a separate ruling. It is
    not bundled with protocol.

**Part 2: constant time (only if the ballot picks the plugin)**

13. Implement the §3.3 hook exactly, including the core short-circuit and the
    hard error when no validator is registered.
14. Move `ir_lint_fixed`, `Timing.jet` and `Web/Timing.jet` into the plugin.
    Keep `Taint.jet` E0737/8/9, `BuildMask.jet`, `slot_fixed`, the selection
    gating, the `#Scrub` no-inline rule, `HostTiming.jet` and E0744 in core.
15. Zero-regression evidence: Core/Crypto, Core/Net and Core/DB fixed code;
    all 58 Tests/Examples files; seeded-mutation ICE parity on x86-64 and
    wasm32; `jet perf` cold-compile results with zero fixed slots and on
    Core/Crypto.
16. Confirm the E0738 and E0739 UI fixtures exist. If they do not, add them
    **before** extraction, so parity has a baseline.

**Part 3: FFI**

17. Core keeps the §4.2 primitive set. The root `FFI` effect and the `#Unsafe`
    gate are recorded by the primitive.
18. Resolve the effect spelling and package-leaf namespacing by ballot. The
    library-keyed grant (`FFI.C("<lib>")` semantics, E0749/E1220/E1221) is kept
    under the new spelling. The `Effects.jet` 24-case enum shrinks to the
    surviving first-party set. `CoreSourceRows.jet`, `Core/Compiler/Lang.jet`,
    `BinderDescriptor.jet` and the 247 `FFI.<Lang>` lines in 84 files are
    migrated.
19. Delete the `#Close(close)` marker (`PreludeRows.jet:34`, `Foreign.jet:204`,
    `spec.md:5511`). `impl Close` is the only spelling. The fallible-close rule
    is ratified.
20. Move the orchestration out of the compiler (`ForeignLink`, `ForeignProviders`,
    `ForeignDeps`, the per-language `inspect bind` hosts), or record the reason
    each stays. Bridge identity stays in the build cache key.
21. Every moved diagnostic (E0701-E0705, E3201-E3208, E3221) keeps its code,
    its what/why/fix text and its snapshot.
22. Spec: `spec.md:5461-6030` shrinks to the C ABI primitives plus a pointer to
    the library docs. Per-language sections move with their libraries.
23. Python keeps the out-of-process worker as the default. In-process embedding
    is gated behind `#Unsafe`, with a GIL capability and borrowed/new reference
    typing, if the ballot admits it.
24. Set #7066 to be blocked by #7046 (criteria 2-6 open).

## 6. Ballots required (owner decisions; no implementation choice substitutes)

- **B1: `Validate` naming.** (a) Rename the builder (`Validate.over` → `FieldErrors.collect`).
  (b) Make the trait and the builder one type with an associated function.
  (c) Name the trait `Validated`. Trade-off: (b) merges the mechanisms but
  mixes a static builder into a trait.
- **B2: migration shape.** (a) Keep the grammar and remove only the dictionary
  noise. (b) Per-field markers `#Was(old)`/`#Default(expr)` plus a type-level
  `#Removed(field)`. (c) A #7046 DSL with an owned `Migrate` role and comptime
  snapshot reads. Edge cases for each option: historical names,
  LSP rename, the snapshot cache key, `via` staying.
- **B3: protocol mechanism.** (a) Keep it in core, since it is already a thin
  expansion over typestate. (b) Library code through an item-generating
  mechanism (new; I8 gate). (c) Type-level session lists (needs recursive
  generics, with worse errors).
- **B4: retired `#State` teaching.** Keep E0175 or retire it.
- **B5: constant time.** (a) Keep it in core and move the spec text to
  `Docs/Spec/Reference/` (recommended). (b) The §3.3 plugin hook with mandatory
  activation. Trade-off: (b) adds a frozen public IR API and per-pass hook cost
  for about 450 lines moved, and removes no user concept.
- **B6: first-party FFI set.** (a) C and Rust stay first-party, as O39 says.
  (b) Only the C ABI primitives stay, and C/Python/Rust become libraries, as the
  card says. (c) C stays and Rust moves.
- **B7: FFI effect spelling.** `FFI.C("<lib>")` (live), `c.FFI`, or `FFI(lang)`
  as a value (O23). Package-identity namespacing of leaves is required.
- **B8: Python mode.** Worker by default, in-process opt-in, or in-process by
  default, with the perf-cell mode stated.
- **B9: sealing third-party bridges.** How a non-Core bridge's `#Unsafe` gates
  interact with `deny: [Unsafe]` callers.
- **B10: fallible foreign close.** Use `Close` (`Never!`) plus an explicit
  `finish: fn(^^self) -> Unit E!`, or a fallible `Close`.

## 7. What this review did not do

- It did not build or run any compiler, test or benchmark. No tier behaviour is
  claimed from execution.
- It did not re-read peer sources this session: Django/Alembic, Serde,
  `session_types`/Ferrite, Scribble, CT-Wasm, GCC/LLVM plugins, Zig `@cImport`,
  Swift ClangImporter, bindgen and PyO3 are cited from general knowledge. The
  build's design note must cite them from primary sources.
- It did not check whether `Item.TemplateLoop`/`UserTemplates` can host
  protocol expansion (§2.3 attack 1).
- It did not locate the E0738 and E0739 UI fixtures by code string (§1.3).
