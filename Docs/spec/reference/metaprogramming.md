# Jet metaprogramming

This page is for Jet programmers who need compile-time values, typed facts, or
build-time generation. It separates source forms exercised by the feature
examples from metadata and diagnostic contracts defined by the compiler.
The executable surface is registered in the
[syntax registry](../../../Compiler/JetFoundation/Data/Syntax/).
The runnable examples are the best guide to accepted source forms:
[`Examples/Metaprogramming/Comptime`](../../../Examples/Metaprogramming/Comptime) and
[`Examples/Metaprogramming/Reflection`](../../../Examples/Metaprogramming/Reflection).

## The authority model

Jet uses one language at every stage. A compile-time expression is still typed
Jet code, and a generated item must pass the ordinary checker before it can
become part of a program. The boundary is visible in source:

- `comptime { ... }` is the explicit shared-preparation block (D-PREP-SURFACE2=A).
  Its body runs while building and emits no runtime code. In the binding form
  `name: comptime { ... }`, the final expression supplies the prepared value.
  That value retains its checked initializer type: numeric widths, empty-list
  element types, optional elements, and function return types are not inferred
  again from the evaluator's erased value representation. The retired
  `@ { ... }` spelling teaches `comptime { ... }` (E0388).
- `@name: expression` binds a value evaluated during compilation.
- Compiler facts carry `$`: members of their subject such as `T.reflect().fields`, or
  the roots `#build`, `#package`, `#phase` and `#program`. They do not replace
  preparation or compile-time bindings.
- `@if` selects a compile-time branch.
- `@loop` repeats a typed body over a compile-time collection.
- `derive` and marker bodies use ordinary items and statements rather than a
  string macro language.

The feature examples exercise these forms directly. For example, the block in
[`prep_block.jet`](../../../Examples/Metaprogramming/Comptime/PrepBlock.jet)
checks `@cells` while compiling and then uses the resulting value in an
ordinary runtime expression:

```jet
@WIDTH: 12
@HEIGHT: 5

area: fn(width: Int, height: Int) -> Int { width * height }

run: fn() {
    comptime {
        @cells: area(@WIDTH, @HEIGHT)
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
construction. [`comptime_table.jet`](../../../Examples/Metaprogramming/Comptime/ComptimeTable.jet)
computes a list once and prints the baked value from `run: fn()`.

`@if` performs compile-time selection. The selected arm is type-checked and
emitted; the other arm is name-resolved so misspelled names are still found.
Nested `@if` forms use the same rule. See
[`comptime_if.jet`](../../../Examples/Metaprogramming/Comptime/ComptimeIf.jet).

`comptime loop` is the one typed compile-time repetition mechanism used by derive
and marker bodies. [`derive_loop.jet`](../../../Examples/Metaprogramming/Comptime/PrepLoop.jet)
uses it to create methods from `T.reflect().fields`, to emit an `impl` for each type,
and to create measured test declarations from `CASES`.

Inside a template a paste `#{name}` splices a template binding
(D-META-HASH1=A). `#{method}: fn(self)` and `impl #{type_name}` name a
declaration from the binding's text, `self.#{field}` reads the member a loop
value names, `.#{left}` matches the variant it names, and a bare `#{count}`
or a string hole `{count}` reads the binding's build-time value. A compiler
fact reads through the subject, `field.reflect().name`, inside a template as
anywhere else. A paste outside a derive or marker template is E0003, and so
is any `$` word in program source (the site values are `#here` and
`#caller`, D-SITE-CALLER1=A).

## Effect tiers and reproducibility

Compile-time effects are classified by what they can observe:

| Tier | Allowed work | Reproducibility rule |
|---|---|---|
| 0 | Pure Jet values and effect-free Core calls | No external input is read. |
| 1 | `embed_file`, `embed_bytes`, `find(glob)`, and pinned `net.fetch(url, sha256:)` | File inputs and their hashes are recorded in `.jet/lock`, and a fetch's hash is the pin written in the source; equal inputs produce equal values. Each file read is an input of the build identity: editing it rebuilds under `jet build` and reruns under `jet dev`. |
| 2 | Ambient filesystem, environment, process, clock, random, or unpinned network access | The source uses `#Impure("reason")`, and the build invocation grants `impure`. |

The examples show the Tier 1 boundary: [`embed.jet`](../../../Examples/Metaprogramming/Comptime/Embed.jet)
and [`embed_bytes.jet`](../../../Examples/Metaprogramming/Comptime/Embed.jet) bake
text or raw `[U8]` data into the program, while
[`find.jet`](../../../Examples/Metaprogramming/Comptime/Find.jet) returns sorted
relative paths with their input identity. An empty `find` result is still a
typed empty `[String]`; the comptime tier-parity case
[`find_empty.jet`](../../../tests/cases/comptime_cli_parity/fixtures/tier_parity_examples_run_through_aot_jit_and_interpreter/comptime_find_empty/find_empty.jet) proves it.

