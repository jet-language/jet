# Compiler data

Vocabulary: [Jet vocabulary](vocabulary.md).

This record is the contract for how the self-hosted compiler holds its own
data: the id spaces, the column layout, the phase lifetimes, and the rule
that compiler data holds no reference counts. It is for anyone who adds a
table, a fact or a phase to `Compiler/`. The work is card #6793. The census
is the compiler-data ratchet,
[`Tools/agent/CompilerData.jet`](../../Tools/agent/CompilerData.jet), and
its ledger, [`tests/compiler_data.tsv`](../../tests/compiler_data.tsv).
The pipeline it applies to is in [Architecture](architecture.md).

## Scope

Compiler data is what the compile pipeline builds and passes between phases:
the packages `JetLexer`, `JetParser`, `JetFoundation`, `JetSema`, `JetIR`,
`JetBackend`, `JetDriver`, `JetEval` and `JetWasm`. Tool packages (the CLI,
devserver, LSP, test kit and the others) read compiler data through the
same accessors, but their own records are tool data and are not bound by
this contract. `JetRuntime` and `Compiler/Prelude` are program code.

## Id spaces

Every compiler name, module, symbol, type, function and node is a `U32` id
into one table per id space. Each space has its own type, a struct holding
only `id: U32`, so ids of two spaces never mix. An id is dense, and id 0 is
"none" (for names, the empty spelling), so an optional id is a plain id and
never an Option box. Ids are assigned in a fixed order (source-unit order,
then item order), so one input always gets the same ids and the self-host
chain reaches a fixed point.

| Space | Table | Assigned by | Holds |
|---|---|---|---|
| `Name` | the compilation's `NameTable` | the loader, merging each file's lexer table in source-unit order | every identifier and literal spelling, once |
| `ModuleID` | the `ModuleTable` | the loader, in load order | each module's identity |
| `SymbolID` | the symbol table | registration, in module and item order | each declaration: kind, module, owner, name, span |
| `TypeID` | the type table | sema, hash-consed | each distinct type, once; children are type ids |
| `NodeID` | a file's AST columns | the parser | each syntax node of that file |

A function is its `SymbolID`. A phase that makes new symbols (a generated
codec, a derived method, a closure, an instantiation) takes their ids from
the symbol table in a fixed order. A parallel phase gives each worker its
own range and renumbers the ranges at its deterministic merge.

A key that a body names but no declaration registered is a local id. It
carries the local bit (`0x80000000`) and indexes the body's own key table,
so it never equals a run-wide id. A compile seeded from the Core record
interns the record's symbol ranges before anything else, so an id in the
record names the same declaration in the compile.

Text exists only at the edges: diagnostics, emitted symbol names, and the
key text a record or cache stores. The tables resolve an id to its text; no
compiler record stores a copy of it.

## Columns

A table is a struct of parallel lists indexed by id (struct of arrays). An
element of a column is a scalar: an id, an integer, a flag, a span offset,
or a one-byte kind tag. A row that has a variable number of children stores
the first child's index and the count into a shared child column. A
variable payload goes in a side column that the row's payload field indexes.

No column element is a `String`, a list, a map, an Option or Result of a
non-scalar, or an enum with payloads: each of those is its own heap block
with its own count. Kinds are tags, and the code that reads a row matches on
the tag.

## Lookups

Compiler data holds no `String`-keyed map, set or tally, and no `NameRows`
(a `String`-to-row map). A lookup by id indexes a column; a sparse lookup by
id may use an id-keyed map. The interners are the only text-to-id lookups,
and they hash the spelling's bytes once. A pass that matches text no
compilation table holds (a dotted import path, a line of a diff) interns it
into a `NameTable` of its own, dropped with the pass, and keys its columns
by that table's ids.

## Phase lifetimes

Each phase owns its data as one value of columns, its arena, and drops it in
bulk when the phase ends. Data that a later phase keeps lives in the
compilation tables, which last for the whole compile.

| Data | Built by | Dropped after |
|---|---|---|
| file bytes, line starts | loader | the compile (diagnostics read them) |
| tokens | lexer, per file | that file's parse |
| AST columns | parser, per file | the last phase that reads syntax (sema and comptime) |
| names, modules, symbols, types | loader, registration, sema | the compile |
| checked bodies | sema, per function | that function's lowering |
| IR | the IR builder, per function | that function's code generation |
| machine code, objects | the backend | the link |

A record of a later phase never points into an earlier phase's arena. It
names what it needs by an id into the compilation tables.

## No reference counts

Compiler data carries no reference counts. The only heap blocks are the
column buffers. Each one has a single owner for its whole life, so it is
never shared or copied on write, and nothing retains or releases it between
creation and drop. No compiler record holds a `Shared<T>`. A parallel phase
lends its read-only tables to its workers, has each worker write its own
columns, and merges them in a fixed order.

## Proof

A change to compiler data is a whole cutover: the old representation, an
adapter, or a flag never stays beside the new one. An id space is a
`JetFoundation` type that every package reads, so its cutover changes every
package at once; only data private to one package is cut over package by
package. A cutover too large for one review is written in parts that land
together as one commit, so no part lands alone. Each cutover's proof is the
same:

- the self-host chain reaches a fixed point;
- goldens, ui and cases are identical to before;
- the compiler-data ratchet's counts fall, and its ledger rows are lowered
  to them.

The ratchet counts, in each source file of the pipeline packages, the
text-keyed lookups, the `Shared<` cells and the payload enum declarations.
`jet Tools/agent/CompilerData.jet --check` runs in the lane check and in
every landing; a count over its row fails, and so does a row that a falling
count left stale. `--write` lowers the rows and refuses to raise one.

Card #6793 holds the measurements: peak RSS of the self-compile and of
`jet check` of one compiler file, and CPU per line on the cold build cell.
