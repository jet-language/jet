# Block Footprint Design Review (#7033)

Date: 2026-11-26. Card #7033 ("Show what a block reads and writes").

Subject: owner-approved tooling design (zero new syntax) that infers what any
checked block or line range reads, writes, and takes from compiler-checked
facts, exposes it through `jet inspect footprint FILE:LINE[-LINE]` (human and
`--json`) and through JetLSP (hover / code lens for the inferred footprint,
plus an `Extract function` code action whose generated parameters carry that
exact inferred `x: T`, `x: ^T`, `x: ^^T` footprint).

Sources audited in tree:
- `Compiler/JetFoundation/Source/Facts/Body.jet` (`FnBody`, `BodyTree`, `NodeKind`, `BodyCallArg`, `BodyLambda`, `BodyParameter`, `BodyPlace`)
- `Compiler/JetFoundation/Source/Facts/BodyStmt.jet` (`StmtKind`)
- `Compiler/JetFoundation/Source/Facts/Checked.jet` (`TLocal`, `TFieldRef`, `TMethodRef`, `TBuiltinOp`, `TCapture`)
- `Compiler/JetFoundation/Source/Registry/BuiltinMethods.jet` (`BUILTIN_METHOD_ROWS`)
- `Compiler/JetFoundation/Source/Collections.jet` (`COLLECTION_OPERATIONS`)
- `Compiler/JetSema/Source/Sema/Footprint.jet` (`sema_footprint_solve_module`, `FnBody.receiver_footprint`)
- `Compiler/JetSema/Source/Sema/Calls/Checking.jet` (`sema_call_lower_bound_arguments`, `sema_call_marked_move_place`, `sema_call_marked_move_value`)
- `Compiler/JetSema/Source/Sema/Calls/Methods.jet` (`sema_ownership_check_method_call_with_footprint`, receiver pinning and lowering)
- `Compiler/JetSema/Source/Sema/Ownership/CopySites.jet` (`NodeKind.Clone` vs actual transfer)
- `Compiler/JetDriver/Source/Driver/Identity.jet` (`JetDriverDocModule`)
- `Compiler/JetSemIndex/Source/SemIndex/Types.jet` (`SemIndex`, `SemCallable`, `SemDef`, `SemRef`, `SemMember`)
- `Compiler/JetLSP/Source/LSP/{Check,Navigation,Commands,CodeActions,Hints}.jet`

## 1. Where reads, writes, and takes are already computed

An audit of the compiler shows five existing truth sources, all converging onto
the checked `JetDriverDocModule` bundle already produced by `JetDriverCompileMode.Doc`
for both `jet inspect` and `JetLSP`:

1. **Method receiver footprints (`Compiler/JetSema/Source/Sema/Footprint.jet`, `FunctionCheck.jet:1162`)**:
   `sema_footprint_solve_module` solves every struct/impl method's stored-field
   footprint (`^self.[a, b]` when stated and checked by `E0231`, or inferred to
   a fixpoint over sibling `self` calls when unstated), and `FunctionCheck.jet:1162`
   copies that solved footprint onto `FnBody.receiver_footprint: [String]?`
   (`Body.jet:737`). Because `FunctionCheck.jet:890` omits `self` from
   `FnBody.params`, method receiver convention and owner type are carried on
   `FnBody.kind` (`TFuncKind.Method` / `TFuncKind.TraitMethod`). Reading
   `fn_body.kind` and `fn_body.receiver_footprint` directly requires zero
   re-solving. `None` means the method touches whole `self` or takes a consuming
   `^^self` receiver; `Val(fields)` means the call touches only those stored
   fields of its receiver.
2. **Checked body trees (`FnBody.tree: BodyTree`)**:
   Every checked function, method, and `#Test` in `JetDriverDocModule.functions`
   already carries its flat checked `BodyTree` (`nodes: [BodyNode]`,
   `stmts: [StmtKind]`, `expr_types: [TExprType]`) alongside `params: [BodyParameter]`.
   Every local reference (`NodeKind.Local(local: TLocal)`) carries its checked
   `name`, `symbol_id`, declaration `span`, `ty: Type`, `mutable`, and `generated`
   flag. Every field projection (`NodeKind.Field(receiver, field: TFieldRef)`)
   carries the field `name` and checked `ty: Type`.
