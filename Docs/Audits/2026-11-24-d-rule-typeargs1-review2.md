# D-RULE-TYPEARGS1 second adversarial review

- Date: 2026-11-24
- Reviewer: TypeArgsReview2 (fresh agent; did not write the ballot or review an earlier pass)
- Author model family: Claude. Adversarial model family: Claude.
- Ballot: `$JET_SCRATCH_ROOT/tmp/orch/ballots/D-RULE-TYPEARGS1.json` (edited in place; `Validate.jet` prints ok)
- Design section reviewed: `$JET_SCRATCH_ROOT/tmp/orch/land/2026-11-23-d-rule-typeargs1-design.md`
- Code truth: dev worktree `$HOME/Projects/Github/jet/.agent-worktrees/Main-dev-collectall` at d5f753f52c, read only.
  The ballot cites 4529a137b8. The only code change between the two is 4 lines in CheckRules.jet, so every cite still holds.
- Probe compiler: `$JET_SCRATCH_ROOT/tmp/orch/batch12-chain/s1/jet`. Machine: 32 threads, 61 GiB RAM.

## Verdict

**A holds within limits.** It is still the right choice over B, C and D. As written, though, it failed four claims: "the Interner is the table, equal types are one row" (F1), the stability of `name` (F2), the fn build tier (F3), and the completeness of its inventory (F4, F5, F7). The ballot text now repairs each of these. What still limits A: rules can't read argument types' fields or traits, every maker and reader changes in one cutover, `name` no longer echoes a written module path, and fn build needs a frozen table per program snapshot.

## Whole-mechanism inventory (attacked first)

I searched dev for every record, TypeId maker, reader, tier and tool that holds type facts, and for the sibling handles.

