# Jet metaprogramming

This page is for Jet programmers who need compile-time values, typed facts, or
build-time generation. It separates source forms exercised by the feature
examples from metadata and diagnostic contracts defined by the compiler.
The executable surface is registered in [`Syntax.rs`](../../../crates/jet-foundation/src/Syntax.rs);
shared contract records live in
[`AST/metaprogramming.rs`](../../../crates/jet-foundation/src/AST/metaprogramming.rs).
The runnable examples are the best guide to accepted source forms:
[`Examples/features/comptime`](../../../Examples/features/comptime) and
[`Examples/features/reflection`](../../../Examples/features/reflection).

## The authority model

Jet uses one language at every stage. A compile-time expression is still typed
Jet code, and a generated item must pass the ordinary checker before it can
become part of a program. The boundary is visible in source:

- `prep { ... }` is the explicit shared-preparation block (D-PREP-SURFACE2=A).
  Its body runs while building and emits no runtime code; compile-time
  bindings made inside it keep their `@` names afterward. The ratified value
  form, where the block's final expression supplies an ordinary value, is not
  implemented yet: `name :: prep { ... }` is rejected with E0391. The retired
  `@ { ... }` spelling teaches `prep { ... }` (E0388).
- `@name :: expression` binds a value evaluated during compilation.
- Compiler facts carry `$`: members of their subject such as `T.$fields`, or
  the roots `$build`, `$package`, `$phase` and `$program`. They do not replace
  preparation or compile-time bindings.
- `@if` selects a compile-time branch.
- `@loop` repeats a typed body over a compile-time collection.
- `derive` and marker bodies use ordinary items and statements rather than a
  string macro language.

The feature examples exercise these forms directly. For example, the block in
[`prep_block.jet`](../../../Examples/features/comptime/prep_block.jet)
checks `@cells` while compiling and then uses the resulting value in an
ordinary runtime expression:

```jet
@WIDTH :: 12
@HEIGHT :: 5

fn area(width: Int, height: Int) -> Int { width * height }

fn run() {
    prep {
        @cells :: area(@WIDTH, @HEIGHT)
        if @cells > 100 -> panic("the grid is too large")
    }
    print("cells: {@cells}")
}
```

A compile-time name remains an ordinary value at its use site. It does not
introduce a second type-position syntax or a runtime reflection object.

## Compile-time evaluation

Pure expressions are evaluated without a host authority grant. The examples
cover literal and arithmetic constants, Core math, parsing, and collection
construction. [`comptime_table.jet`](../../../Examples/features/comptime/comptime_table.jet)
computes a list once and prints the baked value from `fn run()`.

`@if` performs compile-time selection. The selected arm is type-checked and
emitted; the other arm is name-resolved so misspelled names are still found.
Nested `@if` forms use the same rule. See
[`comptime_if.jet`](../../../Examples/features/comptime/comptime_if.jet).

`prep loop` is the one typed compile-time repetition mechanism used by derive
and marker bodies. [`derive_loop.jet`](../../../Examples/features/reflection/derive_loop.jet)
uses it to create methods from `T.$fields`, to emit an `impl` for each type,
and to create measured test declarations from `CASES`.

Inside a template the fact sigil also splices a template binding
(D-NAME-SPLICE1=B). `fn $method(self)` and `impl $type_name` name a
declaration from the binding's text, `self.$field` reads the member a loop
value names, `.$left` matches the variant it names, and `{$count}` or a bare
`$count` reads the binding's build-time value. A `$` word that names no
template binding, or that is read on a bound value such as `field.$name`,
stays a compiler fact. The retired `@` splice teaches E0388 with the `$`
respelling; prefix `@` in code marks only a live link.

## Effect tiers and reproducibility

Compile-time effects are classified by what they can observe:

| Tier | Allowed work | Reproducibility rule |
|---|---|---|
| 0 | Pure Jet values and effect-free Core calls | No external input is read. |
| 1 | `embed_file`, `embed_bytes`, `find(glob)`, and pinned `fetch(url, sha256:)` | Inputs and hashes are recorded in `.jet/lock`; equal inputs produce equal values. |
| 2 | Ambient filesystem, environment, process, clock, random, or unpinned network access | The source uses `#Impure("reason")`, and the build invocation grants `impure`. |