3. **Call-site and ownership lowering (`Calls/Checking.jet`, `Calls/Methods.jet`, `Ownership/CopySites.jet`)**:
   - Write assignments and compound assignments lower to `StmtKind.Assign(place, op, value, ...)`.
   - Call arguments lower via `sema_call_lower_bound_arguments` (`Checking.jet:1608-1639`)
     into `BodyCallArg`:
     - `mut_borrow == true` for every `AccessConvention.Write` (`^`) parameter.
     - `borrow == true` for every non-scalar `AccessConvention.Read` parameter.
     - Scalar `Read` arguments have `borrow == false && mut_borrow == false`, so
       `!borrow && !mut_borrow` alone must **not** be treated as a take.
   - Actual consuming moves of places are explicitly wrapped by
     `sema_call_marked_move_value` (`Checking.jet:1589-1597`), `Methods.jet:1168`,
     and `Places.jet:222` in `NodeKind.Move(value)`. Explicit resource disposal
     lowers to `NodeKind.Drop(value)` and `NodeKind.Close(value)`.
   - Copies at owning destinations lower to `NodeKind.Clone(value, fact)` or
     `NodeKind.ExplicitCopy(value)`, which **read** their inner place rather than
     taking it.
   - Explicit place borrows lower to `NodeKind.Borrow(place, access)` and checked
     indexing carries `NodeKind.Index(base, index, kind, access, ...)`.
   - When `Methods.jet:1152-1169` pins a receiver or argument into a synthetic
     `TLocal` (`local.generated == true`), the generated `StmtKind.Let` initializes
     it with `NodeKind.Borrow(real_place, access)` (or moves it via `NodeKind.Move`),
     so walking the statement's initializer records the real user place while
     skipping `local.generated` temps keeps synthetic names out of the footprint.
4. **Built-in and collection methods (`BuiltinMethods.jet`, `Collections.jet`, `Checked.jet`)**:
   Built-in value and collection calls lower to `NodeKind.BuiltinMethod(receiver, operation: TBuiltinOp, args)`
   and `NodeKind.HandleMethod(receiver, operation: BodyHandleOp, args)`. An
   exhaustive match on `TBuiltinOp` and `BodyHandleOp` classifies mutating
   receivers (`Push`, `Pop`, `InsertMap`, `RemoveMap`, `Sort`, `Clear`,
   `StringEdit`, `AtomicMethod` store/add/compare_exchange, etc.) as `write`,
   consuming receiver operations as `take`, and pure queries as `read`.
5. **Semantic index (`Compiler/JetSemIndex/Source/SemIndex/Types.jet`)**:
   `SemIndex` already indexes cross-module definitions (`SemDef`), callables
   (`SemCallable` with `params` and `receiver: AccessConvention?`), and members
   (`SemMember`), which resolves receiver and parameter conventions even for
   methods whose `FnBody` is not in the user module list (such as Core methods).

## 2. Single-source architecture (no duplicate walker)

Both `jet inspect` (`Compiler/JetCLI`) and `JetLSP` (`Compiler/JetLSP`) already
depend on `jet_semindex` and `jet_driver`, and both compile checked user code in
`JetDriverCompileMode.Doc`, producing the exact same `modules: [JetDriverDocModule]`
and `index: SemIndex?`.

Therefore, the entire block-footprint inference engine lives in **one** module:
- `Compiler/JetSemIndex/Source/SemIndex/Footprint.jet`

It exposes one canonical entry point:
- `semindex_block_footprint(modules: [JetDriverDocModule], index: SemIndex?, target_path: String, range: Span) -> SemBlockFootprint?`

and canonical formatting helpers:
- `semindex_block_footprint_summary(footprint: SemBlockFootprint) -> String`
- `semindex_block_footprint_params(footprint: SemBlockFootprint) -> [SemFootprintParam]`

Consumers read only this module:
- **`jet inspect footprint FILE:LINE[-LINE] [--json]`** (`Compiler/JetCLI/Source/CLI/InspectFootprint.jet`):
  compiles the file in `JetDriverCompileMode.Doc`, maps `LINE[-LINE]` to a byte
  `Span`, calls `semindex_block_footprint`, and renders either human lines or
  stable JSON.
- **`JetLSP`** (`Compiler/JetLSP/Source/LSP/Footprint.jet`, wired into
  `Navigation.jet`, `Commands.jet`, and `CodeActions.jet`):
  calls the exact same `semindex_block_footprint` on `view.checked.modules` and
  `view.checked.index` for:
  1. Range/block footprint display in **hover** and **code lens**;
  2. **`Extract function`** (`refactor.extract`) code action, whose generated
     local helper signature uses `semindex_block_footprint_params` so every
     parameter carries its exact inferred convention (`name: T`, `name: ^T`,
     `name: ^^T`) and every call-site argument carries the matching write/take
     mark (`^name`, `^^name`) when needed.

## 3. Semantics and edge-case invariants

1. **Access lattice (`Take > Write > Read`)**:
   Each accessed place `(root, field_path)` merges repeated uses under
   `Read < Write < Take`. A compound assignment (`n += 1`) records `Write`; a
   read followed by a write on the same place merges to `Write`; a write followed
   by a move/drop merges to `Take`.