| Area | Ballot/design claim | Found | Verdict |
|---|---|---|---|
| Store | "The Interner is the table; equal types are one row; A adds no store" | `type_intern_syntax` (TypeIDs.jet:906-917) keys `.Named`/`.Apply` by `name_intern` of the **spelling**. Sema Types carry caller-relative spellings: bare `Point`, written `geo.Point`, checker keys `module:N:pkg:path.Point`, and `<corelib>/core.prelude.X` (Types.jet:1644-1700, QualifiedTypes.jet:1-10). `canonical` (TypeIDs.jet:76-78) ignores only map key spans, tags and Fn metadata. `sema_registration_seed_interner` (Items.jet:6696-6709, 2277) seeds both bare leaves and `"{module}.{leaf}"`. `functions: TypeSides<Type>` (TypeIDs.jet:112-114) stores whole written Fn trees as side rows. | **F1 fails** |
| Reflection source | "Every reflected fact is read from its row" | Reflection never reads the interner. `sema_comptime_reflection_type_id` (Reflection.jet:1519-1527) prints `type_name(qualified_type(...))` and `type_name(type_identity_unqualified_nominals(ty))`. TypeInfo is a parallel AST walk: `registered_module.declarations` scanned by name, then specialized by AST substitution (Reflection.jet:3038-3100). | **F1 fails** (claim overstated) |
| Makers | design §3.1 list | Missing: Reflection.jet:674 (MarkerArgInfo.type from marker param text), 1478 (setting type from text, caller -1), 2454 (`Unit` payload), 2456-2459 (named variant payload as `{a: T, …}` text; no interner kind for it), 2545 (builtin number TypeInfo identity), the `TypeId{T}` literal (Expressions.jet:8193-8209). Dead: `sema_comptime_reflection_written_signature` (793-797), no callers. Further callers of the one maker that the cutover touches: Reflection.jet:785-786, 1292, 1395, 1692, 1704, 2385, 2426, 2455, 2585, 2716, 2752; `declared_type_id` at 2880, 2952, 2993. Optional graph: `closure_info` (844-863) is reached from Statements.jet:6323 with `program.registration_graph`, an Option. CheckRules.jet:275 also passes the Option to `_type_id_in`. | **F4 fails** |
| fn build tier | "answered in the compiler on the session's checked program, which owns the same interner" | ProgramInfo is a snapshot value built by `jet_eval_compiler_services_program_info` (Pipeline.jet:6715-6748) in an earlier compile and stored in `load.build_program`. BuildAdapter answers `find_type`/`add_struct` from its text rows `root_types`/`scope_types` (BuildAdapter.jet:331-411, 639-651; CompilerServices.jet:1162-1205). `compiler.add` only queues source for a later compile (Session.jet:690-707). At BuildAdapter.jet:410 the new struct's TypeId is `_type_id_record("{home_module}.{name}", name)`, and no interner holds it. | **F3 fails** |
| Readers/tools | "SemIndexJSON one site; LSP; jet inspect" | Missing: JetLSP Completion.jet:49, Navigation.jet:312. JetDevServer Canvas: EditActions.jet:409; EditActions02.jet:238-239, 904; GraphProjection.jet:241, 561-577; QueryActions.jet:477-482, 805-811. SemIndexJSON.jet prints at 118, 146, 163, 173, 178, 187, 189 and 204. JetSemIndex Build.jet:344, 374, 956, 2233-2253, 2561-2563, 2634; Symbols.jet:305, 335, 612-653. All of these already share `type_name`. Moving them to rows needs #6793's TypeIDs in the semantic index. | **F5 fails** (lists incomplete; mechanism sound) |
| "TypeIds encoded in Dyn" reader | listed as a reader | Doesn't exist. `CompilerArg.value` is serde Dyn (CheckRules.jet:419-493). The only runtime TypeId is the Prelude `CasePayload.Single(TypeId)` (Prelude.jet:231). | **F7 fails** (claim inflated) |
| Identity spellings | "identity derived from the row" | Several spellings exist with no single rule: TypeId.identity `{module_name}.{leaf}` (Reflection.jet:1632-1636, 3062); TypeInfo.path and `reflect.of` path from the name ledger (`app.geo.Point`, metaprogramming.md:744-749); checker key `module:N:…`; trait payload keys hash the checked key (Calls/Reflection.jet:76-77); `specialized_identity` (2360-2362) unqualified while `declared_type_id` (2368-2375) qualifies; `Type` value path `type_name(qualified_type)` (TypeValues.jet:82-88). | **F6 fails**; probe below |
| Interner fork/merge | not mentioned | TypeIDs.jet:1-37: per-worker `interner_fork`/`interner_merge`/`InternerDelta` renumber ids at merge. | gap, repaired in text |
| SnapshotId | conforms, digest | Core/Compiler/Compiler.jet:101 `SnapshotID{priv digest}`; :116-123 `MetadataFailure.Stale(requested, current)`. SchemaFacts.jet:28 names `<corelib>/core.compiler.SnapshotID`. | holds; rename must cover SchemaFacts |
| ScopeId | becomes handle, renamed | Compiler.jet:108 `ScopeID{priv name}`; SchemaFacts.jet:40. | holds; rename must cover SchemaFacts |
| DeclarationHandle | second patch | Also SemIndexJSON.jet:226 emits `metadata_declaration_identity` as unavailable ("the semindex producer does not receive a checker-issued DeclarationHandle"). The design's §7 omits it. | gap, added to audit |
| NodeId, ValueHandle | nothing to cut over | No public record holds either. | holds |
| Records | CoreRecordRows, FactReads, CompilerOwned, Derives | Verified: FactReads TypeId sites 267, 299, 475, 511, 537, 569, 602, 629, 643, 655; CoreRecordRows 221, 223, 228, 230, 261, 303, 305, 359, 391, 400; CompilerOwned.jet:186; Derives.jet:2198-2203 (used 2215, 2251; TypeInfo record 2218-2222). | holds |

## Failed claims and repairs (all written into the ballot)

- **F1, interner keyed by spelling.** Repair: the one maker interns the *resolved* type, with each nominal head replaced by the checker's canonical key (the SemaCanonicalNominals / QualifiedTypes key). It never interns a caller-relative spelling, so maker rows are identity rows. This costs nothing over today, which already resolves every nominal through `qualified_type`. TypeInfo stays a declaration record, and only its type references become handles.
- **F2, `name` is a fact of the spelling.** Repair: `name` is printed from the identity row with leaf names. Named loss: `name` stops echoing a written module path (`geo.Point` prints `Point`). Mitigation: `identity` names the package and module. E2521's text is unchanged, because its snapshot types are local.
- **F3, fn build tier.** Repair: the ProgramInfo snapshot carries its own frozen table, stamped with its SnapshotId. `find_type` reads it, and `add_struct` appends a row. The next compile interns again by `identity`. A handle read against another table is interned again or fails `MetadataFailure.Stale`. Named loss: a second table instance per snapshot, in the same row format.
- **F4, missing makers.** Repair: every listed site calls the one maker, and `written_signature` is deleted. A named variant payload interns as a tuple row with field names. The optional-graph callers (Statements.jet:6323, CheckRules.jet:275) get a required graph.
- **F5, missing readers/tools.** Repair: the LSP, Canvas, SemIndexJSON and JetSemIndex sites are named in the ballot's reader list.
- **F6, identity unsound today.** Repair: `identity` is printed from the row and names the declared type's package and path, so it decodes back to one row. Trait payload keys keep hashing the checker key, so they are unchanged.
- **F7, Dyn reader.** Repair: the claim is removed from the ballot.
- **F8, cited paths didn't resolve.** `$JET_SCRATCH_ROOT` is `$JET_SCRATCH_ROOT`, so the design and probes live under `$JET_SCRATCH_ROOT/tmp/...`. Repaired in `detail` and `checkInstructions`.
- **F9, cost comparison not like for like.** The old "0.3-0.5 µs to spell one today" timed a single `type_name`. Today's maker prints twice, through `type_name(qualified)` and `type_name(unqualified)`. Measured below, A's maker cost is about the same as today's, not 2-3 times more. Repaired in `detail`, `losses` and `tradeoff`.
- **Edge (new).** Handles are minted only from the merged compile interner. A worker's delta ids are remapped at `interner_merge` before any record holds them. A table id is unique within a process (LSP and `jet dev` run many compiles), and only `identity` text leaves a compile.