The examples show the Tier 1 boundary: [`embed.jet`](../../../Examples/features/comptime/embed.jet)
and [`embed_bytes.jet`](../../../Examples/features/comptime/embed_bytes.jet) bake
text or raw `[U8]` data into the program, while
[`find.jet`](../../../Examples/features/comptime/find.jet) returns sorted
relative paths with their input identity. An empty `find` result is still a
typed empty `[String]`; see [`find_empty.jet`](../../../Examples/features/comptime/find_empty.jet).

`core.net.fetch(url, sha256:)` is the only Tier 1 network operation. Other
`core.net` methods are unavailable at compile time (E3412), and a content-hash
mismatch is E3413.

A Tier 2 operation does not become acceptable merely because it appears in a
compile-time expression. A compile-time ambient read needs both the source
gate and the command-line grant:

```jet
use core.files as fs

fn run() {
    prep {
        #Impure("read an explicitly approved ambient input") {
            @CONFIG :: fs.read("config.txt")
        }
    }
    print(@CONFIG)
}
```

A runtime-only statement inside `#Impure` is an ordinary runtime block; it does
not exercise a Tier 2 compile-time effect and does not by itself require
`--gate impure=allow`. The compile-time `fs.read` above requires both the
source gate and `--gate impure=allow` (E3410, E3411). The `#Impure` marker is a
gate, not a blanket host permission. Keep declarations, grants, and resulting
inputs in the package and lock records rather than relying on ambient process
state. The executable policy is defined at the compiler boundary; the
source-facing effect names are registered through
[`Syntax.rs`](../../../crates/jet-foundation/src/Syntax.rs).

## Build entries and typed generation

A package may expose an ordinary `fn build` that takes `BuildContext` and
returns a `BuildPlan`. It inspects typed build facts, registers typed graph
nodes, and returns the plan; it does not become a second configuration
language. `BuildContext` exposes typed graph operations: `b.add_executable`,
`b.add_library`, `b.add_test`, `b.add_asset_bundle`, `b.add_doc`,
`b.add_install`, `b.add_package`, and `b.add_publish` register targets;
`b.action` registers a typed action; `b.generate` registers additive generated
Jet source; and `b.find_program`, `b.pkg_config`, `b.header_check`, and
`b.compile_check` register typed probes. `b.plan()` hands the graph to the
driver.

The checked example in [`distinct_reflect.jet`](../../../Examples/features/comptime/distinct_reflect.jet)
uses the current return-arrow form:

```jet
fn build(b: BuildContext) -> BuildPlan {
    loop info in b.program.types() {
        if {
            info.name == "CustomerID" -> {
                if {
                    info.has_marker("Comparable") -> { next }
                    else -> {
                        b.error(info.span, "REFLECT", "missing marker",
                            "#Comparable was lost",
                            "keep the marker in reflection")
                    }
                }
            }
            else -> { next }
        }
    }
    app :: b.add_executable("distinct_reflect", ["distinct_reflect.jet"], [])
    return b.plan(app)
}
```

Each action declares its capabilities. Execution proceeds only when package
and workspace policy permits them and the invocation supplies matching grants;
dependency build scripts do not inherit root authority. The execution runtime
rejects a selected action or probe when its grant is absent.

The graph and environment contracts that a build entry consumes are defined
in the [Jetpack reference](jetpack-epoch5.md). Build-side inspection is
read-only unless a ratified generation operation is used; a build entry cannot
silently rewrite an existing declaration.

The source registry reserves typed item-template generation. `derive T.Trait {
... }` and `b.generate(name) { ... }` carry ordinary Jet item and statement
grammar, and neither form accepts a source string or re-lexes generated text.
This is the `D-META-CODE1` / `D-META-BODY1` rule in [`Syntax.rs`](../../../crates/jet-foundation/src/Syntax.rs).

## Typed reflection

Jet has two related reflection paths:

1. A compile-time fact read uses the `@` member form. Examples include
   `Severity.$range`, `send_report.$effects`, `T.$layout`, and
   `$package.name`. These reads produce typed fact records that can be
   used in a compile-time binding or a derive body.
2. `T.reflect()` returns aggregate type information for a derive body, while
   `reflect.of(value)` returns the runtime reflection floor for a value.

Compile-time marker reflection and declaration templates read the same catalog
retained by declaration registration. Its rules come from the embedded
`Prelude/Markers.jet` declarations, and its argument menus come from the
canonical Core declaration projection. The catalog is available before template
expansion; user-defined marker declarations are separate package rules, not
replacements for the embedded vocabulary. A malformed embedded declaration is
an internal compiler error, never an empty reflection catalog.
Marker names, attachment sites and repeatability are checked against that
retained vocabulary. Argument types are checked in the scope where they are
written: callable parameters are available for callable markers, and preceding
locals are available for statement and binding markers. A module-only check
must not replace either live scope.

[`reflect.jet`](../../../Examples/features/comptime/reflect.jet) combines typed
fact reads with `T.reflect()`. [`reflect-value.jet`](../../../Examples/features/reflection/reflect-value.jet)
shows the runtime floor:

```jet
use core.reflect as reflect

struct Point {
    x: Int(0..10)
    y: Int
    level: Int(0..10)
}

fn run() {
    point :: Point{x: 3, y: 4, level: 2}
    value :: reflect.of(point)
    print(value.type_name())
    print(value.path())
    print(value.display())
    loop field in value.fields() ->
        print("{field.name()}:{field.value().type_name()}")
}
```

`reflect.of(value)` is legal where value interpolation is legal. Its
`type_name`, `path`, and `display` operations are always available; `fields()`
is populated for a struct and is empty for a non-struct value. A generic
reflection value retains the substituted field type, as the same example
shows for `Box<Int>`.

The fact plane is deliberately typed and closed. The feature examples read
range, dimension, state, effect, sendability, movedness, attribution, origin,
view provenance, and maturity facts; consumers must use the registered fact
record rather than infer a fact from a string. `fact_reads.jet` and
`reflect-value.jet` are executable examples of those member names.

Unit dimensions are produced before derives and nominal registration. The
canonical Foundation embedded-source accessor publishes ordinary
`Prelude/Units.jet`; Sema parses it with the compiler-generated lexer mode,
selects source-mentioned families/members and their dimension dependencies,
and anchors injected member diagnostics to the receiving source's UTF-8 bytes.
`#NoPrelude` skips injection and a local family shadows its whole standard
family. Base axes use the loader-retained owning package fingerprint and
root-relative module path; standard axes retain `core.units` independently
of the receiving package. Derived claims resolve local and public imported
dimensions through exact import edges, recording alias uses in the name ledger.
The sole registration unit-fact pass carries normalized dimensions, rational
scale/offset, source provenance and Point/Delta/Linear kind. Its indexed,
module-scoped literal facts feed both ordinary literals and job schedules;
Duration and Instant are builtin types, not a fallback Time suffix catalog.
Generated embedded payload publication and composed execution verification
remain distinct from this source contract.

## Compiler-value projection

Foundation owns the ordered AST and checked TIR compiler-value writer. Compound
map keys convert to values with the lowercase `tuple` identity and retain their
authored field order. Struct projection omits only compiler-generated memo
storage (`__jet_` followed by `__memo_`); user lookalikes remain visible.
No field or map entry sorting or deduplication occurs.

Strings, characters, field names, enum variants and argument labels use the
same Foundation JSON escaper. An enum with all labeled arguments has an object
payload; positional arguments have an array payload. In a mixed array, each
labeled argument is a one-key object. Present values project as their payload,
clean failures and Unit/closures as null, and told failures as an `err` object.

Float text comes from the native-width scalar formatter: Float32 rounding and
shortest spelling are not reconstructed from a Float64 string. Compiler-value
notation remains JSON-like rather than a general JSON product format: map keys
may themselves be numbers or compound values, and nonfinite floats retain their
diagnostic spelling. The finite, string-key subset is strict JSON. This notation
is distinct from ordered CLI status envelopes and sorted canonical JSON.

