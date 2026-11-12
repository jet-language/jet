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

- `#run { ... }` is the explicit shared-preparation block (D-PREP-SURFACE2=A).
  Its body runs while building and emits no runtime code. In the binding form
  `name: #run { ... }`, the final expression supplies the prepared value.
  That value retains its checked initializer type: numeric widths, empty-list
  element types, optional elements, and function return types are not inferred
  again from the evaluator's erased value representation. The retired
  `@ { ... }` spelling teaches `#run { ... }` (E0388).
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
    #run {
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

`#loop` is the one typed compile-time repetition mechanism used by derive
and marker bodies. [`derive_loop.jet`](../../../Examples/Metaprogramming/Comptime/PrepLoop.jet)
uses it to create methods from `T.reflect().fields`, to emit an `impl` for each type,
and to create measured test declarations from `CASES`.

A `#Pre` condition written `#run { condition }` is decided for each call
while building, by the same evaluator, after the call's arguments are
substituted for the parameters. A call it holds for carries no run-time
check; a false answer, or a condition that reads an argument known only at
run time, is E0180 at the call. A `#Post` condition written the same way is
decided the same way, with `result` holding the call's own value, which
the evaluator computes while building from the same arguments; the body
checks it for no call. Only a call by name is computed again, so a method
or function-value call of such a function is E0180, and so is one whose
result can fail. `#run` in any other marker argument is E0391 for now.
[`RequiredPreDecidedWhileBuilding.jet`](../../../tests/run/Contracts/RequiredPreDecidedWhileBuilding.jet)
prints 10 for `sum(4, [1, 2, 3, 4])` under
`#Pre(#run { lanes == 4 || lanes == 8 }, "need 4 or 8")`, and
[`contract_required_pre_false.jet`](../../../tests/ui/contract_required_pre_false.jet)
is `sum(3, …)` refused while building.
[`RequiredPostDecidedWhileBuilding.jet`](../../../tests/run/Contracts/RequiredPostDecidedWhileBuilding.jet)
prints 12 for `padded(4, 10)` under
`#Post(#run { result % lanes == 0 }, "a whole number of lanes")`, and
[`contract_required_post_false.jet`](../../../tests/ui/contract_required_post_false.jet)
is `width(1)` refused while building.

Inside a template a paste `#{name}` splices a template binding
(D-META-HASH1=A). `#{method}: fn(self)` and `impl #{type_name}` name a
declaration from the binding's text, `self.#{field}` reads the member a loop
value names, `.#{left}` matches the variant it names, and a bare `#{count}`
or a string hole `{count}` reads the binding's build-time value. A compiler
fact reads through the subject, `field.reflect().name`, inside a template as
anywhere else. A paste outside a derive or marker template is E0003, and so
is any `$` word in program source (the site values are `#here` and
`#caller`, D-SITE-CALLER1=A).

A paste stands for a whole name and is never glued to letters:
`get_#{field}` is E0003, so `pkg#1.4` and `[Int#4]` keep their meaning
inside a template. Build a compound name as a string first,
`method: "get_{field.name}"`, then paste `#{method}`. A local declared in
added code is private to that code, so it needs no reserved prefix: a
user's function of the same name keeps its meaning. A name the template
wrote resolves where the template was written, so a template in another
module calls that module's helpers (private ones too), and a pasted name
resolves at the marker's site. A template's own local (a binding or a
`#loop` name in `expand`) may not hide a `use` name of its file or a
Prelude name such as `print`: that is E0118, like a hidden inner local
([`template_local_hides_use_or_prelude_name.jet`](../../../tests/ui/template_local_hides_use_or_prelude_name.jet),
[`TemplateHygiene.jet`](../../../tests/run/Comptime/TemplateHygiene.jet),
[`TemplateDefinitionSite`](../../../tests/run/Comptime/TemplateDefinitionSite/run.jet)).

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
(`GREETING: #run greeting()`).

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
    #run {
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
source gate and `--gate impure=allow` (E3410, E3411). With both, the call runs
natively in the compile-time child, as the same call runs in the program: the
child admits the file system calls of the `#Impure` block and of the functions
it calls, and every other host effect stays refused. The block's authority is
checked first, so an effect the package or the row denies is E0749 at build
and at run alike. The `#Impure` marker is a
gate, not a blanket host permission. Keep declarations, grants, and resulting
inputs in the package and lock records rather than relying on ambient process
state. The executable policy is defined at the compiler boundary; the
source-facing effect names are registered through
the [syntax registry](../../../Compiler/JetFoundation/Data/Syntax/).

