# #7065 pre-build adversarial audit: text traits, self-documenting markers, Module/methods, `it`, division names, `#UnitFamily`

Date: 2026-11-27. Scope: card #7065 criterion 1. This was a read-only review of
the live tree, the Tower card, and the parent audit
`Docs/Audits/2026-11-27-cognitive-budget-orthogonality-audit.md` (O5, O7, O8,
O10, O11, O17, O30, O33).

Count method: `fd` file lists over `Compiler Core Examples Tests Tools
Jetpack` (plus `Docs/Spec` where noted), counted with `awk`. Three LSP fixture
paths that contain spaces could not be opened and are excluded from the cross-repo
totals. Totals marked "lines" count source lines that match the pattern. They
are not use counts.

## Verdict summary

|Part|Verdict|Blocking gaps|
|---|---|---|
|1 Text traits 7→4|**Go with corrections**|The card's "7" list leaves out `Describe`, which is a library trait in `Core/Prelude/Prelude.jet:175`. Folding `public`/`detail` into `Display`/`Debug` weakens the #7043 safety contract. Keep `Describe` out of the cut, or put it on a ballot.|
|2 Markers|**Go for purpose docs and hover. The opt-out needs a ballot. Hold Repeatable/Replayable.**|`#X: false` *is* the in-body spelling of `#!X` (`spec.md:1506-1507`), so deleting it deletes the in-body opt-out. `#Replayable` has no sema reader. `#!Copy` already conflicts with `#MoveOnly`/`#SingleUse` work on #7053.|
|3 Module/methods/`it`/division|**Go for Module, free methods, and `it`. Do not go on the division rename as worded.**|The card's "floor/trunc names" replacement for `div_euclid`/`rem_euclid` changes semantics for negative divisors and duplicates the `/%` and `%%` operators.|
|4 `#UnitFamily` → `distinct` + `Unit`|**No-go as specified.**|One surface already maps to one AST node, so there is no "duplicate". The rewrite has a high dimensional-safety risk, and `hour`/`day` are not live members.|

## Part 1: text traits

### Live evidence

- The compiler-owned traits are in `Compiler/Prelude/Traits.jet`:
  - `Printable` (205): derived, auto_derive, bundle.
  - `Serialize` (214): derived, through_elements, and no methods.
  - `Renderable` (222): derived, and no methods.
  - `Display` (230): `requires: "Printable Renderable"`.
  - `Debug` (240).
  - `Encode`/`Decode` (152/164): codec, family `Codable`.
- `Describe` is **not** compiler-owned. It is declared in
  `Core/Prelude/Prelude.jet:175` with `public` and `detail`, and documented at
  `spec.md:2187-2226`.
- Live line counts:

  |Name|Lines|Files|
  |---|---|---|
  |`Printable`|217|131|
  |`Serialize`|11|8|
  |`Renderable`|53|23|
  |`Describe`|203|80|

  - `Describe` in Core: 46 lines of `impl Describe`, `: Describe>` or
    `.Describe` across 30 files, plus 29 `.public()`/`.detail()` call lines.
  - Generic `fn<E: Describe>` boundaries:
    - `Core/Args/Args.jet:1371,1377,1433`
    - `Core/HTTP/Server.jet:369,1266`
    - `Core/Jobs/Jobs.jet:66,303,309`
- `spec.md:1205-1211` lists `Printable` as auto-derived and says "A `Display`
  implementation also satisfies a `Printable` bound". This is the edge that
  makes `Printable` redundant.
- `spec.md:1241` covers distinct forwarding:
  `#[Comparable, Printable] Score: distinct Int`.

### Attacks

1. **The "7" is miscounted.** O10 lists 7 compiler traits *plus* `Describe`,
   which makes 8 names. The card says "7 → 4" and the brief adds `Describe` to
   the deletions. Deleting 4 of 7 gives 3 plus 1 codec pair. That only works if
   `Describe` is counted separately. Fix the card text so the count matches
   what actually gets deleted.