## The metadata contract

The owner-controlled lexical ledger names these ratified fact roots
(D-META-ROOT3=A, D-BUILD-FACT3=A). Facts about a type, function, method,
closure or value are `$` members of that subject, such as `T.$fields`, so no
query root exists for them. `@SOURCE` and `@TYPES` keep their registry
spelling until a ballot names their `$` root.

```text
$build  $package  $phase  $program
```

`$build.settings.<name>` reads a declared typed setting, not an evaluator
global or an ambient environment value. The canonical package model supplies
its declaration and default. Every profile is validated, including inactive
profiles, before the selected profile, computed build contributions and explicit
CLI overrides enter the shared fact resolver. The retained snapshot keeps the
declared type and full ordered writer chain together; Sema folds each read and
checks the caller's expected type. Fixed package, OS and lock-stamp leaves use
that same resolver and provenance path. An undeclared setting is rejected rather
than inferred from its spelling.

`$phase` reports the evaluation-site `Phase` value. The shared AST contract
contains exactly `Preparation`, `Build`, and `Runtime`. Do not confuse that
language phase with `CompilerStage`, which contains `Parsed`, `Resolved`,
`Typed`, `Prepared`, and `Emitted` and describes when a checked fact becomes
available.

Metadata queries use typed records and typed failure. The closed failure
variants are:

- `Unavailable { required_stage, reason }`;
- `Unsupported { capability }`;
- `Stale { requested, current }`;
- `Invalid { diagnostics }`;
- `Denied { scope, reason }`; and
- `Truncated { limit, observed }`.

`MetadataError` carries the failure, the query spelling, and an optional
`SourceSpan`. A missing fact is therefore distinguishable from a checked empty
collection. Opaque `SnapshotId`, `TypeId`, `DeclarationHandle`, `ScopeId`,
`NodeId`, and `ValueHandle` values are compiler-issued identities; source names
and casts cannot manufacture them. The canonical names and fields live in
[`AST/metaprogramming.rs`](../../../crates/jet-foundation/src/AST/metaprogramming.rs),
not in a prose catalog.

Root inputs and scopes are also part of the contract. A root may accept a type
expression, callable selection, method name, method handle, authorized
snapshot handle, runtime value, or integer. Its scope is one of evaluation
site, lexical snapshot, authorized program, lexical package, lexical source,
runtime value, or retained runtime type catalog. A result has a typed success
value and, when applicable, a `MetadataError` record. These are the
`MetadataRootInputType`, `MetadataRootScope`, and `MetadataRootResult` records
in the AST contract.

The shared Rust file is a schema boundary. Its records define public shapes for
metadata identities, roots, results, and typed failures; parsing, checking,
evaluation, and runtime retention use their own compiler seams. The feature
examples show exercised forms, and the syntax ledger names ratified roots.

## Publication and advanced providers

Standard metadata inspection is read-only. The syntax ledger reserves
`compiler.generate(name, value)` for explicit additive publication of the
existing `Generated` value. Generation cannot replace an existing body,
signature, or type.

More powerful operations have separate names and grants:

- `compiler.advanced.register` registers an explicitly declared provider;
- `compiler.advanced.register_expansion` registers a call-expansion provider;
- `compiler.advanced.register_specialization` registers a specialization
  provider; and
- `compiler.advanced.session` opens a session only inside an authorized,
  registered provider.

The operation schema accepts typed `Generated` values, providers, opaque
identities, and metadata records. An advanced compiler grant is not a
filesystem, process, FFI, network, or secret permission. Those authorities
remain on their own effect and package-policy rails.

## Design boundary

Jet takes compile-time power from systems such as Jai, but rejects invisible
string insertion, arbitrary message-loop mutation, and ambient build authority.
The durable rule is simple: compute typed values, inspect typed facts, generate
checked items additively, and record external inputs. The source examples and
the two executable registries linked above are the evidence; this page does
not turn a schema row into an implementation claim.