## Probes I ran (load recorded)

All probes ran under `systemd-run --user --scope -p MemoryMax`, one at a time.

### Identity split (live dev bug)

`$JET_SCRATCH_ROOT/tmp/TypeArgsReview2/identity/main.jet`, `jet run main.jet`, at load 57-60:

```
Outer fields: top: identity=main.Point name=Point; geo_point: identity=main.M3GeoPoint name=M3GeoPoint
geo.Inner fields: p: identity=main.M3GeoPoint name=M3GeoPoint
TypeInfo identity: top main.Point  geo main.M3GeoPoint
pair main.Pair<geo.Point, main.Point>  bindings A=geo.Point,B=main.Point
```

One type (`geo.Point`) gets two identities in one compile: `main.M3GeoPoint` as a field and `geo.Point` as a generic argument. The field form is a compiler-generated name. Both break metaprogramming.md:748 ("never a name the compiler generates") and 888-890 ("two TypeIds name one type exactly when their identities are equal"). I also found that `geo.Inner.reflect()` in a top-level `#run` fails with Jet.ModuleAsValue. A one-handle design keyed by the canonical key removes the split. Today's text design can't.

### Read cost in the comptime child

I copied `$JET_SCRATCH_ROOT/tmp/TypeIDSlice3/probe-typetable/{plain,rule,table,table64}` to `$JET_SCRATCH_ROOT/tmp/TypeArgsReview2/probe-typetable/` and ran `run.sh`: 7 interleaved rounds of `jet check main.jet`. The results are in `times.tsv`. Load was 70-77.

| variant | min s | mean s |
|---|---|---|
| plain | 0.2763 | 0.2808 |
| rule | 1.2858 | 1.3214 |
| table | 1.2806 | 1.3069 |
| table64 | 1.3110 | 1.3351 |

- Per E2521 read (two row reads plus a compare): (table64 − rule) / 20000 / 64 = **19.7 ns** by minimum and **10.7 ns** by mean.
- The single-read `table` variant is not distinguishable from `rule`.
- Round-to-round spread (~0.07 s) is larger than the signal (~0.03 s). The design's "about 25 ns" is therefore an upper bound inside noise. The ballot now says 10 to 25 ns.
- Rule dispatch: (rule − plain) / 20000 = **50.5 µs** per heard call. The design measured 46.5 µs at load 13-16. The ballot now says 46 to 50 µs.
- Caveat: the probe's rows are a constant compiled into the rule package. A's rows arrive as batch static data, which is the same access path. Shipping cost per batch was not measured.

### Interning cost

`$JET_SCRATCH_ROOT/tmp/TypeArgsReview2/dev/Tools/ProbeIntern` is inside a `git archive HEAD` export of dev. It ran `jet run -O2 main.jet` three times (`intern-1.out` to `intern-3.out` in `$JET_SCRATCH_ROOT/tmp/TypeArgsReview2/`). I extended the original probe with today's full text maker (two `type_name` calls plus `type_identity_unqualified_nominals`) and a warm small table. Each type is `Transform2<Float, S_i, S_i+1>`.

| ns per type | run 1 (load 105) | run 2 (load 94) | run 3 (load 92) |
|---|---|---|---|
| intern new, cold 200k-row table | 1544 | 1211 | 1019 |
| intern hit, cold 200k-row table | 1114 | 727 | 668 |
| one `type_name` | 458 | 325 | 338 |
| today's maker text (two prints) | 886 | 647 | 622 |
| intern hit, warm 2k-row table | 353 | 223 | 222 |
| today's maker text, warm set | 905 | 455 | 447 |