## Build entries and typed generation

A package may expose an ordinary `build: fn` that takes `Build`. It inspects
typed build facts, registers typed graph nodes, and hands the driver one plan;
it does not become a second configuration language. `Build` exposes typed
graph operations: `b.add_executable`,
`b.add_library`, `b.add_test`, `b.add_asset_bundle`, `b.add_doc`,
`b.add_install`, `b.add_package`, and `b.add_publish` register targets;
`b.run(name, tool, caps)` registers a typed step; `b.probe(name, kind, value)`
registers a typed `find_program`, `find_folder`, `pkg_config` or `header`
probe; and `b.generate(name) { ... }` registers an additive generated module.
`b.plan()` or `b.default(target)` hands the graph to the driver. The
[programmable builds](../spec.md#programmable-builds) section of the spec owns
each operation's rules, including the generated-module diagnostics.

The checked example [`Reflect.jet`](../../../Examples/Metaprogramming/Reflection/Reflect.jet)
reads the program's reflection view from a build entry:

```jet
build: fn(b: Build) {
    loop info in b.program.types() {
        if {
            info.name == "CustomerID" -> {
                if {
                    info.marker(Comparable) != None -> { next }
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

Only the root package's own build entry runs. Importing a package and calling
what it exports never runs that package's `build: fn`. Its entry is still
checked with its package, so an error in it fails every importer (E0626).

The graph and environment contracts that a build entry consumes are defined
in the [Jetpack reference](jetpack-epoch5.md). Build-side inspection is
read-only unless a ratified generation operation is used; a build entry cannot
silently rewrite an existing declaration.

The source registry reserves typed item-template generation. A marker's
`expand` template and `b.generate(name) { ... }` carry ordinary Jet item and
statement grammar, and neither form accepts a source string or re-lexes
generated text. The old `derive T.Trait { ... }` provider declaration is not
Jet syntax; a provider is a marker.
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
declaration's name comes only from a compile-time binding, such as the name
a `#loop` binds. A `#{name}` that names no build local stays a
compile-time name of the items.

## Typed reflection

Jet has two related reflection paths:

1. A compile-time fact read is a `$` member of its subject. Examples include
   `Severity.reflect().range`, `send_report.reflect().effects`, `T.reflect().layout`, and
   `#package.name`. These reads produce typed fact records that can be
   used in a compile-time binding or a derive body.
2. `T.reflect()` returns the aggregate `TypeInfo` record computed while
   compiling. It reads the same in a marker body, a `#run` value,
   `build: fn` and ordinary code; in the running program it is a constant.
   `reflect.of(value)` returns the runtime reflection floor for a value.

Each struct field's `FieldInfo` carries its `type`, its `offset` in bytes
(`None` where the layout is not computed) and its `default`: `None` for a
field without a written default, else a `DefaultInfo` whose `source` is the
`SourceSpan` of the default expression. Reading `default` never runs the
expression.

A `FunctionInfo` and a `MethodInfo` share one typed contract: `signature` is
a `CallableSignature`. Its `params` is a `[ParamInfo]` in declaration order,
the receiver included; its `return_type` is the success type the callable
returns, a `String?` that is `None` when it returns no value; its `effects` is an `EffectInfo`;
its `calling_convention` is a `CallingConvention` (`Jet`, `Kernel` for a
`#Kernel` function, or `Foreign` for one whose body is inline foreign code); and
its `generic_bindings` is a `[GenericBindingInfo]`, empty on a declaration,
which binds no generic parameter, and on a specialization's method pairs each
of the owner's type parameters with its argument. No record carries the
signature as text.

Each `ParamInfo` carries the parameter's `name`; the `label` a call writes
(its public label, else its name); its `zone` (`ParamZone`: `PositionalOnly`,
`Either` or `LabelOnly`); its `type`; its `access` (`ParamAccess`: `Read`,
`Write` for a `&` parameter, or `Move` for a `&&` one); its `default`, a
`DefaultInfo?` read the same way as a field's and never run; and whether it is
`variadic`. A catalog reads a callable's parameters without parsing a
signature string (`Examples/Metaprogramming/Reflection/CallableCatalog.jet`).

The signature's `effects` is an `EffectInfo`, and the `effects` fact read
(`send.reflect().effects`) is that same record: it reads
`signature.effects`. `declared` is the row the signature writes, a
`[String]?` that is `None` when it writes none and the row is inferred.
`inferred` is the row the checker solved from the body: it is known after the
check, so `b.program.functions()` reads it, while a read folded inside the
check (`send.reflect().effects`, a marker body, a template) reads `None`.
`values` is the row known where it is read: the solved row once there is one,
else the written row.

A name bound once to a lambda reflects as a `ClosureInfo`: `keep.reflect()`
reads its `name`, its `signature` (a `CallableSignature` whose parameters are
positional and whose `effects` declares no row), the `span` and `location` of
the name, and its `captures`, a `[CaptureInfo]` in the order the captured
bindings are declared. Each `CaptureInfo` carries the binding's `name`, its
`type`, the `span` where it is declared, and its `mode`, the `CaptureMode` the
checker chose: `Read` in place, `Write` to the owner's binding, `Copy` into an
escaping closure, `Materialize` for an owned copy of a view, or `Move` when the
closure takes the value. Reading it is not a use of the closure value, so it
never makes the closure escape. A `:=` name, which can be rebound to another
value, does not reflect as a closure.

Every declaration record names where its declaration is written: the
`location` of a `TypeInfo`, a `FieldInfo` (a field or an enum case), a
`MethodInfo` and a `FunctionInfo` is a `SourceLocation?` whose `file`, `line`
and `column` place the declaration's name, and whose `module` is the module
that declares it; `function` is empty. A function reflects as a type does:
`helper.reflect()` is its `FunctionInfo`. A generic type named with its
arguments reads its definition's location. A declaration the compiler
synthesized, such as a builtin number or a generated module's item, reads
`None`.

```jet
Point: struct { x: Int }

helper: fn() -> Int { 1 }

run: fn() {
    point_site: Point.reflect().location ?? panic("no declaration")
    helper_site: helper.reflect().location ?? panic("no declaration")
    print("Point {point_site.line}")   // Point 1
    print("helper {helper_site.line}") // helper 3
}
```

Each declaration record also lists its `documentation`: the `///` lines
written directly above the declaration, in order, read up past its markers,
its `pub` or `priv` and any other comment, until a blank line or other code.
A `ModuleInfo` lists every comment written in its file as `comments`. Both
hold `CommentInfo` records: the comment's `text` without its `//`, `///` or
`/* */` and one space after the opening, its `span`, and its `kind`
(`CommentKind.Line`, `.Block` or `.Documentation`). The lexer finds the
comments, so a `//` inside a string or a marker's argument stays text: a
marker's arguments are read only through `marker(M)`.

```jet
/// A customer's order.
// Checked at checkout.
#Codable
Order: struct { id: Int }

run: fn() {
    docs: Order.reflect().documentation
    print(docs.len())     // 1
    print(docs[0].text)   // A customer's order.
}
```

A `TypeInfo` lists what the type implements and binds, and the form of a number:

- `traits` has one `TraitInfo` per trait the type implements, with its `name`,
  its `associated_types` (`AssociatedTypeInfo` `name` and `type`) and the
  `methods` the implementation gives. Ask whether a type implements a trait by
  looking for its name in `traits`; there is no string `implements` query.
- `constants` has one `ConstantInfo` per associated constant of those traits:
  its `name`, the `trait_name` that declares it, its declared `type`, and its
  `value` as `print` shows it (`None` when the value is not known at
  compile time, which is reported where the implementation gives it). A
  constant the implementation leaves out reads the trait's default.
- `generic_bindings` pairs each type parameter with the type written for it
  when the type is named with its arguments: `Pair<Int, String>.reflect()`
  binds `A` to `Int` and `B` to `String`. Naming the generic type alone, or
  reflecting its declaration in a marker body, gives an empty list. A type
  named with its arguments reflects that specialization, not the declaration:
  its `identity` names the arguments (`Pair<Int, String>` and
  `Pair<String, Int>` differ), and its fields, variant payloads and method
  signatures read each argument where the declaration writes its parameter
  (a field `first: A` reads `Int`).
- `numeric` is the `NumericInfo` of a number type: whether it is `signed`, its
  width in `bits` and whether it `is_float`. A builtin number reflects too:
  `I32.reflect().numeric` is `signed: true, bits: 32`, `U8`'s is unsigned
  with 8 bits and `F32`'s is a 32-bit float. A `distinct` type over a number
  reports its base's form; any other type has `None`.

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

`info.marker(T)` on a `TypeInfo`, `FieldInfo`, `MethodInfo` or `FunctionInfo`
(and, in a marker body, on a module's `ModuleInfo`) reads the marker
`T` written on the reflected item as a `T?`: a `T` whose fields are the
marker's arguments, or `None` when the item does not carry it. `T` names a
marker struct (one with an `impl Marker` block) or a Prelude marker, so
`info.marker(Codable) != None` asks whether `#Codable` is written; any other
name is E1134. A string test by name (`has_marker("…")`) is gone. The
read gives the same answer in a marker body, a marker that adds methods, a
comptime constant and `build: fn`, as
[`ReflectionRungs`](../../../Examples/Metaprogramming/Reflection/ReflectionRungs/run.jet)
shows: each rung reads the type's name, kind, identity, module, field and
marker counts and `marker(Label)?.text`, and prints the same line; a marker
on the package record and fn build read the package's PackageInfo the same
way.
[`MarkerSites`](../../../Examples/Metaprogramming/Reflection/MarkerSites.jet)
reads `target.marker(T)` at every site kind a marker body runs on, and
[`PackageMarker`](../../../Examples/Metaprogramming/Reflection/PackageMarker/run.jet)
runs one on the package record: a header line above `package.jet`'s first
field.
A PackageInfo's `settings` lists the built package's settings, one
SettingInfo each: the setting's `name`, its `type` as written, its `default`,
the value this build `selected`, and its `contributions`, one ContributionInfo
per writer from least to most explicit. A ContributionInfo has the writer's
`value`, its `source`, where the value is written (such as
`package.jet:build.release.settings.level`), its `kind`
(`ContributionKind.Declaration`, `.Profile`, `.Build` for the package's
`build: fn`, or `.CommandLine` for `--set`) and `selected`, true for the
writer the contribution law chose. Values are the setting's text as written.
The build reads settings from the built package alone, so a dependency's
PackageInfo lists none.
[`PackageSettings`](../../../Examples/Metaprogramming/Reflection/PackageSettings/run.jet)
prints each setting with its writers; under `--release` the profile's value
is selected.
[`ConsoleRegistry`](../../../Examples/Metaprogramming/Reflection/ConsoleRegistry/run.jet)
builds a command registry this way in a plain library: `#CLI.Console` on a
type reads its methods marked `#CLI.Command` and writes `run`, which parses
each argument into the parameter's declared type before the call, and
`help`, which lists each command's typed parameters and its marker's help
text. The Command marker's own body rejects a parameter type no command line
parses. Only the marked type's own methods become commands, so importing a
package that declares commands adds none. A string hole in a template reads
a reflected fact, such as `{m.signature.params[1].name}` or
`{m.marker(Command)?.help ?? ""}`, as that fact's literal.
[`ReflectedInspector`](../../../Examples/Metaprogramming/Reflection/ReflectedInspector/run.jet)
is the editor demo the same way: `#Engine.Entity(version: N)` writes a
struct's inspector rows, an `Edit` impl that sets a field by name from text,
`diff` into a `Patch` that undo and redo replay through `Edit`, and a
positional text `save` and `load`, all from the struct's field rows. The
inspector leaves out a field marked `#Engine.Hidden` and greys a field equal
to the default-built value; `save` still writes a hidden field, so editor and
file policy stay apart. `load` skips a field marked
`#Engine.Since(version: N)` in a file older than N, which keeps it at its
default. A `value ?? fallback` expression in a template body over a reflected
fact, such as `f.marker(Since)?.version ?? 1`, is that fact's literal.

`M.reflect().uses` lists every use of the marker struct `M` in the whole
program, a library's uses in the app build included. Each entry is a
`(m: M, site: ItemInfo)` record: the marker's value at that use (its
arguments, then the declared defaults of the fields left out), under `M`'s
snake_case name (`route` for `Route`), and the carrying item's `ItemInfo`
(`name`, `path`, `module`, and `kind`: `struct`, `enum`, `distinct`,
`function`, `field`, `variant`, `method` or `module`). The list is in module
order, then source order. It folds to a constant after checking, so a program
reads it in a compile-time binding such as `ENTITY_COUNT: #run {
Entity.reflect().uses.len() }`. Only the package that declares `M` reads its
uses; any other package's read is E1136. A use whose marker argument has no
compile-time value is E0302. See
[`MarkerUses`](../../../Examples/Metaprogramming/Reflection/MarkerUses.jet).

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

`#phase` is a `Phase` value (`core.prelude`) that Jet fills in for each
compiled copy, one of exactly `Preparation`, `Build`, and `Runtime`. A
`#run` block or binding reads `.Preparation`. A function body is compiled
once for each phase that runs it: its compile-time copy reads `.Preparation`,
its copy in the build entry (`build: fn` and what it calls) reads `.Build`,
and its copy in the program reads `.Runtime`. Each copy folds its own
`if #phase == ...`, so one helper called from a `#run` binding and from
`run` returns `.Preparation` to the first and `.Runtime` to the second, and
`print(#phase)` in `run` prints `Runtime` on every tier. A local in `run`
whose initializer reads `#phase`, itself or in a function it calls, is never
computed while compiling, so it holds `.Runtime` too.
[`Phase.jet`](../../../Examples/Metaprogramming/Comptime/Phase.jet) shows both
sites, and [`PhaseContract`](../../../Examples/Metaprogramming/Comptime/PhaseContract/run.jet)
adds the `build: fn` copy, a helper whose copies differ, and a prepared record
that keeps `.Preparation` while the program reads it. Do not confuse that language phase with
`CompilerStage`, which contains `Parsed`, `Resolved`, `Typed`, `Prepared`,
and `Emitted` and describes when a checked fact becomes available.

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
build that made it. A second compile (a workspace) is another `Compiler`
value from the same `b.compiler(output)`, one per call; there is no separate
workspace API. Every such session is checked like any compile: in a build
that holds a `Compiler` value, an option that skips or weakens a check
(`b.bounds(.Off)` or `.Report`, set or pinned before `b.compiler` or after
it) is refused (E3503). `#code { items }` is a `Code` value holding its items,
and `meta.add(code, scope: "name")` adds them to the value's target as the
generated module `name`, checked with the program like hand-written code.
A `#{name}` in the items that names a String build local pastes the name or
type it spells, as a template paste does, and the build host refuses text
that is not one name or type's spelling (E0357).
`meta.add` uses `Meta.Generate`, so a function whose row denies that effect
(`-[!Meta.Generate]>`) can't call it. A root package's
`authority: { deny: [Meta.Generate] }` refuses it wherever the root's build
reaches it, a dependency's function included, and a dependency's own `allow`
row widens nothing for its importer.
`meta.rewrite_body(f, code)` replaces the body of the top-level function `f`
with the statements of a `#code { statements }` block. `f` is named directly,
through its package's import (`orm.query`), or by a `FunctionInfo` value such
as the `f` a `meta.events` handler hears.
The rewritten function, its own declaration with the new body, is the
generated module `rewrite_body/f` of `f`'s package, checked with the
program, and its original text is withdrawn from its file. A line `#{f.body}`
in the block, written with the name the call gives `f`, pastes the original
body there, so a rewrite can wrap it. Rewrites of one function compose in the
order they run: a later rewrite's paste is the body the earlier ones made, so
the last rewrite registered is the outermost. It uses `Meta.Rewrite`, which a
row denies with `-[!Meta.Rewrite]>`.
The root package's own functions need no consent. A dependency's function is
rewritten only after the root writes `meta.allow_rewrite(package: "orm")` on
its value, naming that dependency, which the build loads at its locked
version; without it the call fails the build with E3514, which names the
package, the function and the line to add. The rewrite changes only this
build's copy of the dependency, never its cached source.
A report in code a `Compiler` value made ends with a note that names the call
and the function that made it, with its place: the root's own `build`, or a
dependency's function the root handed the value to. The build's provenance
records the same handoff as the module's `made_by`, in `jet inspect graph
--json` and in `jet inspect explain-build <module>`.
The authority and the consent a build grants freeze when `build` returns:
every later call on its `Build` or `Compiler` values is refused, so the
actions and compiles after it read the frozen state. A module a `Compiler`
value made records both: its `authority`, the Meta effect its call performed
(`Meta.Generate` for `meta.add`, `Meta.Rewrite` for `meta.rewrite_body`), and
its `consent`, the dependency a `meta.allow_rewrite` line allowed (null for
the root's own code). Both appear in its provenance, and both are inputs of
its check node, so a change to either is a new identity for that module and
for every compile and link node after it (`jet inspect explain-build
<file> --json`).
`meta.events(watch: .Functions{marked: Traced{}}, handler)` on the value calls
the handler with `.Checked(.Function(f))`, a `CompilerEvent`, once for each
function of the build's checked program that carries the marker of the
watched value's type, in the order the program lists them; the value's fields
filter nothing, and a value that is no marker struct's is E1134.
`meta.events(watch: .Calls{to: print}, handler)` calls the handler with
`.Checked(.Call(c))` once for each checked call written in the root package's
own source (outside its `build: fn`) whose resolved callee is the watched one,
in the order the checker met them; `c` is a `CompilerCall` (caller, callee,
module, span). The watched name is a function, or an ambient call such as
`print` that no name in scope hides; a function or method of the same name
elsewhere is another callee, and any other name is refused (E0104). Watching
only reads, so it needs no Meta effect, and the build refuses a value whose
build has returned. A handler that rewrites a heard function `f` uses
`Meta.Rewrite` like any other caller; `f.package` is the identity of the
package that declares it.
A handler runs inside the build's own call, so it spends the build's
compile-time step budget, the one every compile-time call has: the build and
all its handlers share it. A build that runs out stops there with E0952, and
the build publishes nothing: no handler's `meta.add` lands and nothing is
compiled. Only a program run (`jet eval`, like a built binary) has no budget.
Code that `meta.add` or `meta.rewrite_body` hands the compiler can't write
`#Unsafe`: it is never audited where it lands, so the call is refused (E3503)
and the build publishes nothing. An unsafe operation stays in a function of
the package's source, with its own `#Unsafe("reason")` block, and the code
calls that function. A body pasted in through `#{f.body}` is the function's
own source, and it keeps the blocks that source wrote.
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
facts, and no host permission carried by a Meta effect. `b.compiler`, `#code`,
`meta.add`, `meta.rewrite_body`, its `#{f.body}` paste, `meta.allow_rewrite`
and `meta.events` watching marked functions and resolved calls are built; the
other operations on the value are ratified contract and not yet built.
The source examples and the two executable registries linked above are the
evidence; this page does not turn a schema row into an implementation claim.