2. **`Describe` is a security contract, not a formatting trait.** The
   requirement on `public()` is negative: it must never expose causes, paths,
   SQL, addresses or secrets (`spec.md:2201-2203`). `Display` has no such law.
   - If `public` becomes `Display`, every type's user text becomes the
     "caller-safe" text. A struct whose derived `Display` interpolates a path
     field would then leak that path through HTTP `respond_error`.
   - If `detail` becomes `Debug`, it inherits `Debug`'s auto-derive
     (`Traits.jet:246`). A missing hand impl would then silently produce a
     structural dump rather than the curated cause chain that `Err.detail()`
     builds (`spec.md:2211-2212`).
   - #7043 c1 (met) keys the `#Error` auto-lift on "`#Error` types implementing
     `Describe`". Folding `Describe` away changes that admission rule.
   - **Required:** keep `Describe` as the one two-audience error trait. It is a
     library trait, not compiler-owned, so it does not count toward the
     compiler's text-trait budget. Alternatively, put the fold on a ballot
     with a written leak law attached to `Display` for `#Error` types.
3. **Deleting `Printable` changes interpolation.** Interpolation currently
   accepts *any* value through derived `Printable`, and `Display` itself
   `requires` it.
   - After the cut, `{}` must bound on `Display`. Then either every type
     auto-derives `Display`, or interpolating a type without it becomes a new
     error.
   - Auto-deriving `Display` blurs user text with developer text, which is
     exactly the `Display`/`Debug` split the cut tries to sharpen.
   - **Ballot needed:** choose between "`Display` auto-derives structurally
     (today's `Printable` behaviour)" and "`{}` of a non-`Display` type is an
     error whose fix is `{x:Debug}`". The second option is stricter and
     touches far more code: 217 `Printable` lines plus every interpolation of
     a plain struct in Examples and Tests.
4. **`Renderable` → "Display into a target" and `Serialize` → `Encode` need
   their real behaviour checked first.** Both have no methods and are
   `derived: true`, so they are compiler-only facts.
   - The cutover must list every sema or codegen reader of the
     `Renderable`/`Serialize` facts: 53 and 11 lines respectively.
   - Each reader must be proven to need only `Display`/`Encode`. An HTML
     renderer that uses the structural tree needs `Encode` to a document codec
     (O10 trade-off). That codec must exist before `Renderable` is deleted, or
     the UI examples break.
5. **The bundle bit has to move.** `Printable` carries `bundle: true`, which
   is what makes `#[…, Printable]` on a distinct type work. `Display` has no
   `bundle`. Add `bundle` to `Display` in the same commit, otherwise
   `Score: distinct Int` loses its printing.

### Binding cutover list (Part 1)

- `Compiler/Prelude/Traits.jet`:
  - Delete `Printable`, `Serialize` and `Renderable`.
  - Change `Display.requires` to empty and add `bundle`.
  - Choose the auto-derive behaviour per attack 3.
- Rerender `OwnedTraitRows.jet` with `Tools/Agent/OwnedTraitRows.jet --check`.
- `Compiler/Prelude/Markers.jet:376`: delete the `Printable` marker.
- Update `spec.md:1205-1211` and `spec.md:1241`.
- Update the trait rows in `Docs/Spec/Reference/`.
- Sweep all 217 `Printable`, 53 `Renderable` and 11 `Serialize` lines.
- Update every UI snapshot that names those traits, using E0905 messages as
  the starting point.
- Keep `Describe` unless a ballot rules otherwise, and attach the #7043
  auto-lift regression test as a criterion.

## Part 2: self-documenting markers, a single opt-out, and family merges

### Live evidence

- `Compiler/Prelude/Markers.jet` declares 83 top-level marker structs.
  **35 of them have no leading comment**, for example `Comparable`,
  `Equatable`, `Debug`, `Numeric`, `Printable`, `SingleUse`, `Replayable` and
  `UnitFamily`.
- No `purpose` field exists today: there are 8 `.purpose` lines repo-wide,
  none of them in Markers.jet.
- LSP hover (`Compiler/JetLSP/Source/LSP/Navigation.jet:27-140`) renders
  `symbol.signature` and `symbol.summary` for declarations. Navigation, Facts
  and Requests contain **no** marker handling at all, so hovering `#Codable`
  shows nothing today.
- The opt-out spellings:
  - `#!X`.
  - `#X: false`, which is defined as "`#X: false` is `#!X`" for in-body
    marker lines (`spec.md:1504-1513`). Markers above and inside the same
    declaration together are E0942.
  - `policy: { lints: { deny: [auto_derive] } }` (`spec.md:1207-1208`):
    197 `auto_derive` lines in 55 files.
- The move/use markers are `SingleUse` (`Markers.jet:411`, 181 lines in 79
  files) and `MoveOnly` (`:420`, 28 lines in 11 files). `#Linear` has 0
  lines; it does not exist.
- `Replayable` (`:216`) has no fields and no comment. It appears on 8
  `#Replayable` lines. JetSema has **0** `MARKER_REPLAYABLE` reads, against 6
  for `MARKER_REPEATABLE` and 27 `#Repeatable` lines. `spec.md` has no
  `Replayable` text.

### Attacks

1. **Deleting `#X: false` deletes the in-body opt-out.** The card says
   "replace `#Trait: false` … with single `#!Trait` (preserving in-body
   markers)". In-body lines are grammatically `#X: value`, and `false` is the
   only way to write `!` there. One of these must be ratified:
   - (a) Allow a bare `#!X` line inside a body. This is a new in-body form,
     and the parser must accept a value-less marker line.
   - (b) Treat `#X: false` as positional grammar for `#!X`, not as a second
     opt-out, and delete only `deny: [auto_derive]`.

   Option (b) honours "preserving in-body markers" with zero new syntax.
   Recommend (b). Either way it needs a **ballot** because it is user syntax
   (I7).