These Tier 1 inputs are ordinary effect-governed calls: `embed_file`,
`embed_bytes` and `find` read `FS.Read`, and `net.fetch` reaches `Net`. A
function that makes one carries that effect in its row, so a plain helper may
call one, `jet explain effects` lists it, and a row that leaves it out or
denies it (`-[IO]>`, `-[!Net]>`) refuses the call
([`build_io_net_denied.jet`](../../../tests/ui/build_io_net_denied.jet),
[`comptime_input_fs_read_denied.jet`](../../../tests/ui/comptime_input_fs_read_denied.jet)).
The compiler answers these calls only while it compiles, so code the running
program reaches can't make one, directly or through a helper: a call that
`run`, a test, or any function they call reaches is E0956 at check time
([`comptime_input_reached_at_run_time.jet`](../../../tests/ui/comptime_input_reached_at_run_time.jet)).
A helper that only comptime code and `build: fn` call is prep code, and the
program reads its result through a prepared constant
(`GREETING: comptime { greeting() }`).

`core.net.fetch(url, sha256:)` is the only Tier 1 network operation. It runs
only while compiling; code the running program reaches can't call it (E0956).
The URL is a String, written or computed by comptime code, and the `sha256:`
pin is a String literal; any other shape is E3414. The compiler fetches
`file://` (inside the source directory), `http://` and `https://` URLs from
public destinations only, and content that misses the pin is E3413. Because
the pin in the source fixes the value, the fetch is not recorded in
`.jet/lock`. Other `core.net` methods are unavailable at compile time (E3412).

`build: fn` reads its inputs through the same calls. `Build` has no `find`,
`embed` or `fetch` method of its own, and naming one is E0102. Each file and
glob a build reads lands in `.jet/lock` beside the generated inputs, a file as
its path and SHA-256, and `jet build --locked` refuses a build whose reads
drift. A build's `net.fetch` has no lock row, for the same reason as above.
The [`sprite_atlas.jet`](../../../tests/run/Preparation/comptime/sprite_atlas.jet)
golden packs the files that `find` lists through a plain helper that calls
`embed_file` and `embed_bytes`, and the running program reads only the baked
atlas.

A Tier 2 operation does not become acceptable merely because it appears in a
compile-time expression. A compile-time ambient read needs both the source
gate and the command-line grant:

```jet
use fs: core.files

run: fn() {
    comptime {
        #Impure("read an explicitly approved ambient input") {
            @CONFIG: fs.read("config.txt")
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
the [syntax registry](../../../Compiler/JetFoundation/Data/Syntax/).

## Build entries and typed generation

A package may expose an ordinary `build: fn` that takes `Build` and
returns a `BuildPlan`. It inspects typed build facts, registers typed graph
nodes, and returns the plan; it does not become a second configuration
language. `Build` exposes typed graph operations: `b.add_executable`,
`b.add_library`, `b.add_test`, `b.add_asset_bundle`, `b.add_doc`,
`b.add_install`, `b.add_package`, and `b.add_publish` register targets;
`b.action` registers a typed action; `b.generate` registers additive generated
Jet source; and `b.find_program`, `b.pkg_config`, `b.header_check`, and
`b.compile_check` register typed probes. `b.plan()` hands the graph to the
driver.

The checked example in [`distinct_reflect.jet`](../../../Examples/Metaprogramming/Reflection/Reflect.jet)
uses the current return-arrow form:

```jet
build: fn(b: Build) {
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
    app: b.add_executable("distinct_reflect", ["distinct_reflect.jet"], [])
    b.default(app)
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
This is the `D-META-CODE1` / `D-META-BODY1` rule in the [syntax decisions](../syntax-decisions.md).

A `#{name}` paste in a `b.generate` body that names a local of the build entry splices
that local's value into the generated items as a Jet literal, so build
reflection can feed generated code:

```jet
build: fn(b: Build) {
    loop function in b.program.functions() {
        if function.name == "wrapped" {
            operation: function.arithmetic[0].operation
            b.generate("reflection_values") {
                reflected_operation: fn() -> String { #{operation} }
            }
        }
    }
}
```

A spliced local is a `String`, `Int` or `Bool`, or a list of them; any other
type is E3502. The value goes where an expression goes; a generated
declaration's name comes only from a compile-time binding, such as a
`comptime` loop name. A `#{name}` that names no build local stays a
compile-time name of the items.

## Typed reflection

Jet has two related reflection paths:

1. A compile-time fact read is a `$` member of its subject. Examples include
   `Severity.reflect().range`, `send_report.reflect().effects`, `T.reflect().layout`, and
   `#package.name`. These reads produce typed fact records that can be
   used in a compile-time binding or a derive body.
2. `T.reflect()` returns aggregate type information for a derive body, while
   `reflect.of(value)` returns the runtime reflection floor for a value.

[`reflect.jet`](../../../Examples/Metaprogramming/Reflection/Reflect.jet) combines typed
fact reads with `T.reflect()`. [`reflect-value.jet`](../../../Examples/Metaprogramming/Reflection/ReflectValue.jet)
shows the runtime floor:

```jet
use reflect: core.reflect

Point: struct {
    x: Int(0..10)
    y: Int
    level: Int(0..10)
}

run: fn() {
    point: Point{x: 3, y: 4, level: 2}
    value: reflect.of(point)
    print(value.type_name())
    print(value.path())
    print(value.display())
    loop field in value.fields() ->
        print("{field.name()}:{field.value().type_name()}")
}
```

`reflect.of(value)` is legal where value interpolation is legal. Its
`type_name`, `path`, and `display` operations are always available; `fields()`
is populated for a struct and is empty for a non-struct value. A struct
field's `value()` lists that struct's own fields, so `.fields()[i].value()`
walks a nested value level by level. A generic reflection value retains the
substituted field type, as the same example shows for `Box<Int>`.

`info.marker(T)` on a `TypeInfo`, `FieldInfo` or `MethodInfo` reads the marker
`T` written on the reflected item as a `T?`: a `T` whose fields are the
marker's arguments, or `None` when the item does not carry it. `T` names a
marker struct (one with an `impl Marker` block); any other name is E1134. The
read gives the same answer in a marker body, a marker that adds methods, a
comptime constant and `build: fn`, as
[`ReflectionRungs`](../../../Examples/Metaprogramming/Reflection/ReflectionRungs/run.jet)
shows: each rung reads the type's name, kind, identity, module, field and
marker counts and `marker(Label)?.text`, and prints the same line.

The fact plane is deliberately typed and closed. The feature examples read
range, dimension, state, effect, sendability, movedness, attribution, origin,
view provenance, and maturity facts; consumers must use the registered fact
record rather than infer a fact from a string. `fact_reads.jet` and
`reflect-value.jet` are executable examples of those member names.

## The metadata contract

The owner-controlled lexical ledger names these ratified fact roots
(D-META-ROOT3=A, D-BUILD-FACT3=A). Facts about a type, function, method,
closure or value are `$` members of that subject, such as `T.reflect().fields`, so no
query root exists for them. `@SOURCE` and `@TYPES` keep their registry
spelling until a ballot names their `$` root.

```text
#build  #package  #phase  #program
```

`#phase` reports the evaluation-site `Phase` value (`core.prelude`), one of
exactly `Preparation`, `Build`, and `Runtime`: `.Preparation` in a `comptime`
block or binding, `.Build` in `build: fn`, and `.Runtime` in the running
program. The evaluation site decides it, so `print(#phase)` in `run` prints
`Runtime` on every tier. [`Phase.jet`](../../../Examples/Metaprogramming/Comptime/Phase.jet)
shows both sites. Do not confuse that language phase with `CompilerStage`,
which contains `Parsed`, `Resolved`, `Typed`, `Prepared`, and `Emitted` and
describes when a checked fact becomes available.

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
and casts cannot manufacture them. The compiler defines the canonical names
and fields; this page is not a catalog of them.

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

More powerful operations are effects, not a separate opt-in. The `Meta`
effects in `core.compiler` (`Generate`, `Include`, `Rewrite`, `Types` and
`Raw`) are allowed unless a row denies them, and reading the program needs
none. Operations above checked generation also need the `Compiler` value,
which only the root package's own build source makes (`b.compiler(app)`). A
`b.compiler` call written in any other package is E0776, even when the root
passed that package its `Build`, so a dependency can't make one. A function
that receives the value acts for the root, and the value expires with the
build that made it.
A Meta effect is not a filesystem, process, FFI, network, or secret
permission. Those authorities remain on their own effect and package-policy
rails.

## Design boundary

Jet takes compile-time power from systems such as Jai, but rejects invisible
string insertion and ambient build authority. Standard code, which every
program gets, computes typed values, inspects typed facts, generates checked
items additively and records external inputs; its written code means what the
file says. The approved root package's `Compiler` value is the advanced mode:
it may build types from facts, parse code into checked fragments, specialize
and rewrite written function bodies, each change rechecked with the callers it
affects and recorded in a receipt that does not promise equivalence. Some
limits hold in both modes: no token, tree or attribute macros, no custom
syntax, no message loop, no user macros, no mutable compiler nodes, no forged
facts, and no host permission carried by a Meta effect. `b.compiler` itself is
built; the operations on the value are ratified contract and not yet built.
The source examples and the two executable registries linked above are the
evidence; this page does not turn a schema row into an implementation claim.
