# Core record census (2026-11-12, #6328 criterion 22, slice S2)

Criterion 22 ships Core and Prelude inside the jet binary as one checked
record (`jet.core/v1`, Docs/proposals/precompiled-libraries-2026-11-05.md
section 4.1). The compiler then reads Core from that record and never parses
or checks Core source per program. This census lists every place sema, IR,
the driver and comptime evaluation read module AST or registration state.
Any of those reads can reach a Core module while a user program is handled,
so together they fix what the record must hold.

All locations are at dev `ab2d2ab82b`. The site list is
`2026-11-12-core-record-census.tsv`, one row per source line, with columns
file, line, enclosing function, class, record section and the code.

## Method

The census covers every `.jet` file under `Compiler/JetSema/Source`,
`Compiler/JetIR/Source`, `Compiler/JetDriver/Source` and
`Compiler/JetEval/Source`. Comments are stripped first. A line becomes a site
under the first class whose pattern it matches:

| Class | Pattern (on one line) | Record section that serves it |
|---|---|---|
| graph-walk | `loop x in …modules {`, `loop x in …modules ->`, `loop x in 0..<…modules.len()` | module table, then reach |
| body-read | `.declaration.body`, `.func.body`, `.function.body` | checked bodies |
| default-expr | `.default`, `.declaration.value` | compile-time values |
| module-ast | `.program.items`, `.program.<field>` | declaration AST |
| declaration-ast | `.declaration` | declaration AST |
| source-text | `.source_segments`, `.source_file`, `.source.slice/lines/bytes/byte_at` | locations |
| impl-table | `.impls`, `.impl_conditions`, `.associated_type_impls` | interface: impl rows |
| nominal-table | `.nominals`, `.variants`, `.trait_methods` | interface: types |
| constant-table | `.constants` | compile-time values |
| import-export | `.exports`, `.imports`, `.namespace_names/refs/private` | module table |
| function-table | `.functions`, `.signature` | interface: functions (sema); checked bodies (IR, eval) |

Sema registers and checks Core and user modules in one graph, with no field
that tells them apart, so a pattern match can't separate a Core read from a
user read. Every site therefore counts: each one runs over Core rows whenever
the graph holds Core. The census is an upper bound. It doesn't miss a read
that goes through these field names, but it can't see a read through a helper
that returns a whole row and is then destructured.

## Counts

| Class | JetSema | JetDriver | JetIR | JetEval | Total |
|---|---|---|---|---|---|
| graph-walk | 215 | 105 | 0 | 4 | 324 |
| module-ast | 339 | 76 | 15 | 6 | 436 |
| declaration-ast | 465 | 73 | 0 | 2 | 540 |
| default-expr | 81 | 11 | 1 | 0 | 93 |
| body-read | 6 | 0 | 0 | 0 | 6 |
| function-table | 332 | 55 | 506 | 27 | 920 |
| nominal-table | 496 | 4 | 12 | 0 | 512 |
| impl-table | 186 | 6 | 0 | 0 | 192 |
| constant-table | 94 | 5 | 9 | 1 | 109 |
| import-export | 175 | 37 | 0 | 0 | 212 |
| source-text | 250 | 44 | 66 | 9 | 369 |
| total | 2639 | 416 | 609 | 49 | 3713 |

The 3713 sites sit in 1324 functions. The 324 graph walks sit in 225
functions. The walks with the most sites are `sema_finalize_program` (12),
`sema_registration_prepare_graph` (8), `sema_compiler_semantic_index_from_checked` (7),
`jet_driver_compile_loaded_stages` (5), `jet_driver_core_module_order` (5),
`sema_effect_file_module_caps`, `sema_local_value_splice`,
`sema_finalize_walks`, `jet_driver_resolve_entry_key` and
`jet_driver_load_reach` (4 each).

## Findings

1. **Registration rows carry AST.** The rows sema builds for every module
   hold parser trees: `SemaRegistrationFunction.declaration: Func` (its
   signature and its body), `SemaRegistrationImpl.declaration: ImplDef`,
   `SemaRegistrationConstant.declaration: ConstDef`,
   `SemaRegistrationTraitMethod.declaration: TraitMethodSig`,
   `SemaRegistrationAssociatedType.declaration: NamedType`, the
   `declaration: Item?` of exports and import bindings, and each field's
   `default: Expr?` (Registration/Items.jet:418-560). The 540
   declaration-ast and 436 module-ast sites read those trees after
   registration. The record must therefore hold the declaration AST of every
   Core item, not only its types. Typed signatures alone would not serve
   them.