2. **`deny: [auto_derive]` is not a per-type opt-out.** It is a package-wide
   flip to opt-in (`spec.md:1246`: "under a package that denies `auto_derive`,
   `#Codable` asks for it").
   - Deleting it removes the only way for a package to say "no implicit
     derives", which security-sensitive packages use to keep wire shapes out
     of accidental `Encode`.
   - Replacing it costs one `#!T` per type per trait. Count the 197 lines and
     their packages before deleting.
   - It may be better to keep it as a policy, not a marker, because policies
     tighten and do not duplicate.
3. **`#!Copy` vs `#MoveOnly`/`#SingleUse`.**
   - `#!Copy` means "move-only" only after #7053 (structural Copy, still
     `ready`) lands. Before then there is no Copy auto-derive to opt out of.
     **Part 2's merge is blocked by #7053.**
   - `#SingleUse` (linear, use exactly once) is a stronger fact than
     move-only. It cannot be spelled as an opt-out, so `#Linear` would be a
     *rename*, not a deletion. That is net zero under I8's 1-in/1-out rule.
     Recommend keeping `#SingleUse` and deleting only `#MoveOnly` in favour
     of `#!Copy`.
   - Sweep all 181 `#SingleUse` and 28 `#MoveOnly` lines.
4. **`#Repeatable`/`#Replayable` are not shown to overlap.**
   - `Repeatable` means idempotence (`Markers.jet:222-226`) and has a sema
     checker.
   - `Replayable` has no checker, no spec text and no comment.
   - Merging an undocumented marker into a checked one would give 8 call
     sites a checker they never opted into.
   - **Required:** before merging, find what `#Replayable` is meant to mean,
     or confirm it means nothing. If it is a dead marker, *delete* it
     (8 lines) rather than merge it.
5. **What `purpose` costs.**
   - A `purpose: String` field on `impl Marker` is metadata, not a use-site
     argument, so it must not become a typed field users can pass. Put it in
     the `impl Marker { … }` settings block next to `sites`.
   - Alternatively, use the leading `//` comment as the source. That gives one
     source of truth, but comments are not data today.
   - Recommend a `purpose` setting checked by a closed-list guard: every
     marker must have one, so the 35 undocumented markers fail the guard
     until they are filled in.
   - Hover and Go-to-Definition need a marker-name resolver in Navigation,
     because none exists. Go-to-Definition should target the `Markers.jet`
     struct span.
   - The existing LSP is a JSON-text server, so this needs LSP case tests
     under `Tests/Cases/LSP/`.