2. **Field precision and receiver footprint expansion**:
   - Direct field accesses record the dotted path (`state.count`, `cfg.host`).
   - Calling a method whose solved `FnBody.receiver_footprint` is `Val(fields)`
     on a place `p` (for instance `counter.bump()` where `bump` has `^self.[hits]`
     or inferred `self.[hits]`) expands the call to `p.<field>` for each field
     in `fields` at the method's receiver access convention (`Read` or `Write`),
     rather than pessimistically claiming whole `p`.
   - When the whole root `p` is itself written or taken (e.g., `p = other`,
     `consume(^^p)`, or calling a whole-receiver method), whole `p` is reported
     at that strongest access; field accesses strictly weaker than or equal to
     the whole-root access are subsumed, while a stronger field access (e.g.,
     reading whole `p` elsewhere while writing `p.count`) remains visible.
3. **Parameter projection for `Extract function`**:
   Because Jet functions take whole-value parameters (`x: T`, `x: ^T`, `x: ^^T`),
   `semindex_block_footprint_params` aggregates each distinct outer root binding
   (in deterministic source/name order) to the strongest access among the root
   and all of its accessed fields:
   - `Read` -> `x: T` (call site: `x`)
   - `Write` -> `x: ^T` (call site: `^x`)
   - `Take` -> `x: ^^T` (call site: `^^x`)
   Field-level detail remains intact in `SemBlockFootprint.entries` for `jet inspect footprint`
   and LSP hover/code lens.
4. **Block-local scoping and nested selections**:
   - Bindings declared inside the selected span (`StmtKind.Let`, `RefutableBind`,
     `TupleDestructure`, `StructDestructure`, `ListDestructure`, `ForRange`,
     `Range`, `ForIn`, `TaskGroup`, `Layout`, `Transact`, pattern bindings, and
     lambda parameters) are tracked as internal locals and excluded from the
     external footprint.
   - When a compound statement (`If`, `Loop`, `While`, `ForIn`, `ForRange`,
     `Match`, `EnumMatch`, `Region`, `UnsafeBlock`, etc.) strictly encloses the
     requested range (`stmt_span.start < range.start && stmt_span.end > range.end`),
     selection descends into its child blocks so selecting lines inside a loop
     or branch treats the enclosing loop variable or branch binding as an
     external input.
   - When a single line (`FILE:LINE`) hits the header of a compound block (such
     as `if ... {` or `loop ... {`), the enclosing block statement is selected so
     `jet inspect footprint file.jet:12` works naturally on block headers as well
     as single statements and multi-line ranges `FILE:12-18`.
5. **Closures and captures**:
   `NodeKind.Lambda(lambda)` walks the lambda body with its own `source_params`
   treated as internal, attributing outer accesses to the enclosing block (and
   upgrading non-Copy captured places to `Take` when `lambda.is_move` is true).
6. **Fail-closed checking**:
   If the target file fails to check, `jet inspect footprint` reports the
   compiler diagnostics and exits non-zero rather than fabricating a syntactic
   guess. If `LINE` or `LINE-LINE` is out of range or malformed, `jet inspect footprint`
   reports a clear usage diagnostic (`E2104`).
7. **Zero new syntax**:
   No keywords, sigils, parser rules, or editor TextMate/Tree-sitter grammars
   change. Only the CLI command registry (`Compiler/JetFoundation/Source/Registry/CLICommands.jet`)
   registers the new `jet inspect footprint` action.

## 4. Verification plan

- **Unit tests (`Compiler/JetSemIndex` / `Compiler/JetCLI` / `Compiler/JetLSP`)**:
  - Reads, writes, takes, compound assignments, indexed writes, method receiver
    writes, solved `^self.[a, b]` receiver footprint expansion, built-in
    collection mutations (`push`, `clear`, `insert`), block-local exclusion,
    nested-range selection inside `loop`/`if`, and `Clone`/`ExplicitCopy` staying
    `read`.
  - LSP hover/code-lens footprint output and `Extract function` edits producing
    compiling Jet code with exact `x: T`, `x: ^T`, `x: ^^T` signatures.
- **End-to-end `.case` tests**:
  - `Tests/Cases/CompilerAPI/inspect_footprint.case` (`jet inspect footprint FILE:LINE[-LINE]`
    both human and `--json`, plus malformed range and non-checking refusal).
  - `Tests/Cases/LSP/lsp_block_footprint_and_extract.case` (stdio LSP hover,
    code lens, and `Extract function` code action verified to compile cleanly
    after applying the edit).
- **Golden example & docs**:
  - Executable golden example under `Examples/` verified at O0, O1, and O2.
  - Documentation updates in `Docs/Spec/spec.md` and `Docs/Spec/Reference/cli.md`.