- The design's 1.0-1.2 µs intern figure reproduces as an upper bound (0.67-1.1 µs hit, 1.0-1.5 µs new).
- Compared like for like, A's maker is about even with today's on a cold table and 2-2.5 times cheaper on a warm one.
- Both makers also pay the same nominal resolution, which was not timed.
- `find_hit` in the output is not a valid measure. `type_find_syntax` returns none for `.Apply` (TypeIDs.jet:731), so it timed an early return.

### Row size and batch size

- 6 rows: `Tests/UI/coordinate_transform_mismatch.jet` passes `Transform<Screen, World>` and `Transform<View, Camera>`. That is 2 Apply rows and 4 Named rows. No other `then` call is in the program (Core's only Transform `then` call is inside a `#Test` at Math.jet:44699).
- 68 B per row is the compile interner's own cost, not a batch row's. I checked it against TypeColumns and design §8.3. Columns `kind` (U8), `a`/`b`/`side`/`canonical` (U32) and `first`/`count`/`hashes` (Int) give 41 B. Slots give 524288 × 8 / 200019 ≈ 21 B, and children 300000 × 4 / 200019 ≈ 6 B. A batch `TypeRows` row is 17 B plus 4 B per child plus its text. The ballot said "6 rows of 68 bytes" after my first edit; that is corrected.

## Prior art (fair, with what each does that A doesn't)

- **C++26 P2996** (wg21.link/p2996): `template_arguments_of(^^T)` returns reflections you can query further. `std::meta::info` is consteval-only and never reaches run time, so it can never be stale. A lets handles reach running code, so it must ship a table and refuse staleness at boundaries.
- **Roslyn** (`INamedTypeSymbol.TypeArguments`; learn.microsoft.com SymbolEqualityComparer): a symbol belongs to one Compilation. Cross-compilation matching goes through a stable id such as `DocumentationCommentId` or the IDE's SymbolKey. That is A's split between handle and `identity`. Roslyn also gives the full semantic model (members, bases, interfaces) from any type symbol, which A withholds from rules.
- **Scala 3** (`quotes.reflect.TypeRepr.typeArgs`, `=:=`): TypeRepr is path-dependent on its `Quotes`, so the type system stops escape. A checks staleness dynamically. `=:=` also follows aliases and subtyping. A's equality is row identity.
- **rustc** (rustc-dev-guide "ty": `Ty<'tcx>` is `Interned<…TyKind>`, "types are always canonicalized and interned", compared with `==`): the closest analog to A. Cross-session identity is `DefPathHash`, a stable hash that includes the crate. That is what A's repaired `identity` must match: package plus path, not today's `{module_name}.{leaf}`. Proc macros get only `syn` tokens.
- **Swift SE-0382**: macros get syntax only. Type information was deferred because its representation would have to stay stable.
- **Zig**: `type` is a comptime value compared with `==`, and `@typeInfo` doesn't list generic arguments. Types never exist at run time.

## Open gaps

1. **Cross-package identity collision (unverified).** Today's identity is `{module_name}.{leaf}` (Reflection.jet:1635). I did not test whether two packages with the same module name collide. The repaired ballot requires identity to name the package. The exact spelling (for example, derived from the checker key minus its per-compile index) is left to the cutover.
2. The named-payload-as-tuple-row mapping changes the identity text of a variant payload from `{a: T}` to the tuple spelling unless the printer special-cases it. The cutover must pick one.
3. The cost of shipping a batch table, and the speedup from dropping today's three strings per TypeId, were not measured.
4. Nominal resolution cost (`qualified_type`) is common to both makers and was not timed.
5. The live identity split (probe above) and `geo.Inner.reflect()` → Jet.ModuleAsValue are dev bugs today, whatever this ballot decides.
6. The JetSemIndex and LSP move to rows depends on #6793 giving the semantic index TypeIDs.

## Files

- Ballot (edited): `$JET_SCRATCH_ROOT/tmp/orch/ballots/D-RULE-TYPEARGS1.json`
- This audit: `$JET_SCRATCH_ROOT/tmp/orch/land/2026-11-24-d-rule-typeargs1-review2.md`
- Probes: `$JET_SCRATCH_ROOT/tmp/TypeArgsReview2/{identity,probe-typetable,dev/Tools/ProbeIntern,intern-*.out}`
