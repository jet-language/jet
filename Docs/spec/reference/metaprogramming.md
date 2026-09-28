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

- `prep { ... }` is a brace-delimited expression evaluated before execution.
  Its final expression supplies an ordinary value; without one it returns
  `Unit`.
- `@name :: expression` binds a value evaluated during compilation.
- `@ { ... }` evaluates a compile-time block without emitting a runtime block.
- Metadata roots such as `@TYPE(...)` query checked facts; they do not replace
  preparation or compile-time bindings.
- `@if` selects a compile-time branch.
- `@loop` repeats a typed body over a compile-time collection.
- `derive` and marker bodies use ordinary items and statements rather than a
  string macro language.

The feature examples exercise these forms directly. For example, the block in
[`comptime_block.jet`](../../../Examples/features/comptime/comptime_block.jet)
checks `@ratio` while compiling and then uses the resulting value in an
ordinary runtime expression:

```jet
@LIMIT :: 1000
@BASE :: 10

fn run() {
    @ {
        @ratio :: @LIMIT /% @BASE
        if @ratio < 1 -> panic("limit must be >= base")
    }
    print("ratio: {@ratio}")
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

`@loop` is the one typed compile-time repetition mechanism used by derive and
marker bodies. [`derive_loop.jet`](../../../Examples/features/reflection/derive_loop.jet)
uses it to create methods from `T.@fields`, to emit an `impl` for each type,
and to create measured test declarations from `@CASES`.

The compile-time mark is `@`. The former `comptime` keyword is not a second
spelling for it; the syntax registry retains that word only for a teaching
diagnostic. Keep the mark attached to the name or block, as in
`@LIMIT` and `@ { ... }`.

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
    @ {
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
            info.name == "CustomerId" -> {
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
   `Severity.@range`, `send_report.@effects`, `T.@layout`, and
   `@build.package.name`. These reads produce typed fact records that can be
   used in a compile-time binding or a derive body.
2. `T.reflect()` returns aggregate type information for a derive body, while
   `reflect.of(value)` returns the runtime reflection floor for a value.

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

## The metadata contract

The owner-controlled lexical ledger names these ratified metadata roots:

```text
@PHASE  @TYPE  @FUNCTION  @METHOD  @CLOSURE
@PROGRAM  @PACKAGE  @SOURCE  @VALUE  @TYPES
```

`@PHASE()` reports the evaluation-site `Phase` value. The shared AST contract
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