2. **Finalization edits registration rows in place, per program.**
   `SemaGraphTrail` (CheckProgram.jet:6478-6520) lists every in-place edit
   that the declaration pass, the comptime run and finalization make:
   - staged values typing symbols, constants, items and import bindings
   - function, method and impl-method rows the failure solve rewrites
   - error types minted by appending rows to a module
   - the counting switch
   - codec bodies that a codec wave fills on demand (`CodecBody`,
     `sema_codec_set_body` in Registration/SerdeItems.jet:1720-1735)

   For a Core row, each of these edits is a fact computed per program. The
   record holds them already applied: every Core staged value is typed,
   every Core failure projection is solved, every Core error type is minted,
   and every derived Core codec body is filled. S1 already refuses a Core
   read of a per-program fact, and this is the list of those facts.
3. **Whole-graph walks are the cutover list.** The 324 graph-walk sites loop
   over every module of the graph, Core included, on every compile. After
   the cutover each one has to walk the program's own modules and read Core
   rows from the record as they are reached. None of them may walk the
   record whole. The site rows name the 225 functions S5 changes.
4. **IR and eval read checked function bodies.** 506 of IR's sites and 27
   of eval's read `.functions`, the flat checked bodies (`FnBody`) in
   `SemaGraphModuleResult.functions` (CheckProgram.jet:259-305) that IR
   lowers and comptime runs. For Core functions these become the record's
   checked-bodies section, together with each body's side facts:
   `failure_facts`, `call_facts`, `compute_facts`, `copy_events`,
   `declarations` (`TDeclarationFacts`) and the graph's
   `memory_declarations` and `memory_projections`.
5. **A few reads inspect a body.** Six sites read a function's body
   directly. HTTP route shapes read a handler method's body
   (Calls/HttpRouting.jet:115 and :149). The divergence check walks a body
   (Divergence.jet:63). The memory plan compares body lengths
   (CheckProgram.jet:2690). The codec fill writes and tests bodies
   (Registration/SerdeItems.jet:1615 and :1727). The first three see Core
   only through a Core function a user route or call names, so the
   checked-bodies section serves them.
6. **Source text is for diagnostics, plus marker and splice rules.** The
   369 source-text sites read `source_segments` and the source file. Most of
   them render a span. The rest are marker rules and local splices, which
   read declared text blocks. A Core text block is consumed when Core is
   checked, so the locations section has to hold only spans and file names,
   never Core source text.

## What `jet.core/v1` holds

Each line names the census classes that read it.

1. **Header:** schema, compiler identity, target, and the digest of the Core
   and Prelude sources.
2. **Module table** (graph-walk, import-export): one row per Core module
   with its identity, name, package identity, path and file. The row holds
   its exports and its imports with their bindings, and its namespace names.
   The table is sorted by module name so a lookup can binary-search it.
3. **Interface rows** (nominal-table, function-table, impl-table): the
   `SemaRegistrationNominal`, `SemaRegistrationFunction`,
   `SemaRegistrationImpl`, `SemaRegistrationConstant`, and the checked
   foreign function and provider rows, with their symbols, types,
   identities, identity keys, generic parameters, parameters and ABI. Impl
   rows are keyed by type module and trait module, so the reach rule
   (`jet_driver_core_impl_reached`) runs on them unchanged.
4. **Declaration AST** (declaration-ast, module-ast): the `Func` signature,
   `ImplDef` header, `ConstDef`, `TraitMethodSig`, `NamedType` and `Item`
   trees the rows above point at, as finalization left them (finding 2).
5. **Compile-time values** (constant-table, default-expr): every Core
   constant and field default already evaluated, plus the binary assets.
6. **Checked bodies** (body-read, IR and eval function-table): every Core
   function's `FnBody`, filled codec bodies included, with the side facts
   listed in finding 4. Generic bodies keep their type parameters open.
7. **Locations** (source-text): declaration and body spans and file names,
   used only to render diagnostics.

Nothing in the record depends on a user program. Per-program state stays out
of it: `SemaGraphResult.core_usage`, name references from user code, comptime
requests from user code, and the trail of the user modules' own edits.