### Binding cutover list (Part 2)

- `Compiler/Prelude/Markers.jet`:
  - Add a `purpose` setting to all 83 markers, including the 35 that have no
    comment today.
  - Remove the `Printable` marker (from Part 1), the `MoveOnly` marker (after
    #7053), and `Replayable` (after its meaning is proven).
- Add `Tools/Agent/` registry rendering and a guard so that a marker without a
  `purpose` fails the build.
- In `Compiler/JetLSP/Source/LSP/Navigation.jet`, add marker hover and
  definition. Add LSP fixtures for hover and definition on `#Codable`, `#!Debug`
  and an in-body `#Debug: false` line.
- Update `spec.md:1205-1213`, `1246` and `1504-1513` according to the ballot
  outcome.
- Delete or keep the `auto_derive` lint per the ballot.
- Rename the UI snapshots for E0942 and E0943.

## Part 3: unified Module, method sites, `it`, and division names

### Module and comptime parameters (O5)

- **Evidence:** the `Item` variants (`Compiler/JetFoundation/Source/AST/Items.jet:1055`)
  include `Module`, `CodeModule`, `GenericModule`, `ModuleAlias`, `CModule` and
  `ExternRust`.
  - Line counts: `GenericModule` 529 lines in 136 files, `ModuleAlias` 417 in
    136, `CodeModule` 525 in 147.
  - Source heads: 74 generic-module heads and 1195 `module {` heads.
  - E0629 (`DiagnosticRows.jet:8271`, `Jet.ModuleAsValue`): 10 lines in 8 files.
- **Attack: the registry text is already inconsistent.** The E0629 `why` says
  "A module … has no value to bind, pass, return, store or print". But
  `spec.md:4336` says every user module *is* a `Module` value, and E0629 fires
  only for Core modules (`spec.md:4345-4347`). That is an I4 text defect
  today.
  - Eliminating E0629 requires Core modules to get `Module` records. That
    means `doc`, `file` and `package` must be filled for embedded Core
    sources, which have no package root.
  - Define what `file` is for an embedded Core module: a stable
    `core/<path>` string, never a host path.
- **Attack: one comptime parameter syntax.** Generic types use `<T>` and
  generic modules use `billing<pricing>` with a module argument. If the
  single syntax also covers value parameters, a module argument has to be
  distinguishable from a type argument without lookahead. Write that grammar
  rule down before building.
- **Cutover:**
  - Fold `GenericModule` and `ModuleAlias` into `Item.Module`.
  - Retire E0629, keeping the code number as retired, never reused.
  - Update `spec.md:4336-4348` and `4418-4468`.
  - Sweep the 74 generic-module heads.
  - Handle the `CodeModule` vs configuration `Module` split per O5. This is
    internal, but it touches 525 lines.

### Method sites (O8)

- **Evidence:** only **11** free `Type.method: fn` sites exist, all under
  `Tests/` (`awk` pattern `^[ \t]*(pub |priv )?[A-Z]…\.[a-z_]…: (#run )?fn`).
  Core, Examples and the compiler use none.
- **Attack:** low risk. The real cost is the parser path
  (`Compiler/JetParser/Source/Parser/ImplPlacement.jet`) and the formatter
  path, plus any `jet fix` migration. With 11 sites, a `jet fix` rule is more
  code than migrating by hand. Delete the parse rule, add a parser diagnostic
  whose fix suggests `impl Type { … }`, and migrate the 11 sites by hand.
- **Cutover:**
  - Remove the parser rule and add a diagnostic plus UI snapshot.
  - Migrate the 11 test sites.
  - Update `spec.md:1031-1083`.

### Dropping `it` (O30), keeping `<>`

- **Evidence:**
  - `KW_IT` is at `EffectsSurface.jet:219`, `TOKEN_KW_IT` at
    `JetLexer/Source/Lexer/Tokens.jet:322`.
  - It is accepted by the parser in `Expressions.jet:345,1190`,
    `Statements.jet:2203`, `ColonDeclarations.jet:121` and `Loops.jet:360-375`.
    The bindingless collecting loop `[loop xs -> …]` binds `"it"` at
    `Loops.jet:375`.
  - Other references: `Highlights.jet:297`, `PackageFiles.jet:492`,
    `JetEval/Source/CompilerServices.jet:446`,
    `JetCLI/Source/CLI/SyntaxDictionaryData.jet:95`, and the migration
    `Migrations/BitSigils1.jet:267`.
  - Spec: `spec.md:794-796`.
- **Live usage:** after stripping comments and strings, the scan found **0**
  real `it` uses in Compiler, Core, Examples, Tests, Tools and Jetpack. The
  only hit is `Tests/UI/E0107_it_outside_a_loop.jet`.
- **Attack: removing `it` also removes the bindingless loop form.** Without
  `it`, `[loop xs -> f(it)]` has no name for the item, so the bindingless
  branch (`Loops.jet:366-375`) must be deleted together with the keyword.
  This overlaps #7049's comprehension work. Coordinate so #7049 does not
  re-add a bindingless form.
- **Replacement for non-name `if` subjects:** bind the subject first
  (`r: call()` then `if r == { … }`). This needs no new syntax.
- **`<>` is kept on both sides:** the pipe value and the `if` hole table both
  stay (`Spellings.jet:73`, `MathLayout.jet:782`; 79 source lines use `<>` in
  `if` headers). The card says to keep both. O30's proposal to make `<>`
  "pipe-only" is **overruled by the card**. Record that the hole-table
  meaning stays, and do not let a worker apply O30's text.
- **Cutover:**
  - Delete `KW_IT` and `TOKEN_KW_IT`, and every parser, lexer, highlight,
    eval and CLI dictionary row listed above.
  - Delete the bindingless loop branch.
  - Leave `it` as an ordinary identifier, which frees the name.
  - Re-point the E0107 UI snapshot `E0107_it_outside_a_loop.jet`: delete it,
    or keep it as the plain unknown-name case.
  - Update `spec.md:794-796`.
  - `BitSigils1.jet` is a frozen migration: decide whether it may still name
    the token. If tokens are deleted, the migration must stop compiling
    against them.

### Division method names: the card's wording is wrong

- **Evidence:**
  - The operators are `/%` = floored quotient, `%` = floored modulo,
    `%%` = truncated remainder, and `/` (`MathLayout.jet:409-421`).
  - The methods are `div_euclid`/`rem_euclid` (`Spellings.jet:246-248`).
    Their uses are 92 + 14 lines, almost all in compiler internals (JetBackend
    28, JetDebug 26, JetIR 12, JetCLI 8, JetFoundation 7), with Examples/Math
    at 6 and Examples/Text at 2.
- **Attack: Euclidean is neither floor nor trunc.**
  - For a negative divisor, `-7 div_euclid -2 = 4` (remainder 1 ≥ 0), while
    floored `-7 /% -2 = 3` (remainder −1) and truncated gives 3.
  - For a positive divisor, Euclidean equals floored, which is already spelled
    `/%` and `%`.
  - Renaming the Euclid methods to `div_floor`/`rem_trunc`:
    - silently changes results for negative divisors, an I5/I9 correctness
      regression; and
    - adds a second spelling for what `/%`, `%` and `%%` already do, which
      violates I8 "one mechanism".
- **Recommendation (ballot):** keep `div_euclid`/`rem_euclid` as the *only*
  Euclidean spelling. Euclidean is a real third rounding mode, used for
  hashing and modular indexing. Do not add floor/trunc methods.
  - If the owner wants clearer names, rename in place to names that still say
    Euclidean (for example `div_nonneg`/`rem_nonneg`), with a golden test
    covering a negative divisor.
  - Note also that the card keeps `/` and `/%` but says nothing about `%%`.
    The ballot must say whether `%%` stays.
- **Cutover (if a rename is ratified):**
  - `Spellings.jet:246-248`.
  - All 106 lines.
  - An `Examples/Math` golden showing `-7` divided by `-2` in every mode.
  - O0, O1 and O2 runs.

## Part 4: `#UnitFamily` → `distinct` + `Unit` trait

### Live evidence

- **There is one surface and one node.**
  - `#UnitFamily(Name, dimension…, base:) { members }` is parsed by
    `parser_parse_unit_family` (`JetParser/Source/Parser/Declarations/Types.jet:260-428`)
    straight into `Item.UnitFamily`.
  - The `UnitFamily` struct in `Markers.jet:401` (and
    `PreludeMarkers.jet:407`) only registers the marker's argument shape.
  - O7's claim that it is "spelled both as an Item and as a marker" is
    **false**. There is no duplicate surface: one surface spelling produces
    one AST node.
- **Footprint:**
  - `UnitFamily` appears on 781 lines in 324 files and on 214 `#UnitFamily(`
    lines (Tests/Cases 130, Compiler/Prelude 34, JetSema 34, Tests/UI 22).
  - The sema checker is `Sema/Units/{Dimensions,Mentions,Prelude}.jet`
    (952 lines) plus `Registration/PreludeUnits.jet` (936 lines). That is
    1888 lines of bespoke checker.
  - The parser enforces exact rational `scale`, affine `offset`, a base member
    whose scale is 1 and offset is 0, and derived dimensions that need a base
    (`Types.jet:342-424`).
  - Measured constants with uncertainty and provenance are also supported
    (`Units.jet:67`, `measured(…, uncertainty:, source:)`).
- **Time:**
  - `#UnitFamily(Time, dimension, base: ns)` (`Compiler/Prelude/Units.jet:73-82`)
    has members `ns us ms s min h d week`, stored as i64 nanoseconds.
  - The members are `h` and `d`. **`hour`/`day` are not live members.** Any
    criterion that tests them tests a nonexistent surface. Use `h`/`d`
    literal suffixes, or the `Duration` constructors the comment names.

### Attacks

1. **The trait rewrite risks losing dimensional safety.** Today the dimension
   algebra (`Length * Length / Length / Length` for `SolidAngle`, `Units.jet:228`)
   is a closed checker that runs on every arithmetic expression. A `Unit`
   trait with comptime dimension algebra needs:
   - Associated comptime values on traits: an exponent vector per base
     dimension.
   - `Mul`/`Div` impls whose `Output` type is *computed* from the operand
     dimensions. That is type-level arithmetic through `Output: type`
     (`Traits.jet:85-104`), and it has no precedent in live trait resolution.
   - Proof that `Length/Length` normalises to the dimensionless `PlaneAngle`
     and still stays nominally distinct, as the Prelude declares today
     (`Units.jet:197`).
   - Affine handling: `Temperature` with an offset cannot be multiplied, and a
     difference of two affine values is a delta. A trait form has to encode
     the point/vector split that the bespoke checker carries.
   - Exact rational conversion with provenance for measured constants.

   Any gap here is a silent unit-unsafety bug, which is **I1**. The card's own
   c4 gate ("zero dimensional-safety loss") cannot be met on argument alone.
2. **Conversion inference.** Literal suffix resolution (`5km`, E-code at
   `DiagnosticRows.jet:2448-2454`) depends on scanning in-scope family members
   (`JetFoundation/Data/Syntax/Predicates.jet:6`). With `distinct` types,
   every suffix becomes a value or constructor lookup across all distinct
   types that implement `Unit`. Ambiguity rules are needed when two families
   export the same suffix, for example `m`, or `min` vs a user `min`
   function.
3. **Compile-time overhead.** The current checker is closed-form. Comptime
   trait evaluation per arithmetic node conflicts with the 30M LOC/s cold
   compile target. **Required:** a two-program `jet perf` cell (Prelude units
   plus a unit-heavy example) comparing cold check time before and after, as
   the perf-motivated-syntax rule demands.
4. **I8 gain is small.** Deleting one `Item` variant removes about 25 match
   arms (25 files touched in Parser and Foundation alone), but it moves the
   1888-line checker into comptime library code plus new trait-resolution
   power.
   - "1-in, 1-out" is not met: it deletes `Item.UnitFamily` but adds
     type-computing associated types, which is a new mechanism.

### Recommendation

- Narrow c4 to the honest delta:
  - Fix O7's evidence. There is no marker/item duplicate.
  - Express `#UnitFamily` as a *declaration marker that elaborates into*
    `distinct` types plus a library-visible `Unit` trait impl, keeping the
    closed dimension checker in sema (I3).
- That gives users one mental model ("a unit is a distinct number with a
  `Unit` impl") without moving safety-critical algebra into comptime trait
  resolution.
- Put the full rewrite on a **ballot**, gated on:
  - the perf cell;
  - a dimensional-safety corpus: every `Tests/Cases` unit case (130 lines)
    plus the 22 UI units snapshots, all passing unchanged on O0, O1 and O2.

### Binding cutover list (Part 4, if a ballot ratifies the rewrite)

- `Item.UnitFamily` and its match arms. 25 files were listed by `grep`; the
  key ones are:
  - `AST/Items.jet:9,1055`
  - `Facts/Declarations.jet:213`
  - `SyntaxCodec.jet:340,440`
  - `Target/Frame.jet:66,307`
  - `Target/Layout.jet:706,1020,1081`
  - Formatter: `Formatter.jet:620,672,1928,1993,2074` and `Items.jet:517,758`
  - Parser: `Types.jet:161-428`, `Modules.jet:404,501`,
    `InBodyMarkers.jet:521`, `StateMarkers.jet:118,144`,
    `ItemHeads.jet:45`, `ConfigProgram.jet:253`, `ImplPlacement.jet:45`
  - Parser tests
- Port or retire `Sema/Units/*` and `Registration/PreludeUnits.jet`.
- Rewrite `Compiler/Prelude/Units.jet`: 34 families, Time included, keeping
  i64-ns `Duration` identity.
- Reword the suffix diagnostic at `DiagnosticRows.jet:2448-2454`.
- Update `MARKER_UNIT_FAMILY` in `PackageFiles.jet:141-149`.
- Update `Examples/Types/Units/*` goldens and the reflection examples
  (`Examples/Metaprogramming/Reflection/*.jet`).
- Run the full suite.

## Cross-card dependencies

- **#7053 (ready):** must land before the `#!Copy`/`#MoveOnly` merge.
- **#7049 (ready):** comprehension and `if` work must not re-add a bindingless
  `it` loop. It must also respect the card's ruling that `<>` stays in `if`
  tables.
- **#7043 (done):** the `#Error` + `Describe` auto-lift must keep its met
  behaviour. Add a regression criterion on #7065 c2.
- **#7056 (ready):** the Core export source of truth must give Core modules
  `Module` records before E0629 can be retired.
- **#7046 (ready):** DSL traits may depend on `Display` and `Renderable`.
  Check them before deleting `Renderable`.

## Items that need owner ballots

1. Remove or keep `Describe`, and where its leak law lives if it is folded.
2. Whether `Display` auto-derives (replacing `Printable`), or `{}` of a
   non-`Display` type becomes an error.
3. In-body opt-out spelling: `#X: false` kept as positional grammar, or a
   bare `#!X` line.
4. Whether the `deny: [auto_derive]` package policy is kept.
5. `#SingleUse` kept vs renamed to `#Linear`. Rename only if a deletion pays
   for it.
6. Division: keep `div_euclid`/`rem_euclid`, rename while keeping Euclidean
   semantics, or something else. Also whether `%%` stays.
7. `#UnitFamily`: the narrowed elaboration form vs the full trait rewrite,
   gated on the perf cell and the safety corpus.
