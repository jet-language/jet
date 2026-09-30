# Core library (`core`)

Vocabulary: [Jet vocabulary](../vocabulary.md).

This reference is for Jet application authors who need the standard modules for
files, data, networking, security, command-line programs, and system
integration. The executable declarations live in [`Core/`](../../../Core/);
the Prelude registry lists exports and module paths in
[`Core.jet`](../../../crates/jet-codegen/src/Prelude/Core.jet), and the
[`Examples/`](../../../Examples/) and [`tests/`](../../../tests/) directories
exercise the contracts. The Rust-hosted compiler remains the reference
execution path; `Compiler/` is a staged compiler port, not a claim that Jet is
self-hosted.

<!-- Stable IDs bind these public Core declarations to reviewed feature depth. -->
<!-- FEATURE_CLAIMS:BEGIN -->
<!-- FEATURE_CLAIM: claim.core-foundation | Core foundations are reachable Jet software. -->
<!-- FEATURE_CLAIM: claim.core-concurrency | Tasks and events share one runtime. -->
<!-- FEATURE_CLAIM: claim.core-files-data | Files, paths, archives, compression, and DB APIs are production claims. -->
<!-- FEATURE_CLAIM: claim.core-encoding-text | Codecs and text follow published standards. -->
<!-- FEATURE_CLAIM: claim.core-network-http | Network and HTTP claims require live interoperability. -->
<!-- FEATURE_CLAIM: claim.core-security | Security APIs require fail-closed entropy. -->
<!-- FEATURE_CLAIM: claim.core-data-compute | Typed data claims require real documented semantics. -->
<!-- FEATURE_CLAIM: claim.core-ui-web | UI and web share one typed component model. -->
<!-- FEATURE_CLAIM: claim.game-product | Game claims require a playable runtime and editor. -->
<!-- FEATURE_CLAIM: claim.plugin-ffi | Plugins and FFI use one typed interop structure. -->
<!-- FEATURE_CLAIMS:END -->

Core is the canonical first-party namespace. Import modules by their `core.*`
name; there is no `jet.*` or `std.*` library namespace. Core wrappers keep
source-facing types and policy in Jet while typed provider leaves perform the
operations that require the host environment.

Types and error enums use PascalCase (`String`, `IOError`, `JSON`); functions
and module segments use snake_case (`read`, `core.files`) (S54). Acronyms keep
their declared spelling; see S66.

## Quick start

```jet
use core.files as fs
use core.term as term
use core.sys as sys
use core.process as process

fn run() {
    args :: process.argv()
    if args.len() < 2 {
        term.eprint("usage: greet <name>")
        return
    }
    name :: args.get(1) ?? return
    greeting :: sys.get("GREETING") ?? "hello"
    fs.write("/tmp/greet.txt", "{greeting}, {name}!") ?? return
    print(fs.read("/tmp/greet.txt") ?? return)
}
```

Everything after `--` is forwarded to the program:

```sh
jet run tool.jet -- World --verbose
jet build tool.jet
```

`process.argv()` retains `argv[0]`; `process.args()` supplies only the
arguments after it. The process and subprocess API is documented in the
[core.process](#coreprocess--process-execution) section below.

## Importing modules and compile-time views

Core modules use `use` without quotes. Quoted imports name `.jet` files, not
compiler-known Core modules:

```jet
use core.files as fs
use core.encoding.json as json
```

`use core.files` and `use core.encoding.json` resolve under the `core` root.
The compiler checks the module and item names; an unknown module is E1001 and
an unknown item in a known module is E1004. A local package/file name reserved
for a first-party package (`core`, `jet`, `http`, `regex`, `csv`, `toml`,
`crypto`, or `archive`) is E1002. A member list after a Core module names
modules, not its items, so keep qualified access through an alias. Quoted
module-like spelling is not an alternative:

```jet
import core.files as fs
use "core/files"
```

The first form is an ordinary import error under D-S14-PAUSE; the second uses
the file-import grammar.

### `core.compiler.lang` — language declarations

`core.compiler.lang` publishes the closed compiler vocabulary used by typed
marker arguments. These declarations are ordinary generated enums and structs;
the marker registry, diagnostics, completion, documentation, and reflection
use the same names.

| Declaration | Kind | Purpose |
| --- | --- | --- |
| `ABI`, `FfiLanguage`, `Target`, `Track` | enum | Calling convention, foreign language, code-generation target, and tracking policy values |
| `ArithmeticMode`, `InlineMode`, `JobScope`, `KernelMode`, `Layout`, `MemoBound` | enum | Arithmetic, inlining, publication scope, parallel-kernel, representation, and memoization choices |
| `Effect`, `Maturity`, `NamingCase`, `ObligationMode`, `PolicySetting`, `Site`, `TaintKind` | enum | Effect roots, maturity, field naming, proof obligations, policy, marker sites, and taint values |
| `Path`, `State` | struct | Marker path text and compiler-threaded typestate names |

For example, `#Inline` accepts an imported enum value or a dot literal:

```jet
use core.compiler.lang as lang

#Inline(lang.InlineMode.Always)
fn parse_fast(text: String) -> Int {
    text.to_int() ?? 0
}
```

`#Inline(.Always)` is the corresponding expected-type form. The language
module contains `ArithmeticMode`, `JobScope`, `KernelMode`, `MemoBound`, and
`Site`; it does not export the retired `IntType` declaration.

### `core.compiler` — compiler-owned package views

`core.compiler` is a read-only, compile-time boundary. `lex`, `parse`, `check`,
and `source_map` expose compiler-owned front-end results; `manifest`, `package`,
`lock`, and `profiles` expose authority-checked package views. These calls do
not read arbitrary paths, dependencies, or sibling packages, and they do not
provide a runtime path-loading fallback. A runtime call is E0956.

The package-view field matrix is versioned by
`PACKAGE_MODEL_SCHEMA_VERSION = 1` (D-PACKAGE-MODEL1). The four views have
separate meanings:

| View | Shape | Ordering and scope |
| --- | --- | --- |
| `manifest()` | `CompilerManifest` | The uncomposed `package.jet` or inline package carrier. `dependencies` and `outputs` use manifest map-key order; `packages` and `build_profiles` keep declaration order. |
| `package()` | `CompilerPackage` | The package after declared Config files compose successfully. It uses the manifest fields and the canonical current-package identity; `name` and `version` remain the `$build.package` values. |
| `lock()` | `CompilerLock` | `schema_version`, `file`, `version`, `root_dependencies`, and locked package records. Root dependencies and locked packages retain lock-model order. |
| `profiles()` | `CompilerProfileSet` | `schema_version`, `file`, and named profiles. Profile and collision maps use key order; `extends`, `packages`, and `sources` retain declaration order. |

`CompilerDependency` requires `name` and `source`;
`CompilerPackageTarget` requires `name` and `targets`; and
`CompilerPackageOutput` requires `name` and `kind` with optional `entry`.
`CompilerBuildProfile` requires `name`, `optimize`, `debug_info`, and `small`
with optional `panic`. Missing optional source fields are `Option<String>`
values and missing collections are empty lists. Git sources omit credentials and
URL query or fragment material from the projection.

Every operation returns a named result. A failed package read carries
`PackageReadError { code, message, file, cause }`; the Core carrier is
`CompilerPackageError` with the same four fields. Malformed, changed, missing,
or escaping inputs are errors, not empty views. Causes retain typed diagnostic
codes and logical reasons without authority paths or secret material. Relative
hashed build inputs record the package manifest and each contributing Config,
lock, or profile file for build-cache identity. The views do not invent
field-level source positions.

```jet
use core.compiler as compiler

@manifest :: compiler.manifest() ?? panic("manifest")
@package :: compiler.package() ?? panic("package")
@lock :: compiler.lock() ?? panic("lock")
@profiles :: compiler.profiles() ?? panic("profiles")
```

### `core.mod` — pinned library loading

`core.mod.load` loads a project-contained compiled Jet library under an explicit
read grant. The provider validates the artifact, package boundary, and ABI
before library code runs; the returned `Mod` owns its provider handle and
releases it when dropped.

```jet
use core.mod as library

fn run() {
    library.load(".jet/build/loadable.jetlib", grant: {read: [".jet/build"]}) ?? return
}
```

The grant must be explicit and non-empty. `core.mod` does not dynamically
search arbitrary paths, bypass a package boundary, or manufacture a detached
handle. The source-facing `CompiledModule` inspection helpers are:

| API | Result | Description |
| --- | --- | --- |
| `load(path, grant)` | `-[FS, Exec]> Mod !Err` | Load one granted compiled library. |
| `is_loaded(compiled)` | `Bool` | Inspect a `CompiledModule` carrier. |
| `path(compiled)` | `String` | Return the carrier's recorded path. |
| `unload(compiled)` | `-[FS]> CompiledModule !Err` | Return the typed refusal; provider-owned `Mod` handles unload on drop. |

## Errors and optional values

Core's fallible calls use a success value plus a named error carrier. A plain
call propagates a failure from the enclosing function; `??` handles it as a
fallback, and `?(text)` adds context while preserving propagation:

```jet
use core.files as fs

fn run() {
    text :: fs.read("data.txt")
    fs.write("out.txt", text.to_upper())
}

text :: fs.read("data.txt") ?? ""
annotated :: fs.read("data.txt")?("reading the input for out.txt")
```

A function without an explicit error type uses the default `Err`. Core's
standard error family, including `IOError` and `EncodingError`, converts into
that default carrier. For an application-owned target, declare one conversion
and name it in the function contract:

```jet
#Error
enum AppErr { Read(IOError) }

impl IOError -> AppErr { return AppErr.Read(self) }

fn load() -> String AppErr! {
    text :: fs.read("data.txt")
    return Ok(text)
}
```

Conversions into a typed target require ownership of the source or target
error. An application error still needs an explicit conversion before it can
propagate into `Err`. `TaskFailure` is outside Core's automatic conversion
family; handle it explicitly or declare an allowed
conversion. The mechanism is the same `impl Source -> Target` declaration in
all cases (D-ERR-CONV, D-FAIL-CONV1, D-FAIL-CONV2).

`?T` is either `Val(x)` or `None`. Pattern tests and `??` are the primitive
forms; combinators compose several optionals without introducing a second
absent-propagating value type (D-HOLE1):

| API | Result | Description |
| --- | --- | --- |
| `option.map(f)` | `?R` | Apply `f` to a present payload; preserve `None`. |
| `option.zip(other)` | `?(a: T, b: U)` | Produce a pair only when both operands are present. |
| `Option.lift2(f, a, b)` | `?R` | Apply a two-argument function only when both options are present. |

```jet
price :: lookup_price(id)
qty :: lookup_qty(id)
total :: price.zip(qty).map((pair) -> pair.a * pair.b)
```

## Collections and iterators

Core uses `[T]` for lists, `[K:V]` for ordered maps, and named types for
specialized behavior. Methods on a concrete list, map, or set are eager:
`map`, `filter`, `flatten`, and `flat_map` return concrete collections. Calling
`.lazy()` enters the deferred `Iter<T>` view; `to_list()`, `collect()`, or a
terminal reducer drives it. File lines, streams, channels, and String splitting
already produce deferred sources (D-CORE-EAGER1=A, D-CORE-EAGER2=A,
D-LOOPMAP1=B).

Map keys are restricted by E0502 to `Int`, `Bool`, `String`, `Char`, and
`IntN`, plus tuples whose fields recursively satisfy that rule. Floats, maps,
functions, shared values, and other unsupported types are not map keys. Set
elements require `Hash + Eq` (E0506); lists, options, tuples, results, and
other structural values are eligible when their contents are hashable, while
maps, floats, shared values, and functions are not.

A finite `loop ... -> value` evaluates immediately to a list. An expected type
does not change collection choice or evaluation time. The naming law in
[`stdlib-api-laws.md`](../stdlib-api-laws.md) owns verb choices; this table
records the shipped surface without adding aliases:

| Type | Constructors | Description |
| --- | --- | --- |
| `[T]` | `[a, b]` | Eager closure methods, sequence adapters, reducers, `lazy`, `to_set`, `join`, `compare`, `split`, `starts_with`, `ends_with`, `slice`, `copy`, and `equal`. |
| `[K:V]` | `["a": 1]`, `Map.new()`, `Map.from_keys(keys, default)` | Ordered `keys`, `values`, `get`, `has_key`, `add`, `add_new`, `remove`, `pop`, `merge`, `copy`, `equal`, `map`, `filter`, `flat_map`, `fold`, `intersection`, `slice`, `top_n`, and reducers. |
| `Set<T>` | `Set.new()`, `Set.from(xs)` | Hash-set `add`, `remove`, `pop`, `has`, set algebra, subset/disjoint checks, `copy`, `first`, `values`, closure methods, `to_list`, and size/clear operations. |
| `Rank<T>` | `Rank.new()`, `Rank.from(xs)` | Ordered set insertion, removal, membership, first/last, set algebra, and list conversion. |
| `Queue<T>` | `Queue.new()`, `Queue.init(xs)` | Double-ended push/pop and peek, capacity, lookup/delete, list conversion, join, reverse, split, and size/clear operations. |
| `PriorityQueue<T>` | `PriorityQueue.new()`, `PriorityQueue.from(xs)` | `push`, `pop`, `peek`, `to_sorted_list`, `remove`, and size/clear operations. |
| `Cache<K,V>` | `Cache.new(capacity)` | Bounded `add`, `add_new`, `get`, `remove`, `has_key`, `keys`, and size/clear operations. |
| `Tally<T>` | `Tally.new()`, `Tally.from(xs)` | Counting `add`, `remove`, `has`, `count`, list conversion, and size/clear operations. |
| `Bits` | `Bits.new()` | Integer-set `add`, `remove`, `has`, `count`, list conversion, copying, and size/clear operations. |
| `Bytes` | `Bytes.new()`, `Bytes.with_capacity(n)`, `Bytes.from(bytes)` | Byte-buffer reads/writes, cursor movement, text conversion, splitting/joining, comparison, copying, flushing, and closing. |

`partition` is stable, and the list sort family preserves source order for equal
keys. `min_by` and `max_by` retain the last source item for equal keys.
`binary_search` returns a matching index, not an insertion position or a
promise to return the first equal item. `counts()` returns a map of element
counts; `top_n(n)` orders by descending count and ascending key for ties.
`para_map` preserves source order, and `para_map(f, limit: n)` bounds workers
for that call.

Set closure operations use the same collection kernels as other containers.
Mapping a Set does not promise to preserve uniqueness; use `.to_set()` when
the result must be deduplicated. `values` is the lazy alias of `to_list`,
`first` has arbitrary hash order, and `pop` removes and returns an element.
`Set.sort()` and `Set.shuffle()` return fresh lists and do not mutate the Set
(D-SET-DECLINE1). `indexof`, `indexed`, `flatten`, and `copyto` remain declined
for an unordered Set or Rank; convert to a list first. There is no stable
positional index for either type.

For an `Iter<T>` positional pick, use `skip(n).first()`; `n` is zero-based and
an out-of-range pick is `None`. `nth` is not part of the API. The lazy adapter
family includes `map`, `filter`, `take`, `skip`, `step_by`, `dedup`,
`dedup_by`, `chunks`, `windows`, `chunk_while`, `flatten`, `intersperse`,
`indexed`, `indexes`, `zip`, `zip_short`, `zip_pad`, `take_while`,
`skip_while`, `flat_map`, `filter_map`, `scan`, `cycle`, `repeat`,
`drop_last`, and `shuffle`. `cycle(n)` is bounded and produces exactly `n`
items; `repeat(n)` repeats the source `n` times. A zero-argument infinite cycle
is not a Core API. `fill`, `cycle_n`, and `duplicate` are declined or mapped
to the existing names under D-ITER-DECLINE1.

`next()` is the one partial pull (D-ITER-RESUME1, which amends
D-ITER-DECLINE1 for `next` only). It needs exclusive access to a writable
binding, written `&items.next()`, pulls exactly one item, and leaves the
remainder in the same source: after two pulls from `[1, 2, 3].lazy()`,
`to_list()` returns `[3]`. The end is `None`. A moved source cannot be pulled,
and a loop that drives a source holds it for its whole body, so pulling from
the same source inside that body is rejected (E0507). A helper that pulls from
a caller's source names it as `&Iter<T>`.
`first()` is different: it consumes the whole iterator and returns only its
first item. Adapter callbacks run only when an item is pulled through them, and
the item that ends a `take_while` is consumed and not yielded. User types that
implement `Iterator` expose the same `next` with the same exclusive receiver.

The zip family is available as a free call or a method. `zip` requires equal
lengths, `zip_short` stops at the shortest input, and `zip_pad` reaches the
longest. Omitted padding is `None`; `fill:` supplies one value for every
missing column and `fills:` supplies one per named column.

```jet
left :: [1, 2, 3]
right :: [10, 20]

loop row in left.zip_pad(right, fill: 0) {
    print(row.b)
}
```

Examples covering the eager/lazy boundary and the adapter ledger are in
[`iter_adapters.jet`](../../../Examples/features/collections/iter_adapters.jet),
[`lazy_iter.jet`](../../../Examples/features/collections/lazy_iter.jet),
[`list_surface.jet`](../../../Examples/features/collections/list_surface.jet),
[`map_surface.jet`](../../../Examples/features/collections/map_surface.jet), and
[`set.jet`](../../../Examples/features/collections/set.jet).

### Operation contract

Every collection and iterator method above is one checked row in
`Compiler/JetFoundation/Source/Collections.jet` (D-ONE-OPERATIONS1). The
compiler reads arity, receiver access, argument and callback shapes, result
type, lowering and the `Mem` effect from that row, and the tables below are
generated from the same rows. Each row names a law; the law states when the
work happens, what the result owns, how often and in what order callbacks run,
whether the item that stops a traversal is consumed, the failure channel and
what must be buffered. Status is `current` when every engine runs the row and
`source-only` when the self-hosted checker accepts it before every engine does.

Empty inputs follow the logical identities: `[].all(p)` is `true`,
`[].any(p)` is `false`, `sum` is `0`, `product` is `1`, and `fold` returns
its seed. `any`, `all`, `find` and `position` stop at the deciding item; later
items and callbacks never run. A fallible `map` or `filter` callback returns
its first `Err` as the whole result, never a partial list presented as a
success. `zip` rejects unequal lengths, `zip_short` stops at the shortest
input, and `zip_pad` pads to the longest; the zip family and `para_*` keep
their dedicated checkers. `min_by` and `max_by` keep the last item on ties
(#2298), and list methods stay eager by default (D-CORE-EAGER1,
D-CORE-EAGER2). [`operation_contract.jet`](../../../Examples/features/collections/operation_contract.jet)
runs each of these cases.

<!-- BEGIN GENERATED COLLECTION OPERATIONS -->
<!-- Source: Compiler/JetFoundation/Source/Collections.jet; regenerate with node Tools/agent/gen-core-tables.mjs --write -->

| Law | Timing | Result | Callbacks | Stop item | Failure channel | Buffering |
| --- | --- | --- | --- | --- | --- | --- |
| Measure | eager | a fresh scalar | none | an iterator receiver is consumed whole | none | none |
| Lookup | eager | an optional copy of the found item or index | none | an iterator receiver is consumed and cannot be resumed | absence is None | none |
| Mutate | eager, in place | Unit, a Bool, or the displaced item | none | not applicable | absence is None; no error channel | the receiver's own storage |
| TryMutate | eager, in place | a Result | none | not applicable | AllocError when storage cannot grow | the receiver's own storage |
| Build | eager | a fresh owned collection | none | the whole receiver is read | none | the whole output |
| Copy | eager | an independent value; storage is shared until the first write | none | not applicable | none | none until the first write |
| Adapt | lazy | an iterator that owns the adapted source | none | pulls only the items it yields or skips | none | none |
| Window | lazy | an iterator of lists | none | pulls one window or chunk at a time | none | one window or chunk |
| Visit | eager on lists and maps, lazy on iterators | a List or Map on eager receivers, an iterator on iterator receivers | once per item, in source order | every item is visited | a callback Err is the whole result; no partial output | eager receivers buffer the output; iterators buffer nothing |
| Each | eager | Unit | once per item, in source order | every item is visited | none | none |
| Decide | eager | a Bool, Option or position | once per item, in source order, until the answer is known | the deciding item is consumed; later items are never read and an iterator receiver is dropped | none | none |
| Stream | lazy | an iterator | once per pulled item, in source order | the item that ends take_while is consumed and not yielded | none | the current item or group |
| Fold | eager | the accumulator or chosen item; the seed or None when empty | once per item, in source order | every item is read; equal keys keep the last item | none | none |
| Group | eager | a fresh map, tuple or lists | once per item, in source order, when a key callback is given | every item is read | none | every item is buffered into the output |
| Drain | eager | a fresh value | none | the whole source is consumed | none | the output |
| Reorder | eager: the whole source is read before the first result | the reordered or trimmed items | none | the whole source is consumed | none | the whole source |
| SortBy | eager, in place | Unit, or a Result when the key callback can fail | key or comparator calls in the sort's order; count and order are unspecified | not applicable | the first callback Err is the result | the list and its keys |
| Search | eager | an optional index | once per probed item, in binary-search order | stops at the match | absence is None | none |
| MergeEntries | eager | a fresh map | once per key present in both maps | every entry is read | none | the output map |
| Pull | one item per call | an Option of the next item | none | exactly the returned item is consumed; the remainder stays in the source | end is None | none |

| Receiver | Operation | Receiver ownership | Law | Empty result | Status |
| --- | --- | --- | --- | --- | --- |
| `Set<T>` | `map((item) -> T)` | borrows | Visit | empty list | current |
| `Set<T>` | `filter((item) -> T)` | borrows | Visit | empty list | current |
| `Set<T>` | `each((item) -> T)` | borrows | Each | Unit | current |
| `Set<T>` | `all((item) -> Bool)` | borrows | Decide | true | current |
| `Set<T>` | `flat_map((item) -> T)` | borrows | Visit | empty iterator | current |
| `Set<T>` | `fold(seed, (acc, item) -> acc)` | borrows | Fold | the seed | current |
| `Set<T>` | `min()` | borrows | Lookup | None | current |
| `Set<T>` | `max()` | borrows | Lookup | None | current |
| `Set<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `copy()` | borrows | Copy | empty Set | current |
| `Set<T>` | `to_set()` | borrows | Copy | empty Set | current |
| `Set<T>` | `add(item)` | exclusive, in place | Mutate | true | current |
| `Set<T>` | `remove(item)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `discard(item)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `has(item)` | borrows | Measure | false | current |
| `Set<T>` | `pop(item)` | exclusive, in place | Mutate | None | current |
| `Set<T>` | `take(item)` | exclusive, in place | Mutate | None | current |
| `Set<T>` | `replace(item)` | exclusive, in place | Mutate | None | current |
| `Set<T>` | `union(same kind)` | borrows | Build | the argument | current |
| `Set<T>` | `intersection(same kind)` | borrows | Build | empty Set | current |
| `Set<T>` | `difference(same kind)` | borrows | Build | empty Set | current |
| `Set<T>` | `symmetric_difference(same kind)` | borrows | Build | the argument | current |
| `Set<T>` | `equal(same kind)` | borrows | Measure | true when both are empty | current |
| `Set<T>` | `is_subset(same kind)` | borrows | Measure | true | current |
| `Set<T>` | `issubset(same kind)` | borrows | Measure | true | current |
| `Set<T>` | `is_superset(same kind)` | borrows | Measure | true only for an empty argument | current |
| `Set<T>` | `issuperset(same kind)` | borrows | Measure | true only for an empty argument | current |
| `Set<T>` | `is_disjoint(same kind)` | borrows | Measure | true | current |
| `Set<T>` | `isdisjoint(same kind)` | borrows | Measure | true | current |
| `Set<T>` | `update(same kind)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `difference_update(same kind)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `intersection_update(same kind)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `symmetric_difference_update(same kind)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `len()` | borrows | Measure | 0 | current |
| `Set<T>` | `is_empty()` | borrows | Measure | true | current |
| `Set<T>` | `capacity()` | borrows | Measure | 0 or more | current |
| `Set<T>` | `to_list()` | borrows | Build | empty list | current |
| `Set<T>` | `first()` | borrows | Lookup | None | current |
| `Set<T>` | `values()` | borrows | Adapt | empty iterator | current |
| `Set<T>` | `sort()` | borrows | Reorder | empty list | current |
| `Set<T>` | `shuffle()` | borrows | Reorder | empty list | current |
| `Rank<T>` | `add(item)` | exclusive, in place | Mutate | true | current |
| `Rank<T>` | `remove(item)` | exclusive, in place | Mutate | Unit | current |
| `Rank<T>` | `has(item)` | borrows | Measure | false | current |
| `Rank<T>` | `union(same kind)` | borrows | Build | the argument | current |
| `Rank<T>` | `intersection(same kind)` | borrows | Build | empty Rank | current |
| `Rank<T>` | `difference(same kind)` | borrows | Build | empty Rank | current |
| `Rank<T>` | `symmetric_difference(same kind)` | borrows | Build | the argument | current |
| `Rank<T>` | `is_subset(same kind)` | borrows | Measure | true | current |
| `Rank<T>` | `is_superset(same kind)` | borrows | Measure | true only for an empty argument | current |
| `Rank<T>` | `is_disjoint(same kind)` | borrows | Measure | true | current |
| `Rank<T>` | `len()` | borrows | Measure | 0 | current |
| `Rank<T>` | `is_empty()` | borrows | Measure | true | current |
| `Rank<T>` | `to_list()` | borrows | Build | empty list | current |
| `Rank<T>` | `first()` | borrows | Lookup | None | current |
| `Rank<T>` | `last()` | borrows | Lookup | None | current |
| `Rank<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `Bits` | `add(Int)` | exclusive, in place | Mutate | true | current |
| `Bits` | `remove(Int)` | exclusive, in place | Mutate | Unit | current |
| `Bits` | `has(Int)` | borrows | Measure | false | current |
| `Bits` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `Bits` | `copy()` | borrows | Copy | empty Bits | current |
| `Tally<T>` | `add(item)` | exclusive, in place | Mutate | true | current |
| `Tally<T>` | `remove(item)` | exclusive, in place | Mutate | Unit | current |
| `Tally<T>` | `has(item)` | borrows | Measure | false | current |
| `Tally<T>` | `count(item)` | borrows | Measure | 0 | current |
| `Tally<T>` | `any((item) -> Bool)` | borrows | Decide | false | current |
| `Tally<T>` | `len()` | borrows | Measure | 0 | current |
| `Tally<T>` | `is_empty()` | borrows | Measure | true | current |
| `Tally<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `Cache<K,V>` | `add(key, value)` | exclusive, in place | Mutate | None | current |
| `Cache<K,V>` | `add_new(key, value)` | exclusive, in place | Mutate | true | current |
| `Cache<K,V>` | `get(key)` | exclusive, in place | Lookup | None | current |
| `Cache<K,V>` | `remove(key)` | exclusive, in place | Mutate | None | current |
| `Cache<K,V>` | `has_key(key)` | borrows | Measure | false | current |
| `Cache<K,V>` | `len()` | borrows | Measure | 0 | current |
| `Cache<K,V>` | `capacity()` | borrows | Measure | the constructed capacity | current |
| `Cache<K,V>` | `is_empty()` | borrows | Measure | true | current |
| `Cache<K,V>` | `keys()` | borrows | Build | empty list | current |
| `Cache<K,V>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `Queue<T>` | `push_front(item)` | exclusive, in place | Mutate | Unit | current |
| `Queue<T>` | `push_back(item)` | exclusive, in place | Mutate | Unit | current |
| `Queue<T>` | `delete(item)` | exclusive, in place | Mutate | Unit | current |
| `Queue<T>` | `get(Int)` | borrows | Lookup | None | current |
| `Queue<T>` | `split(Int)` | exclusive, in place | Mutate | empty Queue | current |
| `Queue<T>` | `join(String)` | borrows | Build | empty string | current |
| `Queue<T>` | `contains(item)` | borrows | Measure | false | current |
| `Queue<T>` | `len()` | borrows | Measure | 0 | current |
| `Queue<T>` | `capacity()` | borrows | Measure | 0 or more | current |
| `Queue<T>` | `is_empty()` | borrows | Measure | true | current |
| `Queue<T>` | `peek_front()` | borrows | Lookup | None | current |
| `Queue<T>` | `peek_back()` | borrows | Lookup | None | current |
| `Queue<T>` | `to_list()` | borrows | Build | empty list | current |
| `Queue<T>` | `pop_front()` | exclusive, in place | Mutate | None | current |
| `Queue<T>` | `pop_back()` | exclusive, in place | Mutate | None | current |
| `Queue<T>` | `reverse()` | exclusive, in place | Mutate | Unit | current |
| `Queue<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `PriorityQueue<T>` | `remove(item, [RemoveBy])` | exclusive, in place | Mutate | None | current |
| `PriorityQueue<T>` | `push(item)` | exclusive, in place | Mutate | Unit | current |
| `PriorityQueue<T>` | `len()` | borrows | Measure | 0 | current |
| `PriorityQueue<T>` | `is_empty()` | borrows | Measure | true | current |
| `PriorityQueue<T>` | `pop()` | exclusive, in place | Mutate | None | current |
| `PriorityQueue<T>` | `peek()` | borrows | Lookup | None | current |
| `PriorityQueue<T>` | `to_sorted_list()` | borrows | Reorder | empty list | current |
| `PriorityQueue<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `[K:V]` | `add(key, value)` | exclusive, in place | Mutate | None | current |
| `[K:V]` | `try_insert(key, value)` | exclusive, in place | TryMutate | Ok(None) | current |
| `[K:V]` | `add_new(key, value)` | exclusive, in place | Mutate | true | current |
| `[K:V]` | `setdefault(key, value)` | exclusive, in place | Mutate | the default | current |
| `[K:V]` | `remove(key)` | exclusive, in place | Mutate | None | current |
| `[K:V]` | `pop(key)` | exclusive, in place | Mutate | None | current |
| `[K:V]` | `get(key)` | borrows | Lookup | None | current |
| `[K:V]` | `has_key(key)` | borrows | Measure | false | current |
| `[K:V]` | `contains_value(value)` | borrows | Measure | false | current |
| `[K:V]` | `pop_first()` | exclusive, in place | Mutate | None | current |
| `[K:V]` | `update(map)` | exclusive, in place | Mutate | Unit | current |
| `[K:V]` | `merge(map, [(key, old, new) -> value])` | borrows | MergeEntries | the argument | current |
| `[K:V]` | `equal(map)` | borrows | Measure | true when both are empty | current |
| `[K:V]` | `intersection(map)` | borrows | Build | empty map | current |
| `[K:V]` | `slice([key])` | borrows | Build | empty map | current |
| `[K:V]` | `top_n(Int)` | borrows | Reorder | empty list | current |
| `[K:V]` | `map((key, value) -> T)` | borrows | Visit | empty map | current |
| `[K:V]` | `filter((key, value) -> Bool)` | borrows | Visit | empty map | current |
| `[K:V]` | `flat_map((key, value) -> map)` | borrows | Visit | empty map | current |
| `[K:V]` | `each((key, value) -> T)` | borrows | Each | Unit | current |
| `[K:V]` | `any((key, value) -> Bool)` | borrows | Decide | false | current |
| `[K:V]` | `all((key, value) -> Bool)` | borrows | Decide | true | current |
| `[K:V]` | `fold(seed, (acc, key, value) -> acc)` | borrows | Fold | the seed | current |
| `[K:V]` | `copy()` | borrows | Copy | empty map | current |
| `[K:V]` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `View<T>` | `map((item) -> T)` | borrows | Visit | empty list | current |
| `View<T>` | `fold(seed, (acc, item) -> acc)` | borrows | Fold | the seed | current |
| `[T]` | `push(item)` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `append(item)` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `try_push(item)` | exclusive, in place | TryMutate | Ok(Unit) | current |
| `[T]` | `try_reserve(Int)` | exclusive, in place | TryMutate | Ok(Unit) | current |
| `[T]` | `insert(Int, item)` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `remove(item, [RemoveBy])` | exclusive, in place | Mutate | None | current |
| `[T]` | `pop()` | exclusive, in place | Mutate | None | current |
| `[T]` | `reverse()` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `get(Int)` | borrows | Lookup | None | current |
| `[T]` | `index_of(Int)` | borrows | Lookup | None | current |
| `[T]` | `index(item)` | borrows | Lookup | None | current |
| `[T]` | `contains(item)` | borrows | Measure | false | current |
| `[T]` | `count(item)` | borrows | Measure | 0 | current |
| `[T]` | `copy()` | borrows | Copy | empty list | current |
| `[T]` | `update_first((item) -> Bool, item)` | exclusive, in place | Decide | false | current |
| `[T]` | `replace(Int, item)` | borrows | Build | empty list | current |
| `[T]` | `binary_search(item)` | borrows | Lookup | None | current |
| `[T]` | `binary_search_by((item) -> T)` | borrows | Search | None | current |
| `[T]` | `random()` | borrows | Lookup | None | current |
| `[T]` | `min_max()` | borrows | Lookup | None | current |
| `[T]` | `min_max_by((item) -> T)` | borrows | Fold | None | current |
| `[T]` | `sort_by((item) -> key or (a, b) -> Ordering)` | exclusive, in place | SortBy | Unit | current |
| `[T]` | `sort_by_desc((item) -> key)` | exclusive, in place | SortBy | Unit | current |
| `[T]` | `sort()` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `sort_desc()` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `lazy()` | borrows | Adapt | empty iterator | current |
| `[T]` | `shuffle()` | borrows | Reorder | empty iterator | current |
| `[T]` | `to_set()` | borrows | Build | empty Set | current |
| `[T]` | `concat([item])` | borrows | Build | the argument | current |
| `[T]` | `extend([item])` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `starts_with([item])` | borrows | Measure | true only for an empty prefix | current |
| `[T]` | `ends_with([item])` | borrows | Measure | true only for an empty suffix | current |
| `[T]` | `equal([item])` | borrows | Measure | true when both are empty | current |
| `[T]` | `union([item])` | borrows | Build | the argument's distinct items | current |
| `[T]` | `intersection([item])` | borrows | Build | empty list | current |
| `[T]` | `difference([item])` | borrows | Build | empty list | current |
| `[T]` | `slice(Int, Int)` | borrows | Build | empty list | current |
| `Iter<T>` | `shuffle()` | consumes | Reorder | empty iterator | current |
| `Iter<T>` | `to_list()` | consumes | Drain | empty list | current |
| `Iter<T>` | `collect()` | consumes | Drain | empty list | current |
| `Iter<T>` | `next()` | exclusive, in place | Pull | None | current |
| `[T]`, `Iter<T>` | `map((item) -> T)` | borrows a list; consumes an iterator | Visit | empty list or iterator | current |
| `[T]`, `Iter<T>` | `filter((item) -> T)` | borrows a list; consumes an iterator | Visit | empty list or iterator | current |
| `[T]`, `Iter<T>` | `each((item) -> T)` | borrows a list; consumes an iterator | Each | Unit | current |
| `[T]`, `Iter<T>` | `find((item) -> Bool)` | borrows a list; consumes an iterator | Decide | None | current |
| `[T]`, `Iter<T>` | `any((item) -> Bool)` | borrows a list; consumes an iterator | Decide | false | current |
| `[T]`, `Iter<T>` | `all((item) -> Bool)` | borrows a list; consumes an iterator | Decide | true | current |
| `[T]`, `Iter<T>` | `position((item) -> Bool)` | borrows a list; consumes an iterator | Decide | None | current |
| `[T]`, `Iter<T>` | `is_sorted_by((item) -> T)` | borrows a list; consumes an iterator | Decide | true | current |
| `[T]`, `Iter<T>` | `filter_map((item) -> T)` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `flat_map((item) -> T)` | borrows a list; consumes an iterator | Visit | empty list or iterator | current |
| `[T]`, `Iter<T>` | `take_while((item) -> Bool)` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `skip_while((item) -> Bool)` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `dedup_by((item) -> T)` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `chunk_while((item, item) -> Bool)` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `scan(seed, (acc, item) -> acc)` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `min_by((item) -> T)` | borrows a list; consumes an iterator | Fold | None | current |
| `[T]`, `Iter<T>` | `max_by((item) -> T)` | borrows a list; consumes an iterator | Fold | None | current |
| `[T]`, `Iter<T>` | `count_where((item) -> Bool)` | borrows a list; consumes an iterator | Fold | 0 | current |
| `[T]`, `Iter<T>` | `reduce(seed, (acc, item) -> acc)` | borrows a list; consumes an iterator | Fold | the seed | current |
| `[T]`, `Iter<T>` | `fold(seed, (acc, item) -> acc)` | borrows a list; consumes an iterator | Fold | the seed | current |
| `[T]`, `Iter<T>` | `group_by((item) -> T)` | borrows a list; consumes an iterator | Group | empty map | current |
| `[T]`, `Iter<T>` | `count_by((item) -> T)` | borrows a list; consumes an iterator | Group | empty map | current |
| `[T]`, `Iter<T>` | `partition((item) -> Bool)` | borrows a list; consumes an iterator | Group | two empty lists | current |
| `[T]`, `Iter<T>` | `counts()` | borrows a list; consumes an iterator | Group | empty map | current |
| `[T]`, `Iter<T>` | `unzip()` | borrows a list; consumes an iterator | Group | one empty list per field | current |
| `[T]`, `Iter<T>` | `take(Int)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `skip(Int)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `step_by(Int)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `repeat(Int)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `cycle(Int)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `drop_last(Int)` | borrows a list; consumes an iterator | Reorder | empty iterator | current |
| `[T]`, `Iter<T>` | `chunks(Int)` | borrows a list; consumes an iterator | Window | empty iterator | current |
| `[T]`, `Iter<T>` | `windows(Int)` | borrows a list; consumes an iterator | Window | empty iterator | current |
| `[T]`, `Iter<T>` | `dedup()` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `flatten()` | borrows a list; consumes an iterator | Adapt | empty list or iterator | current |
| `[T]`, `Iter<T>` | `indexed()` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `indexes()` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `intersperse(item)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `first()` | borrows a list; consumes an iterator | Lookup | None | current |
| `[T]`, `Iter<T>` | `last_index_of(item)` | borrows a list; consumes an iterator | Lookup | None | current |
| `[T]`, `Iter<T>` | `min()` | borrows a list; consumes an iterator | Lookup | None | current |
| `[T]`, `Iter<T>` | `max()` | borrows a list; consumes an iterator | Lookup | None | current |
| `[T]`, `Iter<T>` | `sum()` | borrows a list; consumes an iterator | Drain | 0 | current |
| `[T]`, `Iter<T>` | `product()` | borrows a list; consumes an iterator | Drain | 1 | current |
| `[T]`, `Iter<T>` | `average()` | borrows a list; consumes an iterator | Drain | 0.0 | current |
| `[T]`, `Iter<T>` | `join(String)` | borrows a list; consumes an iterator | Drain | empty string | current |
| `[T]`, `Iter<T>` | `split(Int)` | borrows a list; consumes an iterator | Build | two empty lists | current |
| `[T]`, `Iter<T>` | `is_sorted()` | borrows a list; consumes an iterator | Measure | true | current |
| `[T]`, `Iter<T>` | `compare([item])` | borrows a list; consumes an iterator | Measure | 0 against an empty list | current |
| `[T]`, `Iter<T>` | `is_empty()` | borrows a list; consumes an iterator | Measure | true | current |
| `[T]`, `Iter<T>` | `len()` | borrows a list; consumes an iterator | Measure | 0 | current |

<!-- END GENERATED COLLECTION OPERATIONS -->

## Pay for what you call

Core module names do not by themselves make a program depend on every helper.
The generated program retains reachable helpers for the calls it makes; a
program that imports many modules but calls only `print` need not retain their
unreachable implementations. This is the reachability contract behind the
Core registry.

## Modules

### `core.files` — files, paths, and handles

`core.files` combines whole-file helpers, streaming handles, path operations,
and filesystem metadata. Path-taking calls accept `String` and the checked
`Path` value. Filesystem calls carry the `FS` effect; current-directory and
environment-derived helpers also carry `Env` (D-FILES-WRITE1).

A relative path is resolved from the process working directory. An entry-local
file can be opened directly with `fs.read("input.txt")`; do not read the
working directory and join it to an argument that is already relative. Use a
`Path` when intentionally composing components.

```jet
use core.files as fs

fn run() {
    path :: "/tmp/notes.txt"
    fs.write(path, "hello\n") ?? return
    fs.append_all(path, "world\n") ?? return
    print(fs.read(path) ?? return)
    print(fs.exists(path))
    print(fs.is_dir("/tmp"))
    entries :: fs.list_dir("/tmp") ?? return
    print(entries.len())
}
```

Whole-file and metadata functions:

| API | Result | Description |
| --- | --- | --- |
| `read(path)` | `String !IOError` | Read a UTF-8 text file. Invalid UTF-8 is an `IOError`. |
| `read_bytes(path)` | `[U8] !IOError` | Read raw bytes. |
| `write(path, text)` | `!IOError` | Create or replace a text file. |
| `write_bytes(path, bytes)` | `!IOError` | Create or replace raw bytes. |
| `append_all(path, text)` | `!IOError` | Append text in one whole-file call. |
| `map(path)` | `MappedFile !IOError` | Map a file into the checked byte-view carrier. |
| `read_at(path, offset, count)` | `[U8] !IOError` | Read a bounded byte range. |
| `write_at(path, offset, bytes)` | `!IOError` | Write bytes at an offset. |
| `write_atomic(path, bytes)` | `!IOError` | Publish bytes through a temporary file and rename. |
| `fsync(path)` | `!IOError` | Flush a file to stable storage. |
| `exists(path)` / `is_dir(path)` / `is_file(path)` | `Bool` | Return false when the path cannot be observed; these helpers do not raise a missing-path error. |
| `stat(path)` | `Stat !IOError` | Read size, timestamps, mode, permissions, and file-kind facts. |
| `set_mode(path, mode)` | `!IOError` | Set Unix mode bits; non-Unix providers use their documented readonly mapping. |
| `list_dir(path)` | `[DirEntry] !IOError` | List entries in name order. |
| `create_dir(path)` / `create_dir_all(path)` | `!IOError` | Create one directory or its missing parents. |
| `remove(path)` / `remove_dir(path)` / `remove_all(path)` | `!IOError` | Remove a file, empty directory, or tree. |
| `copy(from, to)` / `copy_dir(from, to)` | `!IOError` | Copy one regular file or a directory tree. Directory copy does not dereference symlinks or special entries. |
| `rename(from, to)` | `!IOError` | Rename or move a path. |
| `symlink(original, link)` / `hard_link(original, link)` | `!IOError` | Create symbolic or hard links. |
| `read_link(path)` | `String !IOError` | Read a symbolic-link target. |
| `canonicalize(path)` / `absolute(path)` | `String !IOError` | Resolve an existing path with symlinks, or make an absolute lexical path without requiring existence. |
| `walk(root, ignore)` / `walk_parallel(root, ignore)` | `[WalkEntry] !IOError` | Traverse recursively with an optional ignore-file name. The parallel spelling keeps the same ordering and no-follow policy. |
| `walk_files(root, ignore)` | `[WalkEntry] !IOError` | Traverse regular files only. |
| `glob(pattern)` | `[String] !IOError` | Return paths matching the bounded glob pattern. |
| `temp_dir(prefix)` / `temp_file(prefix)` | `TempDir` / `TempFile !IOError` | Create temporary resources whose handles retain `.path`. |
| `lock(path)` | `FileLock !IOError` | Create an advisory lock whose handle owns its cleanup. |

`walk`, `walk_parallel`, and `walk_files` accept the optional `ignore` value;
`fs.walk(root)` is the convenience form. `DirEntry` has `name`, `path`, and
`is_dir`. `Stat` has `size`, `modified_ms`, `created_ms`, `readonly`, `is_file`,
`is_dir`, `is_symlink`, `kind`, and `mode`. `WalkEntry` exposes `path`,
`relative`, `is_dir`, and `depth` through the checked type projection.

Streaming constructors keep memory bounded:

```jet
use core.files as files

fn count_lines(path: String) -> Int IOError! {
    reader :: files.open(path)
    n := 0
    loop line in reader.lines() {
        n = n + 1
    }
    return Ok(n)
}
```

| API | Result | Description |
| --- | --- | --- |
| `open(path)` | `FileReader !IOError` | Open a buffered reader. |
| `create(path)` | `FileWriter !IOError` | Create or replace a buffered writer. |
| `append(path)` | `FileWriter !IOError` | Open a buffered appending writer. |
| `reader.read_line()` | `?String !IOError` | Read one line without its newline; return `None` at EOF. |
| `reader.lines()` in a loop source | stream of `String` | Pull lines without loading the complete file. |
| `writer.write_line(text)` | `!IOError` | Write text and a newline. |
| `writer.flush()` | `!IOError` | Flush pending writer bytes. |

Readers, writers, temporary resources, and locks own their provider resource
until lexical scope exit or an explicit `close(^handle)` consumes the handle.
A writer flush remains fallible, and early failure propagation closes the
handle. `append_all` is deliberately distinct from the streaming `append`
constructor (D-FILES-APPEND1). See the [bounded buffering law](../spec.md#bounded-buffering-law).

#### `Path` values

`Path` construction and lexical path math are pure; existence, resolution, and
metadata queries carry filesystem effects. `Path.from(value)` (the checked
constructor used by the feature examples), `Path.of`, and `Path.path` create a
portable value. `join` removes duplicate separators, and `collapse` handles
`.` and `..`; Windows drive letters count as absolute.

Use `Path.join`, `parent`, `parents`, `name`, `stem`, `suffix`, `suffixes`,
`parts`, `is_absolute`, `is_relative`, `as_posix`, `as_windows`, `collapse`,
`resolve_pure`, `as_uri`, and `is_relative_to` for composition and lexical
checks. `resolve`, `absolute`, and `expanduser` consult the environment or
filesystem. `read_text`, `read_bytes`, `read_lines`, `write_text`,
`write_bytes`, `write_lines`, `append_text`, `mkdir`, `ensure_dir`, `rmdir`,
`rmtree`, `unlink`, `rename`, `replace`, `touch`, `chmod`, `symlink_to`,
`hardlink_to`, `readlink`, `copy_file`, `copy_into`, `glob`, `rglob`,
`iterdir`, `walk`, `which`, `open_read`, and `open_write` provide the typed
convenience surface. `is_relative_to` is a lexical comparison; it does not
resolve symlinks. Use `canonicalize` when the policy requires physical,
existing-path containment.

### `core.net.url` — RFC 3986 URL values

`core.net.url` parses, constructs, joins, normalizes, and renders the `URL`
carrier. Jet owns scheme, authority, dot-segment, query, fragment, and percent
codec behavior; the parser follows RFC 3986-style rules and does not claim
WHATWG IDNA or automatic punycode processing.

```jet
use core.net.url as url

fn run() {
    base :: url.parse("https://example.test/a/./b/../c?x=1") ?? return
    next :: base.join("../notify?user=ada%20lovelace") ?? return
    print(next.to_string())
}
```

| API | Result | Description |
| --- | --- | --- |
| `parse(text)` | `URL !URLError` | Parse an absolute URL and reject invalid syntax or percent escapes. |
| `from_parts(scheme, host, path, query, fragment)` | `URL !URLError` | Construct from decoded components; query is `[[String]]`. |
| `file(path)` / `data(mime, payload)` | `URL` | Build `file:` and `data:` values. |
| `query(pairs)` / `urlencode(pairs)` | `String` | Encode repeated query pairs. |
| `percent_encode(text)` / `percent_decode(text)` | `String` / `String !URLError` | Encode or decode URL components. |
| `u.scheme` / `u.host` / `u.port` / `u.path` / `u.query` / `u.fragment` | component values | Read the parsed URL fields; `port` uses the default-port convention. |
| `u.path_segments()` / `u.query_pairs()` | `[String]` / `[[String]]` | Read decoded path and repeated query pairs. |
| `u.normalize()` / `u.join(relative)` | `URL` / `URL !URLError` | Return a normalized URL or resolve a relative reference. |
| `urljoin(base, relative)` | `String` | Join textual URL references. |
| `parse_qsl(text)` / `parse_qs(text)` | query rows | Parse repeated query values into list or grouped forms. |
| `split_fragment(text)` | `(String, String)` | Separate a textual URL from its fragment. |

`URL` is uppercase because it is the declared carrier; `URLError` has `Syntax`
and `Percent` cases. Credential accessors (`username`, `password`, `userinfo`,
and `authority`) expose decoded components and do not grant network access.

### `core.net.mime` — media types

`core.net.mime` parses `type/subtype` values, validates parameters, and maps a
small explicit extension table. Type, subtype, and parameter names are folded
to lowercase; unknown extensions are `None`, not an implicit
`application/octet-stream`.

| API | Result | Description |
| --- | --- | --- |
| `parse(text)` | `MIME !MIMEError` | Parse an essence and its `; name=value` parameters. |
| `from_extension(ext)` | `?String` | Map a common extension such as `html`, `json`, or `png` to an essence. |
| `extension(mime)` | `?String` | Map a known MIME essence back to its common extension. |
| `m.media_type()` / `m.subtype()` / `m.essence()` | `String` | Read normalized type and subtype values. |
| `m.param(name)` / `m.params()` | `?String` / `[[String]]` | Inspect validated parameters through the typed projection. |

`MIMEError` has the `Syntax` case. MIME detection is explicit: static-file
callers choose a type or ask the extension table rather than relying on
sniffing.

### `core.crypto.uuid` — UUID strings (D-UUIDENC1=A)

UUIDs remain plain `String` values. `v4` and `v7` require the fail-closed
random provider; `parse` validates and lowercases the canonical form; `v5`
and `uuid5` derive a deterministic name-based value.

```jet
use core.crypto.uuid as uuid

fn run() {
    id :: uuid.v4() ?? return
    normalized :: uuid.parse("6BA7B810-9DAD-11D1-80B4-00C04FD430C8") ?? return
    same_every_time :: uuid.v5("6ba7b810-9dad-11d1-80b4-00c04fd430c8", "python.org") ?? return
    print("{id} {normalized} {same_every_time}")
}
```

| API | Result | Description |
| --- | --- | --- |
| `v4()` | `String !UUIDError` | Generate a random UUID with the OS CSPRNG. |
| `v7(clock)` | `String !UUIDError` | Generate a time-ordered UUID from an injected `Clock`; random tail bytes still use the random provider. |
| `parse(text)` | `String !UUIDError` | Validate `8-4-4-4-12` hexadecimal form and return lowercase text. |
| `v5(namespace, name)` / `uuid5(namespace, name)` | `String !UUIDError` | Derive the RFC name-based UUID from a valid namespace and name. |

`uuid1` and an unqualified `uuid4` spelling are not Core APIs; use `v7` or
`v4` respectively.

### `core.email` — bounded messages and SMTP submission

`core.email` separates checked address/message construction from SMTP
transport. It rejects controls before serialization and bounds recipients,
attachments, headers, body, and total message size before producing wire bytes.

```jet
use core.email as email

fn run() {
    from :: email.address("Mara <mara@example.com>") ?? return
    to :: email.address("Ada <ada@example.net>") ?? return
    message :: email.message(from, [to], [Address]{}, "Welcome", "Hello", HTML{""}, [Attachment]{}) ?? return
    bytes :: email.serialize(message) ?? return
    print(bytes.len())
}
```

`HTML{...}` is checked text; interpolation values are escaped. Use `HTML{""}`
for a plain-text-only message. Bcc recipients enter the default `Envelope` but
are not emitted as message headers. Serialization uses CRLF, bounded Base64
lines, and deterministic content-derived multipart boundaries.

| API | Result | Description |
| --- | --- | --- |
| `limits()` | `Limits` | Return the checked reply, capability, recipient, message, and challenge limits. |
| `smtp_auth(user, password)` | `SMTPAuth` | Build password authentication from a nominal `Secret`. |
| `dkim(domain, selector, key, signed_headers)` | `DkimConfig` | Configure one signing identity and its signed-header list. |
| `address(text)` | `Address !EmailError` | Parse and validate a mailbox or display-name address. |
| `attachment(name, mime, bytes)` | `Attachment !EmailError` | Validate an attachment name/type/size and normalize its MIME spelling. |
| `message(from, to, bcc, subject, text, html, attachments)` | `Message !EmailError` | Validate fields, create the default envelope, and compute a wire bound. |
| `envelope(from, recipients)` | `Envelope !EmailError` | Validate SMTP recipients independently of message headers. |
| `serialize(message)` | `[U8] !EmailError` | Render bounded MIME bytes without Bcc headers. |
| `smtp(config)` | `Mailer !EmailError` | Validate configuration and create a transport handle. |
| `smtp_from_env()` | `Mailer !EmailError` | Read the checked SMTP configuration from environment variables. |

`smtp_from_env` requires `SMTP_HOST`. `SMTP_SECURITY` accepts `starttls` (the
default) or `tls`; the corresponding default ports are 587 and 465.
`SMTP_PORT`, `SMTP_CA_PEM`, `SMTP_RECIPIENT_POLICY`, and paired
`SMTP_USERNAME`/`SMTP_PASSWORD` are optional. Recipient policy is
`require_all` by default or `deliver_accepted`. DKIM configuration uses
`SMTP_DKIM_DOMAIN`, `SMTP_DKIM_SELECTOR`, `SMTP_DKIM_PRIVATE_KEY_BASE64`, and
optional `SMTP_DKIM_SIGNED_HEADERS`; partial configuration fails before
connecting.

`Mailer.send(message)` reports relay acceptance in `SendReport`, not inbox
delivery. A cancellation after message submission is `DeliveryUnknown`; Core
does not retry an uncertain submission. Use separate mailers for separate DKIM
identities. Custom CA material extends system roots without disabling hostname
verification.

### `core.http` — bounded HTTP values and one-shot calls

`core.http` owns bounded HTTP/1.1 message semantics and one-shot request helpers;
`core.http.client` supplies configuration and session policy, while
`core.http.server` supplies serving. Body reads and writes follow the [bounded
buffering law](../spec.md#bounded-buffering-law). The checked implementation
rejects malformed methods, headers, framing, URLs, and bodies before a provider
call.

The source-facing carriers include `HTTPError`, `Method`, `Version`, `Status`,
`Header`, `Headers`, and `Body`. The default limits are one MiB for a body,
32 KiB for headers, 8 KiB for a header line and request line, 100 headers, and
8 KiB for a URL.

| API | Result | Description |
| --- | --- | --- |
| `method_parse(token)` / `method_text(method)` | `Method` / `String` | Parse or render standard and custom methods. |
| `version_parse(token)` / `version_text(version)` | `Version` / `String` | Parse or render HTTP/1.0 through HTTP/3 labels. |
| `headers()` / `headers_get(h, name)` | `Headers` / `?String` | Create headers or read a case-insensitive first value. |
| `headers_all(h, name)` / `headers_set` / `headers_append` / `headers_remove` | header values | Preserve ordered fields while replacing, appending, or removing names. |
| `body_empty()` / `body_text(text)` / `body_bytes(bytes)` | `Body` | Construct the checked empty, text, or byte body variants. |
| `get(url)` / `post(url, body)` | `HTTPResponse !HTTPError` | Perform one-shot GET or POST calls with URL and body checks. |
| `put(url, body)` / `patch(url, body)` | `HTTPResponse !HTTPError` | Perform one-shot PUT or PATCH calls. |
| `delete(url)` / `head(url)` | `HTTPResponse !HTTPError` | Perform one-shot DELETE or HEAD calls. |
| `exchange(method, url, body, header_lines)` | `HTTPResponse !HTTPError` | Validate and perform a request described by a method and header lines. |
| `parse_request(raw)` / `parse_response(url, raw)` | request/response `!HTTPError` | Parse bounded CRLF-framed messages and reject invalid framing or encoding. |
| `query()` / `query_get` / `query_all` / `query_set` | query values | Build and inspect repeated query pairs. |
| `cookie(name, value)` / `cookie_encode` / `cookies_parse` | cookie values | Construct, render, and parse bounded cookie fields. |
| `basic_auth(user, password)` / `bearer_auth(token)` | `String` | Render authorization header values. |

The one-shot helpers take URL text. Typed `URL` values are accepted at the
checked call sites whose signature admits the URL carrier; converting at the
boundary keeps URL validation in `core.net.url`.

### `core.http.client` — configured client

`core.http.client` adds a request builder, a bounded retry/redirect session,
headers, cookies, authentication, proxy, and timeout policy to the one-shot
surface. A session permits at most ten redirects and one retry; its default
retry count is zero. Redirect logic strips credentials on a cross-origin hop
and refuses HTTPS-to-HTTP downgrades unless the explicit client policy permits
one.

| API | Result | Description |
| --- | --- | --- |
| `get(url)` / `post(url, body)` | `HTTPResponse !HTTPError` | Configured-module spellings of one-shot calls. |
| `client.request(method, url)` | `HTTPRequest` | Start a checked request builder. |
| `send(req)` | `HTTPResponse !HTTPError` | Send a request builder. |
| `header(client, line)` / `set_header(client, name, value)` | `Client` | Add or replace a validated header in a client carrier. |
| `with_proxy(client, proxy)` / `timeout_redirects(client, n)` | `Client` | Set proxy or redirect count on a client carrier. |
| `session()` | `Session` | Create the default session policy. |
| `session_header` / `session_cookie` | `Session` | Add validated request headers or cookies. |
| `session_timeout` / `session_retries` / `session_redirects` | `Session` | Set bounded timeout, retry, or redirect policy. |
| `session_auth` / `session_proxy` | `Session` | Set credentials or an explicit proxy URL. |
| `session_request(session, method, url, body)` | `HTTPResponse !HTTPError` | Apply session headers, retries, redirect checks, and transport. |
| `Client.new()` | `HTTPClient` | Construct the typed client policy carrier; unset policies use the safe defaults. |
| `Client.new().proxy(policy)` | `HTTPClient` | Select `.FromEnvironment` (default), `.None`, or `.Url(proxy)`. |
| `Client.new().tls(config)` | `HTTPClient` | Apply a `core.net.tls.ClientConfig`; custom roots, mTLS identity, and TLS 1.2/1.3 bounds are enforced on HTTPS sends. |
| `Client.new().cookies(.Memory)` | `HTTPClient` | Enable one clone-shared, bounded RFC6265bis memory cookie jar; one-shot shortcuts remain stateless. |
| `Client.new().redirects(.Follow{ max:, same_origin_credentials: })` | `HTTPClient` | Follow at most the bounded limit, strip credentials across origins, and preserve same-origin credentials only when requested. |
| `Client.new().allow_http_downgrade(true)` | `HTTPClient` | Explicitly permit HTTPS-to-HTTP redirects; the default refuses downgrades. |
| `Client.new().retries(.Safe/.Idempotent/.None)` | `HTTPClient` | Retry one stale pooled-connection I/O failure for safe methods, optionally idempotent methods, or never; never retry status or timeout failures. |
| `Client.new().protocols(false, true, false)` | `HTTPClient` | Select the enabled HTTP/2, HTTP/1.1, and HTTP/3 protocol families before transport. |
| `Client.new().timeouts(connect, read, total, dns, tls, write, first_byte)` | `HTTPClient` | Set nonnegative per-phase and total timeout budgets; request overrides and ambient deadlines remain upper bounds. |
| `Client.new().raw_encoding()` | `HTTPClient` | Preserve raw response content-encoding metadata for the response projection. |

Request and response body handles are single-use. Text projection rejects
non-UTF-8 data and uses the shared body limit unless the caller selects an
explicit bounded byte/text read.

### `core.http.server` — HTTP serving

`core.http.server` builds a typed multiplexer and serves HTTP/1.1. `bind` and
`serve` accept optional `HTTPServerTls` and optional deadlines; `serve_once`
and `serve_once_listener` provide testable one-request entry points.

| API | Result | Description |
| --- | --- | --- |
| `mux()` | `HTTPMux` | Create a router carrier. |
| `bind(addr, mux, tls, deadline)` | `HTTPServer !HTTPError` | Bind a plaintext or explicitly configured TLS listener. |
| `serve(addr, mux, tls, deadline)` | `!HTTPError` | Serve requests until the operation fails or is stopped. |
| `serve_once(addr, mux)` / `serve_once_listener(listener, mux)` | `!HTTPError` | Serve one request for tests and small adapters. |
| `response(status, body)` | `HTTPResponse` | Construct a text response. |
| `json(status, body)` | `HTTPResponse` | Encode an `Encode` value as JSON response data. |
| `static_file(path, content_type)` / `static_file_range(request, path, content_type)` | `HTTPResponse !HTTPError` | Serve a file, with a range-aware form for a request. |
| `static_files(mux, prefix, root)` | unit | Mount a directory below a route prefix. |
| `tls(cert, key)` | `HTTPServerTls` | Build explicit server TLS material. |
| `sse(body)` | `HTTPResponse` | Build a server-sent-events response. |
| `cors_policy(origins)` / `cors(mux, policy)` | policy/unit | Validate a CORS origin policy and install it. |
| `access_log(request, status)` | `String` | Render a stable access-log line. |
| `request_id(mux)` | unit | Install request-id middleware. |

### `core.net.ws` — WebSocket transport

`core.net.ws` is the WebSocket home and accepts an HTTP request for server-side
upgrade. The client entry point accepts `ws://` only; it rejects credentials,
fragments, missing hosts, and other schemes before the provider handshake.

| API | Result | Description |
| --- | --- | --- |
| `connect(url)` | `WsConn !WsError` | Validate and dial a cleartext WebSocket URL. |
| `upgrade(request)` | `WsConn !WsError` | Validate GET, Upgrade, Connection, version 13, and key headers before upgrading. |
| `conn.send_text(text)` / `conn.send_bytes(bytes)` | `!WsError` | Send a text or binary frame. |
| `conn.recv()` | `WsMessage !WsError` | Receive a text, binary, or close message. |
| `conn.close(code, reason)` | `!WsError` | Send a close frame and shut down the connection. |

`WsError` distinguishes invalid URLs, invalid handshakes, protocol/size
failures, timeout, cancellation, closed connections, and unsupported targets.

### `core.web.browser` — WebDriver BiDi automation

`core.web.browser` is the portable browser automation home. It speaks
versioned WebDriver BiDi over `core.net.ws`, with a capability-checked CDP
expert path. Profiles are isolated by default, and receipts and traces keep
only redacted audit facts.

| API | Result | Description |
| --- | --- | --- |
| `config()` / `config_from_env()` | `BrowserTestConfig !BrowserError` | Load checked browser-test configuration. |
| `make_profile(name)` / `timeout(ms)` | `BrowserProfile` / `BrowserTimeout !BrowserError` | Construct an isolated profile or a checked timeout. |
| `locked(engine)` | `BrowserLocked !BrowserError` | Read and verify the provider's locked browser record. |
| `connect()` / `connect_browser_profile(profile, timeout)` | `Browser !BrowserError` | Connect through the default or named profile. |
| `browser.context()` | `BrowserContext !BrowserError` | Open an isolated browsing context. |
| `context.page()` / `context.tab()` | `BrowserPage` | Open a page or tab in a context. |
| `page.goto(url)` | `BrowserPage !BrowserError` | Navigate a page under its timeout and protocol policy. |
| `page.main_frame()` / `page.frames()` | `BrowserFrame` / `[BrowserFrame]` | Inspect the main frame or child frames. |
| `page.close()` / `context.close()` / `browser.close()` | `!BrowserError` | Explicitly close page, context, or browser resources. |
| `page.get_by_role(role, name)` / `get_by_text(text)` | `BrowserLocator` | Locate an element through semantic role or visible text. |
| `page.get_by_label(label)` / `get_by_placeholder(text)` / `get_by_test_id(id)` | `BrowserLocator` | Locate labeled, placeholder, or test-id controls. |
| `page.get_by_css(selector)` / `selected(page, selector)` | `BrowserLocator` | Use an explicit CSS locator when semantic selection is not suitable. |
| `locator.wait(timeout)` / `locator.wait_gone(timeout)` | `!BrowserError` | Wait for a locator to appear or disappear. |
| `locator.click()` / `locator.hover()` | `!BrowserError` | Perform checked pointer actions. |
| `locator.fill(text)` / `locator.press(key)` | `!BrowserError` | Fill a control or send a key. |
| `browser.subscribe(kind)` / `browser.next_event(timeout)` | `BrowserEvent` | Subscribe to and receive bounded, redacted browser events. |
| `browser.add_intercept(pattern)` / `add_intercept_url(pattern, url)` | `BrowserIntercept` | Install a network interception rule. |
| `browser.continue_request(id)` / `fail_request(id)` / `fulfill_request(id, status, body)` | `!BrowserError` | Continue, fail, or fulfill an intercepted request. |
| `page.set_cookie(name, value, options)` / `page.cookie(name)` | cookie value | Set or read a page-partition cookie. |
| `page.clear_cookies()` | `!BrowserError` | Clear cookies in the page partition. |
| `page.storage_get(kind, key)` / `storage_set(kind, key, value)` | `String` / `!BrowserError` | Read or write local or session storage. |
| `page.storage_clear(kind)` | `!BrowserError` | Clear a local or session storage partition. |
| `locator.set_files(path)` | `!BrowserError` | Set files on a file-upload control. |
| `page.screenshot()` / `page.pdf()` | `[U8] !BrowserError` | Capture a bounded screenshot or PDF artifact. |
| `browser.protocol("bidi" \| "cdp")` / `protocol.send(method, params)` | `BrowserProtocol` / response | Access the selected protocol; CDP requires the advertised capability. |
| `browser.privacy()` | `BrowserPrivacy` | Report isolated-profile, receipt-redaction, and shared-profile policy facts. |
| `browser.receipt()` / `browser.trace()` | `BrowserReceipt` / `BrowserTrace` | Return redacted audit facts and the bounded action trace. |
| `begin_named(...)` / `fixture_context(browser)` / `fixture_page(context, url)` | fixture values | Build named test fixtures and their contexts/pages. |
| `fixture_source(name, body)` / `generate_source(name, body, base_url)` | `String !BrowserError` | Generate bounded fixture source with escaped base-URL data. |
| `report_new(title)` / `report_add_case(report, name, ok)` | `BrowserReport` | Build an ordered browser report. |
| `report_text(report)` / `report_json(report)` / `report_html(report)` | `String` | Render the report in text, JSON, or HTML. |
| `report_exit_code(report)` / `write_report(report, path)` | `Int` / `!IOError` | Produce the test exit code or write the text report. |
| `server_start(root)` / `server_stop(server)` | `BrowserServer` | Start or stop a provider-backed test server. |
| `watch_changed(path)` | `Bool !BrowserError` | Ask the provider whether the watched path changed. |

The source generator rejects empty names and values over its fixed source
limits; HTML/JSON report renderers escape their output contexts. Provider
connection, headless, protocol, timeout, closed-state, and cleanup failures
are `BrowserError` values.

### `core.crypto` — nominal cryptography

`core.crypto` is the typed cryptography rung (D-ONCE-LAYER1=B). Hashes,
constant-time comparisons, recipient envelopes, signatures, key agreement, and
password hashes use nominal carriers for secret-bearing values. Missing entropy
is an error; the API does not fall back to a weak generator.

```jet
use core.crypto as crypto

fn run() {
    recipient :: crypto.X25519SecretKey.new_random() ?? return
    msg :: "hello, jet".bytes()
    box :: crypto.seal([recipient.public_key()], msg, []) ?? return
    plain :: crypto.open(recipient, box, []) ?? return
    print(plain)
}
```

| API | Result | Description |
| --- | --- | --- |
| `sha256(bytes)` / `sha512(bytes)` / `blake3(bytes)` | `Digest256` / `Digest512` | Produce typed digests; use `.hex()` or `.as_bytes()`. |
| `sha1(bytes)` / `sha224(bytes)` / `sha384(bytes)` | `String` | Produce legacy SHA hex digests; SHA-1 remains for UUID v5 and legacy checks. |
| `sha3_224` / `sha3_256` / `sha3_384` / `sha3_512` | `String` | Produce SHA-3 hex digests. |
| `hmac_sha256(key, data)` | `[U8]` | Produce HMAC-SHA256 bytes. |
| `hkdf_sha256(ikm, salt, info, len)` | `Secret !CryptoError` | Derive a bounded 0–8160-byte nominal secret. |
| `pbkdf2_hmac(password, salt, iterations, key_len)` | `[U8] !CryptoError` | Derive a bounded PBKDF2-HMAC-SHA256 byte sequence. |
| `new()` / `update(hasher, bytes)` / `digest(hasher)` | hasher operations | Use the incremental SHA-256 hasher. |
| `constant_time_equal(a, b)` / `constant_time_equal_bytes(a, b)` | `Bool` | Compare nominal secrets or byte lists without early equality branching. |
| `generatekey()` | `X25519SecretKey !CryptoError` | Generate an X25519 secret through the random provider. |
| `x25519(secret, public)` / `x25519_shared(secret, public)` | `SharedSecret !CryptoError` | Perform typed X25519 agreement and reject invalid keys. |
| `x25519_public(secret)` | `X25519PublicKey !CryptoError` | Derive the public key. |
| `seal(recipients, bytes, aad)` / `open(identity, box, aad)` | `Sealed !CryptoError` / `[U8] !CryptoError` | Use the recipient envelope with internal key and nonce handling. |
| `file_seal(recipients, source, destination)` / `file_open(identity, source, destination)` | `Bool !FileCryptoError` | Seal or open bounded authenticated files with atomic publication. |
| `sign(signing_key, bytes)` / `verify(verify_key, bytes, signature)` | `Signature !CryptoError` / `Bool !CryptoError` | Use nominal Ed25519 signing and verification. |
| `wrap(secret, recipient)` / `unwrap(identity, wrapped)` | `WrappedKey !CryptoError` / `Secret !CryptoError` | Wrap a nominal Secret for an X25519 recipient. |
| `password_hash(password)` / `password_verify(password, stored)` | `PasswordHash !CryptoError` / `Bool !CryptoError` | Hash and verify a nominal Secret with the Core password policy. |

`Sealed`, `Signature`, `Digest256`, and `Digest512` expose byte projection
methods where the declared API permits it; secret-bearing types must not be
logged. `file_seal` and `file_open` take typed `Path` values at their checked
boundary and use `FS` plus `Rand` where indicated.

The RSA-shaped `new`, `generate_key`, `private_encrypt`, `private_decrypt`,
`public_encrypt`, and `public_decrypt` names are not Core APIs. X25519 sealed
boxes are the Core public-key mechanism; other algorithms belong in a regular
Jet package or the explicitly audited expert surface.

### `core.crypto.expert` — audited raw primitives

`core.crypto.expert` is the raw-byte rung. Calls that select an algorithm,
nonce, or key representation directly belong inside an audited `#Unsafe` block;
the safe `core.crypto` functions remain the default.

| API | Result | Description |
| --- | --- | --- |
| `x25519_raw(secret_bytes, public_bytes)` | `Secret !CryptoError` | Perform raw X25519 and reject a non-contributory all-zero result. |
| `hkdf_sha256_raw(ikm, salt, info, len)` | `Secret !CryptoError` | Derive raw-byte HKDF output under the same length bound. |
| `argon2id(password, salt, memory_kib, iterations, lanes, output_len)` | `Secret !CryptoError` | Run bounded Argon2id with caller-selected parameters. |
| `ed25519_sign(secret, bytes)` / `ed25519_verify_strict(public, message, signature)` | signature/bool `!CryptoError` | Use raw Ed25519 material with explicit length checks. |
| `aes256gcm_seal/open` | `[U8] !CryptoError` | Use explicit AES-256-GCM key, nonce, plaintext, and AAD bytes. |
| `xchacha20poly1305_seal/open` | `[U8] !CryptoError` | Use explicit XChaCha20-Poly1305 key, nonce, plaintext, and AAD bytes. |
| `open_v1(key, envelope)` | `[U8] !CryptoError` | Read a canonical historical JETC v1 envelope only from `#Unsafe`. |
| `migrate_v1(key, source, recipients, destination)` | `Bool !FileCryptoError` | Migrate a historical file to recipient JETC v2 after re-open verification. |
| `secret_bytes(secret)` / `shared_secret_bytes(secret)` | `[U8]` | Project raw bytes only at an audited boundary. |

Raw callers own protocol selection, byte validation, and the `#Unsafe` audit;
raw X25519 still rejects non-contributory peers. See
[`crypto_suite.jet`](../../../Examples/features/crypto/crypto_suite.jet) and
[`crypto_envelope.jet`](../../../Examples/features/crypto/crypto_envelope.jet).

### `core.crypto.random` — operating-system cryptographic randomness

`core.crypto.random` is a fail-closed operating-system CSPRNG. It is distinct
from deterministic, seedable `core.math.random`: a missing or rejected entropy
provider returns `CryptoError.Unavailable`, and the API never falls back to a
pseudorandom generator.

| API | Result | Description |
| --- | --- | --- |
| `bytes(n)` | `[U8] !CryptoError` | Request `n` bytes from the OS CSPRNG; `n` must be in `0..=1,048,576`. |
| `u32()` / `u64()` | `U32` / `U64 !CryptoError` | Draw fixed-width unsigned integers from CSPRNG bytes. |
| `int_range(lo, hi)` / `randbelow(n)` | `Int !CryptoError` | Draw an unbiased integer from a checked half-open range; invalid bounds return `CryptoError.Length`. |
| `randbits(k)` | `Int !CryptoError` | Draw up to the bounded bit count, masking unused high bits. |
| `token_bytes(n)` | `[U8] !CryptoError` | Alias for a bounded CSPRNG byte request. |
| `token_hex(n)` / `token_urlsafe(n)` | `String !CryptoError` | Encode random bytes as hexadecimal or URL-safe token text. |
| `choice(items)` / `choice_int(items)` | `String` / `Int !CryptoError` | Choose one item without modulo bias; an empty list returns `CryptoError.Length`. |
| `shuffle_ints(items)` | `[Int] !CryptoError` | Return a CSPRNG Fisher-Yates shuffle of integer values. |
| `compare_digest(a, b)` | `Bool` | Compare byte lists in constant time. |

Lengths outside the byte or bit bounds fail with `CryptoError.Length`;
provider failure and a bounded rejection-sampling exhaustion fail with
`CryptoError.Unavailable`. These operations carry the `Rand` effect.

### `core.crypto.vault` — typed repository keys

`core.crypto.vault` keeps the untyped `get(name) -> ?String` repository-secret
lookup separate from typed `KeyRef<T>` generations. Every vault operation
carries the `Secret` effect. A `KeyRef<T>` contains name/version identity, not
key bytes; the provider owns persistence and key-material lifetimes.

```jet
use core.crypto as crypto
use core.crypto.vault as vault

fn provision() -[Secret, IO]> {
    plan :: vault.prepare_generate<crypto.SigningKey>("release") ?? return
    write :: vault.authorize_write(&plan, "create release signer") ?? return
    key_ref :: vault.commit_generate(^write, ^plan) ?? return
    print("created {key_ref}")
}
```

| API | Result | Description |
| --- | --- | --- |
| `get(name)` | `?String` | Read the separate untyped repository-secret value. |
| `current<T>(name)` | `?KeyRef<T> !VaultError` | Return the active generation, if one exists. |
| `versions<T>(name)` | `[KeyRef<T>] !VaultError` | Return generations in provider order. |
| `load<T>(&key)` / `status<T>(&key)` | `T !VaultError` / `KeyStatus !VaultError` | Load an exact non-revoked key or inspect `Current`, `Retired`, or `Revoked`. |
| `prepare_generate<T>(name)` / `prepare_store<T>(name, ^key)` | `MutationPlan<T> !VaultError` | Prepare a typed mutation without publishing it. |
| `prepare_import_signing` / `prepare_import_x25519` | typed plan `!VaultError` | Prepare a checked 32-byte signing or X25519 secret import. |
| `prepare_rotate<T>(name)` | `MutationPlan<T> !VaultError` | Prepare a rotation for a named generation. |
| `prepare_retire<T>(&key, reason)` / `prepare_revoke<T>(&key, reason)` | `MutationPlan<T> !VaultError` | Bind a retirement or revocation to an exact key and reason. |
| `authorize_write<T>(&plan, reason)` | `VaultWrite<T> !VaultError` | Request one provider-authorized write for the exact plan. |
| `commit_generate/store/rotate/retire/revoke<T>(^write, ^plan)` | typed result `!VaultError` | Consume the write and plan in an atomic mutation. |
| `export_to_recipients<T>(&key, recipients)` | `WrappedVaultKey !KeyWrapError` | Export a typed key for one to sixteen distinct X25519 recipients. |
| `export_to_passphrase<T>(&key, &passphrase)` | `WrappedVaultKey !KeyWrapError` | Export a typed key under a bounded nominal Secret. |
| `prepare_import_wrapped<T>` / `authorize_wrapped_import<T>` / `commit_import_wrapped<T>` | typed results | Bind, authorize, and commit a wrapped-key import. |

Mutation plans, writes, and wrapped-import plans are one-use typed carriers.
Provider authorization is required for every write; source, workspace settings,
environment variables, DAP, and stdin do not silently authorize it. The
persistent format authenticates type and origin, and failures redact paths,
identities, recipients, backend text, and key bytes. Revocation is local
bearer-copy state: an already exported envelope cannot be remotely erased.

`ExpiringSecret<T>` is the lifetime wrapper for `Secret`, `SigningKey`, and
`X25519SecretKey`. Construct it with an injected `Clock` and a `Duration`; use
`.with` for a non-escaping read loan. Expiry returns `Expired`, and the wrapper
destroys the owned credential through its zeroizing drop path. A system clock
adds the `Time` effect. See
[`vault_keys.jet`](../../../Examples/features/crypto/vault_keys.jet) and
[`expiring_secret.jet`](../../../Examples/features/memory/expiring_secret.jet).

### `core.auth` — token verification and sessions

`core.auth` verifies the closed JWT and PASETO forms and provides password,
OAuth, magic-link, and cookie-session helpers. The Jet layer performs strict
claim and input validation; provider calls handle the database/session boundary.

| API | Result | Description |
| --- | --- | --- |
| `verify_jwt(token, key, audience, issuer, clock_skew)` | `Claims !AuthError` | Verify a three-part HS256 JWT with exact integer `exp`, optional `nbf`/`iat`, and audience/issuer checks. |
| `verify_paseto(token, key, audience)` | `Claims !AuthError` | Verify `v4.public` with a 32-byte Ed25519 public key and the supplied audience. |
| `register_user(name, password)` | `Session !AuthError` | Register a validated identity and issue a session. |
| `password_login(name, password, ttl_s, flags)` | `Session !AuthError` | Validate credentials and issue a bounded session lifetime. |
| `oauth_begin(provider)` / `oauth_finish(state, assertion, ttl_s, flags)` | state/session `!AuthError` | Start and finish the provider-backed OAuth assertion flow. |
| `magic_link_issue(email, ttl_s, flags)` / `magic_link_consume(token, ttl_s, flags)` | token/session `!AuthError` | Issue and consume an email token with a bounded lifetime. |
| `session_validate(cookie, ttl_s)` | `Session !AuthError` | Validate the signed session cookie and return its session carrier. |
| `session_user(session)` / `session_cookie(session)` / `session_id(session)` | `String` | Project the validated session fields. |
| `session_show(session)` | `String` | Render a redacted session summary; cookie bytes are not printed. |

JWT accepts only HS256 and requires a key of at least 32 bytes. PASETO accepts
only `v4.public`, a 32-byte key, a non-empty normalized audience, and canonical
base64url segments. Unknown algorithms, malformed JSON, wrong signatures,
expired or not-yet-valid claims, wrong audiences, and invalid inputs fail
closed. `Claims` contains `issuer`, `audience`, `subject`, `expires_at`, and the
validated payload JSON. `AuthError` has `Rejected`, `Expired`, `Malformed`, and
`Unavailable` cases.

Session cookies use the fixed `jet_session=` form with `HttpOnly`, `Secure`,
`SameSite=Lax`, and `Path=/` attributes. TTL and flags are checked before the
provider call; the current helper accepts zero flags. See
[`auth_tokens.jet`](../../../Examples/features/crypto/auth_tokens.jet) and
[`auth_sessions.jet`](../../../Examples/features/crypto/auth_sessions.jet).

### `core.sync` — state-based CRDT carriers

`core.sync` exposes String-valued counters, maps, lists, and text plus a closed
allow/deny `RowPolicy`. The carrier fields are public for serialization, but
mutation functions enforce printable-text and size bounds rather than silently
truncating invalid data.

| API | Result | Description |
| --- | --- | --- |
| `counter_new()` / `counter_for(replica)` | `SyncCounter` | Create a counter with a local or named replica. |
| `counter_inc(counter)` / `counter_value(counter)` | `SyncCounter` / `Int` | Increment the replica's bounded count or sum all counts. |
| `counter_merge(left, right)` | `SyncCounter` | Merge per-replica counts by maximum. |
| `map_new()` / `map_for(replica)` | `SyncMap` | Create a String-keyed LWW map. |
| `map_set(map, key, value)` / `map_delete(map, key)` | `SyncMap` | Write or tombstone a bounded entry. |
| `map_get(map, key)` / `map_keys(map)` / `map_contains(map, key)` | optional/key list/bool | Read live entries; deleted entries are absent. |
| `map_merge(left, right)` | `SyncMap` | Resolve entries by timestamp, replica, deletion, then value tie-breaks. |
| `list_new()` / `list_for(replica)` | `SyncList` | Create an RGA-style id/value/tombstone list. |
| `list_push(list, value)` / `list_remove(list, id)` | `SyncList` | Add an item or mark an item removed. |
| `list_contains(list, id)` / `list_merge(left, right)` | `Bool` / `SyncList` | Inspect live ids or merge deterministic item/tombstone state. |
| `list_show(list)` | `String` | Render live list values in merged order. |
| `text_new()` / `text_for(replica)` | `SyncText` | Create a last-writer-wins text value. |
| `text_set(text, value)` / `text_edit(text, value)` / `text_append(text, suffix)` | `SyncText` | Write, alias-write, or append within the one-MiB text bound. |
| `text_merge(left, right)` | `SyncText` | Choose the larger timestamp, then replica name on ties. |
| `text_show(text)` / `text_metadata(text)` | `String` | Project text or the `replica@timestamp` metadata. |
| `policy_new()` / `policy_grant` / `policy_deny` / `policy_revoke` | `RowPolicy` | Build a closed allow/deny action policy. |
| `policy_allows(policy, action)` / `policy_show(policy)` | `Bool` / `String` | Apply deny-first wildcard rules or render counts. |

A malformed carrier remains unchanged on mutation or merge. Counters cap
replica entries and counts; maps and lists cap entries; text and map values
reject control bytes and exceedance. The map tie-break order makes equal
replica clocks deterministic, and list merges retain tombstones so removed
items do not reappear. This module is not an authenticated remote transport or
a general thread-lock API; use the typed transport and task modules for those
contracts. The thread-lock ledger names (`broadcast`, `clear`, `lock`, `rlock`,
`signal`, `trylock`, `unlock`, `wait`, `thread`, `timer`, and `locked`) remain
declined under D-CORESURF-SMALL1.

### `core.watcher` — file, process, and port watches

`core.watcher` creates explicit watch handles for files, process IDs, and TCP
ports (D-WATCH-SCOPE1). Polling is explicit: a handle or set does not own a
background thread or shell process, and the public module has no callback
registration method.

| API | Result | Description |
| --- | --- | --- |
| `files(path)` / `recursive(path)` | `WatchHandle !IOError` | Watch a path; the recursive spelling uses the host's recursive file snapshot. |
| `process_pid(pid)` | `WatchHandle` | Watch process liveness. |
| `port(host, port)` | `WatchHandle` | Watch TCP readiness. |
| `set()` / `add(set, handle)` | `WatchSet` | Create a set or add a handle. |
| `poll(handle)` / `events(handle)` | `[WatchEvent]` | Poll and drain observations from a handle. |
| `drain(set)` | `[WatchEvent]` | Poll every handle in a set. |
| `cancel(handle)` / `is_active(handle)` | unit/bool | Stop or inspect a handle. |
| `summary(handle)` / `kind(handle)` / `target(handle)` | `String` | Inspect a stable handle summary and its parsed kind/target. |
| `len(set)` | `Int` | Count handles in a set. |
| `remove(set, target)` / `contains(set, target)` | `!IOError` / `Bool !IOError` | Return the explicit unsupported-operation error for non-empty set operations; an empty `contains` target returns false. |
| `debounce(handle, ms)` | `WatchHandle !IOError` | Reject a negative duration and otherwise return the provider's unsupported-operation error. |

`WatchDomain` is `File`, `Process`, or `Port`. `WatchEventKind` is `Created`,
`Modified`, `Removed`, `Error`, `Exited`, or `Ready`. `WatchEvent` carries
`domain`, `kind`, `path`, `detail`, `pid`, and `port`. Match all variants when
handling a closed event enum. See
[`watcher.jet`](../../../Examples/features/io/watcher.jet).

### `core.term` — terminal input and output

`core.term` owns UTF-8 stdio, prompts, raw-key input, terminal size, and ANSI
style. Terminal stream operations follow the [bounded buffering law](../spec.md#bounded-buffering-law). The
qualified `term.print` takes one `String`; the prelude `print` remains the
convenient general printing form.

```jet
use core.term as term

fn run() {
    name :: term.input("your name? ") ?? return
    print("hi, {name}")
    term.eprint("(log) done")
    out :: term.stdout()
    out.write("done") ?? return
    &out.flush() ?? return
}
```

| API | Result | Description |
| --- | --- | --- |
| `input(prompt)` | `String !IOError` | Read one line, stripping its newline; the prompt defaults to empty. |
| `readline()` | `String !IOError` | Read one line without a prompt. |
| `read_until(delimiter)` | `String !IOError` | Read through a non-empty delimiter, excluding the delimiter. |
| `read_all_input()` | `String !IOError` | Read stdin to EOF. |
| `input_secret(prompt)` | `String !IOError` | Read without echo; redirected/non-terminal input is an error rather than an echoed fallback. |
| `take(n)` | `[U8] !IOError` | Read up to `n` raw bytes from stdin. |
| `confirm(prompt)` / `choose(prompt, options)` | `Bool` / `String !IOError` | Ask a bounded yes/no or choice question. |
| `stdin()` / `buffered()` | `StdinHandle` | Return the buffered stdin handle; `buffered` is its alias. |
| `stdout()` / `stderr()` | `Stdout` / `Stderr` | Return output stream handles. |
| `stream.write(text)` / `stream.write_line(text)` | `!IOError` | Write text with or without a newline. |
| `stream.write_bytes(bytes)` / `stream.flush()` | `!IOError` | Write raw bytes or flush a stream. |
| `binread(path)` / `binwrite(path, bytes)` | `[U8] !IOError` / `!IOError` | Read or atomically write raw file bytes. |
| `terminal_width()` / `terminal_height()` | `Int` | Query terminal dimensions with the provider fallback. |
| `style(style, text)` / `style_force(style, text)` | `String` | Render known ANSI styles conditionally or unconditionally. |
| `read_key()` | `Key` | Read a decoded key event. |
| `progress(label)` | `!IOError` | Write a progress update through the terminal stream. |

`style` is a no-op when output is not a TTY unless `style_force` is used. The
`Key` enum contains `Char`, `Escape`, `Backspace`, `Tab`, the four arrow keys,
and `Unknown`. `confirm` treats a bare Enter as no; `choose` returns an input
error after its finite retry budget or when stdin closes.

### `core.args` — declarative command-line parsing (D-ARGS1)

`core.args` provides both a direct `DataTree` decoder and an `ArgsSpec` builder.
The builder consumes a spec and returns a new value at each method call; parsing
is separate from process execution so tests can pass an explicit argument
vector.

```jet
use core.args as args
use core.process as process

fn run() {
    spec :: args.spec()
        .flag("verbose", "print extra detail")
        .option("output", "write result to FILE", "FILE")
        .positional("input", "file to read")
    parsed :: spec.parse(process.argv()) ?? return
    print(parsed.flag("verbose"))
    print(parsed.option("output") ?? "(default)")
}
```

| API | Result | Description |
| --- | --- | --- |
| `spec()` | `ArgsSpec` | Create an empty builder. |
| `spec.flag(name, help)` / `spec.flag_short(name, short, help)` | `ArgsSpec` | Register a boolean long flag and optional short spelling; clustered short flags work. |
| `spec.option(name, help, meta)` / `spec.option_short(name, short, help, meta)` | `ArgsSpec` | Register a string option using `--name VALUE`, `--name=VALUE`, and short forms. |
| `spec.option_int` / `spec.option_float` | `ArgsSpec` | Register typed numeric options. |
| `spec.option_choice(name, help, meta, choices)` | `ArgsSpec` | Restrict a string option to the declared comma-separated choices. |
| `spec.option_default(name, help, meta, value)` / `spec.option_env(name, help, meta, env)` | `ArgsSpec` | Supply a default or environment fallback. |
| `spec.required_option(name, help, meta)` / `spec.repeat(name, help, meta)` | `ArgsSpec` | Require one value or collect repeated values. |
| `spec.positional(name, help)` | `ArgsSpec` | Register a required positional value. |
| `spec.subcommand(name, help, spec)` | `ArgsSpec` | Add a named nested command specification. |
| `spec.description(text)` / `spec.version(text)` | `ArgsSpec` | Add help text or enable `--version`. |
| `spec.help()` / `spec.completion(shell)` | `String` | Render help or shell completion text. |
| `spec.parse(argv)` | `ParsedArgs !String` | Parse without exiting, suitable for tests and custom errors. |
| `spec.parse_or_exit(argv)` | `ParsedArgs` | Handle `--help` and usage errors through process exit. |
| `parsed.flag(name)` / `parsed.option(name)` | `Bool` / `?String` | Read a boolean or optional string value. |
| `parsed.option_int(name)` / `parsed.option_float(name)` | `?Int` / `?Float` | Read a typed numeric option. |
| `parsed.options(name)` / `parsed.positional(index)` | `[String]` / `?String` | Read repeated values or a zero-based positional. |
| `parsed.subcommand()` | `?String` | Read the matched subcommand name. |

`--help` and `--version` are recognized automatically. `parse` returns an error
string rather than exiting; `parse_or_exit` prints help and exits zero for
`--help`, or prints usage and exits two for invalid arguments. The decoder also
handles `--key=value`, `--key value`, `--flag`, `--no-flag`, clustered short
flags, negative numeric values, and `--` as the end-of-options marker.

The direct functions are useful when a typed builder is unnecessary:

| API | Result | Description |
| --- | --- | --- |
| `decode()` | `DataTree` | Decode the current process arguments, including `help` and `h` flags. |
| `decode_argv(argv)` | `DataTree` | Decode an explicit argument vector. |
| `merge(base, overlay)` | `DataTree` | Recursively merge objects; overlay arrays and scalars replace. |
| `get_text` / `get_bool` / `get_int` | scalar values | Read typed scalar fields with empty/false/zero fallbacks. |
| `positionals(tree)` / `remainder(tree)` | `[String]` | Read positional values. |
| `program(tree)` / `wants_help(tree)` | `String` / `Bool` | Read `argv[0]` or help intent. |

See [`args_spec.jet`](../../../Examples/features/io/args_spec.jet) and
[`args_audit.jet`](../../../Examples/features/io/args_audit.jet).

### `core.reflect` — runtime structural reflection (D-ANY-JAI1)

`reflect.of(x)` is the compiler-owned intrinsic for values that can be rendered
by `"{x}"`; the `core.reflect` source module itself owns the explicit text
classifier `inspect`. Runtime reflection is read-only and retains typed field
values rather than pre-rendering them.

```jet
use core.reflect as reflect

struct Point {
    x: Int
    y: Int

    impl Display {
        fn display(self) -> String { "({self.x}, {self.y})" }
    }
}

fn run() {
    p :: Point{x: 3, y: 4}
    value :: reflect.of(p)
    print(value.type_name())
    print(value.path())
    print(value.display())
    loop field in value.fields() -> print("{field.name()} = {field.value().display()}")
}
```

| API | Result | Description |
| --- | --- | --- |
| `reflect.of(value)` | `Value` | Project a compiler-registered runtime value snapshot. |
| `value.type_name()` | `String` | Return the declared leaf type name. |
| `value.path()` | `String` | Return the canonical typeable path. |
| `value.display()` | `String` | Return exactly the interpolation display for the value. |
| `value.fields()` | `[Field]` | Return struct fields in declaration order; primitives, enums, tuples, and lists return an empty list. |
| `field.name()` / `field.value()` | `String` / `Value` | Read a field name or its typed nested value. |
| `inspect(text)` | `ReflectValue` | Classify caller-supplied text without invoking `reflect.of`. |

A value that cannot satisfy the same display requirement as interpolation,
such as a closure or `Shared<T>`, is E0112 at the `reflect.of` call site. The
reflection floor has no runtime type registry, string-named field mutation, or
dynamic code loading; `get`, `set`, `clear`, `copy`, `equal`, `getfile`,
`getmodule`, and `loadfile` remain declined.

### `core.sys` — environment and system facts

`core.sys` exposes the process environment, working directory, platform facts,
process identifiers, and explicitly unsafe POSIX controls. Environment names
and values are checked by the provider; `get` returns `None` for an unset name,
while `unset` and `vars` report `EnvError` for invalid or non-Unicode state.

```jet
use core.sys as sys

fn run() {
    home :: sys.home_dir()
    mode :: sys.get("MODE") ?? "dev"
    sys.set("MODE", "prod")
    removed :: sys.unset("CI") ?? false
    names :: sys.vars() ?? []
    here :: sys.current_dir() ?? return
    print(home ?? "(no home)")
    print(mode)
    print("removed={removed} vars={names.len()} cwd={here}")
}
```

| API | Result | Description |
| --- | --- | --- |
| `get(name)` / `set(name, value)` | `?String -[Env]>` / `Unit -[Env]>` | Read or set a process environment value. |
| `unset(name)` / `vars()` | `Bool !EnvError -[Env]>` / `[String] !EnvError -[Env]>` | Remove a value or return the sorted name snapshot. `vars` exposes names, not values. |
| `current_dir()` | `String !IOError -[FS, Env]>` | Read the process working directory. |
| `set_current_dir(path)` | `!IOError -[FS, Env]>` | Change the process working directory. |
| `home_dir()` / `temp_dir()` | `?String -[Env]>` / `String -[Env]>` | Return platform home or temporary-directory values. |
| `name()` / `family()` / `arch()` | `String -[Env]>` | Return OS name, OS family, and CPU architecture. |
| `platform()` / `is_windows()` / `is_unix()` / `is_linux()` / `is_macos()` | platform values `-[Env]>` | Query the environment-derived platform facts. |
| `executable()` / `hostname()` / `username()` / `release()` / `version()` | `String -[Env]>` | Return process and operating-system facts. |
| `pid()` / `getpid()` / `getppid()` / `cpu_count()` | `Int -[Env]>` | Return process IDs or a logical CPU count of at least one. |
| `getuid()` / `geteuid()` / `getgid()` / `getegid()` / `getpgrp()` | `Int -[Env]>` | Return POSIX identity and process-group values where supported. |
| `getgroups()` | `[Int] -[Env]>` | Return supplementary group IDs. |
| `expand(value)` | `String -[Env]>` | Expand `$VAR`, `${VAR}`, and the supported percent form from the environment. |
| `uptime()` | `Float -[Time, Env]>` | Read system uptime. |
| `loadavg()` / `times()` | `[Float] -[Env]>` | Return load averages or process CPU times. |
| `exitcode(status)` / `success(status)` | `Int` / `Bool` | Interpret a wait status without an effect. |
| `sync()` | `Unit -[FS, Env]>` | Flush filesystem buffers. |
| `umask(mask)` | `Int -[FS, Env]>` | Set and return the previous creation mask. |
| `getpgid(pid)` / `getsid(pid)` / `getpriority(who)` | `Int !IOError -[Env]>` | Inspect POSIX process-group, session, or scheduling state. |
| `setpriority(who, priority)` | `!IOError -[Env]>` | Change scheduling priority. |
| `utime(path, atime, mtime)` | `!IOError -[FS, Env]>` | Change file timestamps. |
| `stop(code)` | never returns `-[Env]>` | Request process termination through the process boundary. |

The source-compatible `set` form reports an invalid environment call as E3001;
its future typed error form is a language-edition change, not a second Core
namespace. Names must be non-empty and contain neither NUL nor `=`; values
cannot contain NUL. `vars` does not silently skip an unrepresentable entry:
`EnvError.NonUnicode` reports it.

POSIX process/session controls require an OS gate and an audited `#Unsafe`
region:

| API | Result | Description |
| --- | --- | --- |
| `fork()` / `setsid()` | `Int !IOError -[Env]>` | Fork or create a process session. |
| `wait()` / `waitpid(pid, options)` | `Int !IOError -[Env, Time.Wait]>` | Reap a child or selected child. |
| `kill(pid, signal)` | `!IOError -[Env]>` | Send a signal. |
| `setuid` / `setgid` / `setpgid` / `setpgrp` / `initgroups` | `!IOError -[Env]>` | Change credentials, groups, or process-group membership. |
| `pipe()` / `close_fd(fd)` | `[Int] !IOError -[FS, Env]>` / `Unit -[FS, Env]>` | Create or close raw process descriptors. |
| `mkfifo(path, mode)` | `!IOError -[FS, Env]>` | Create a named pipe. |

The `core.sys` module exports neither `on_interrupt` nor `atexit`; use the
process cleanup and task APIs for the contracts that they provide.
---

### `core.process` — process execution

`core.process` is the process-boundary reference for application and tool
authors who launch or supervise child processes. Its executable contract is
defined by [`Core/process/process.jet`](../../../Core/process/process.jet), the
process checks in
[`process_ui.rs`](../../../crates/jet-sema/src/Sema/CheckerCoreLib/process_ui.rs),
and tests such as
[`tests/corelib_parts/system.rs`](../../../tests/corelib_parts/system.rs).
The export registry is
[`Prelude/Core.jet`](../../../crates/jet-codegen/src/Prelude/Core.jet); the
data example used later is
[`data_analysis.jet`](../../../Examples/features/tooling/data_analysis.jet).

`core.process` represents a child process as an explicit argument vector and a
policy-bearing `ProcessSpec`. `process.cmd` never invokes a shell: each
argument remains a separate string, including spaces and shell metacharacters.
Use `process.shell` only when the command line is intentionally delegated to
the platform shell (D-PROCESS1).

```jet
use core.process as process

fn run() -[Exec, Time.Wait]> {
    timeout :: Duration.seconds(30) ?? return
    spec :: process.cmd(["cp", "--", "directory with spaces;*.tmp", "backup"])
        .stdout(.Capture)
        .stderr(.Capture)
        .timeout(timeout)
    receipt :: spec.run_checked() ?? return
    print(receipt.output)
}
```

| Function or type | Returns | Description |
| --- | --- | --- |
| `process.args()` / `process.argv()` | `[String] -[Exec]>` | Return the process arguments supplied by the host. |
| `process.cmd(argv)` | `ProcessSpec -[Exec]>` | Build a specification from an explicit executable-and-arguments vector. |
| `process.shell(line)` | `ProcessSpec -[Exec, Env]>` | Explicit shell escape hatch: `sh -c` on Unix-like targets and `cmd /C` on Windows. |
| `process.run_spec(spec)` | `ProcessReceipt !IOError -[Exec, Time.Wait]>` | Run a specification and return its receipt without treating a nonzero exit as a thrown error. |
| `process.pipeline(steps)` | `ProcessReceipt !IOError -[Exec, Time.Wait]>` | Run a sequence of specifications and return the pipeline receipt. |
| `process.call(spec)` | `Int !IOError -[Exec, Time.Wait]>` | Run a specification and return its exit code. |
| `process.capture(spec)` | `ProcessReceipt !IOError -[Exec, Time.Wait]>` | Run with captured output. |
| `process.check_call(spec)` | `ProcessReceipt !IOError -[Exec, Time.Wait]>` | Run and check the resulting status. |
| `process.check_output(spec)` | `String !IOError -[Exec, Time.Wait]>` | Run and return captured standard output when the process succeeds. |
| `process.combined_output(receipt: ProcessReceipt)` | `String` | Purely project a receipt's stdout and stderr into one string; use `getoutput(spec)` to run a specification and capture output. |
| `process.getoutput(spec)` | `String !IOError -[Exec, Time.Wait]>` | Capture output and return it without converting a nonzero exit into an exception. |
| `process.getstatusoutput(spec)` | `(Int, String) !IOError -[Exec, Time.Wait]>` | Return the exit status and captured output. |
| `process.stdin_text(spec, text)` | `Unit !IOError -[Exec, Time.Wait]>` | Feed text to a child, close its standard input, wait, and check it. |
| `process.check(receipt)` | `Unit !IOError` | Convert a failed receipt to `IOError`, preserving timeout, resource-limit, exit-code, or signal information. |
| `process.status_ok(receipt)` | `Bool` | Test whether a receipt represents a successful exit. |
| `process.failed(receipt)` / `process.exited(receipt)` | `Bool` | Inspect failure or normal process exit on a receipt. |
| `process.current_pid()` | `Int -[Env]>` | Return the current process identifier. |
| `process.cwd(spec, path)` | `ProcessSpec` | Builder form that sets the child working directory; there is no zero-argument `process.cwd()`. |
| `process.env_get(name)` | `?String -[Env]>` | Read one environment variable; environment construction belongs to a `ProcessSpec`. |
| `process.env_get_or(name, fallback)` | `String -[Env]>` | Read an environment variable with a fallback. |
| `process.env_keys()` | `[String] !EnvError -[Env]>` | List environment-variable names. |
| `process.env_set(spec, key, value)` | `ProcessSpec` | Builder form that sets one child environment variable. |
| `process.env_truthy(name)` | `Bool -[Env]>` | Test the conventional truthy environment values. |
| `process.which(name)` | `?String -[FS, Env]>` | Find an executable using filesystem checks and the host search path. |
| `process.exit(code)` | `Never -[Exec]>` | Terminate the current process at the explicit process boundary. |
| `process.on_signal(signal)` | `Unit -[Exec]>` | Register the process signal policy for one `ProcessSignal` value. |
| `process.signal_number(signal)` | `Int` | Convert a `ProcessSignal` value to its platform signal number where one exists. |
| `process.list2cmdline(argv)` | `String` | Format an argument vector for display using Windows command-line quoting rules. |
| `ProcessSignal` | type | Closed variants: `Interrupt`, `Terminate`, `Hangup`, `Child`, `User1`, and `User2`. |
| `ProcessReceipt` | type | The immutable result of a run, including status, output, policy digests, backend, authority, descendants, limits, and redaction facts. |
| `ProcessSpec` | type | The executable, arguments, stream policy, authority, terminal policy, limits, and detachment policy for a child. |

#### Building a process policy

`ProcessSpec` builder methods return a modified specification. `.arg(value)`
and `.args_extend(values)` append arguments; `.cwd(path)` selects the working
directory; `.env(name, value)`, `.env_remove(name)`, and `.env_clear()` control
the child environment. `.stdin(mode)`, `.stdout(mode)`, and `.stderr(mode)`
select one of `.Stream`, `.Inherit`, or `.Capture`. `.terminal()` requests a
terminal session, while `.terminal(policy)` supplies an explicit
`TerminalPolicy`. `.detached()` allows the child to outlive the caller, and
`.abilities(...)` records the requested child abilities.

A specification can also carry `.timeout(duration)`, `.cpu_time_limit(value)`,
`.memory_limit(value)`, `.open_file_limit(value)`, and `.output_limit(value)`.
The limits are part of the process plan and receipt. `.under(authority)`
places the child under an `Authority`; `.plan()` returns the planned executable
identity, redacted argument vector, input digest, policy digest, backend,
authority, descendants, and limits without starting the child.

Resource-limit support is typed and fail-closed. Wall time and output limits
are represented in the process policy. Native CPU-time, memory, and open-file
limits are refused with an `IOError` when a target cannot enforce them; the
implementation does not silently claim a limit that the target cannot provide.
The macOS backend refuses memory and open-file limits, Windows refuses
open-file limits, and unsupported targets refuse the corresponding native
CPU, memory, or open-file requests.

#### Authority and output boundaries

An authority is explicit at the process boundary. `Authority.from_rights` can
construct a restricted authority from rights supplied by the caller, and
`Authority.workspace()` supplies the workspace authority used by examples.
The child inherits only the authority selected by the specification; an
unavailable or disallowed operation is an error rather than an implicit host
escape. This explicit authority boundary is the process execution contract (D-AGENT-EXEC1=A).

```jet
fn run_with_authority() -[Exec, Time.Wait]> {
    policy :: Authority.from_rights([
        "FS.Read:repo",
        "FS.Write:.jet/build",
        "Exec:/usr/bin/cargo",
    ])
    spec :: process.cmd(["cargo", "test"]).cwd("/workspace").under(policy)
    receipt :: process.run_spec(spec) ?? return
    print(receipt.success)
}
```

Captured output is governed by the stream mode and authority policy. A receipt
records output, error output, executable identity, argument and input digests,
policy digest, backend, authority, descendants, limits, and whether values
were redacted. Secret-bearing or disallowed output must not be made available
merely because the caller selected `.Capture`.

For a non-detached authority-bound specification, `.Capture` is required for
both output streams. `.Stream` and `.Inherit` are refused before spawn because
live descriptors would bypass receipt redaction; a detached child's output is
discarded and recorded in the plan and receipt.

Authority-bound terminal sessions and authority-bound pipelines refuse before
spawn. A terminal or pipeline cannot bypass the authority's captured, redacted
receipt boundary; ordinary unbound terminal sessions and pipelines retain their
separate APIs.

#### Cleanup and waiting

The cleanup law is deterministic. Deferred resource closes run in reverse
declaration order; scope guards run in reverse registration order; and
`atexit` handlers run in registration order. A host kill or abort can skip
language cleanup. `process.exit(code)` and `os.stop(code)` use the same
explicit process boundary, not ordinary return and defer unwinding (D-FAIL-EXIT1).
Deferred closes run before scope guards, and the requested exit code is
returned only after those actions and the registered `atexit` handlers finish.

`spec.run()` returns a `ProcessReceipt` after waiting. `spec.run_checked()`
waits and converts a failed exit into `IOError`; `process.check(receipt)`
performs the same status conversion for a receipt obtained through another
policy. `spec.spawn()` returns a `ProcessChild` for incremental interaction.
The default standard input is closed/null. `.Stream` and `.Capture` both use a
pipe that the parent must drain; `.Stream` exposes the stream interface,
whereas `.Capture` stores bounded output in the receipt. `.Inherit` connects a
child to the parent's corresponding stream.

| ProcessChild operation | Returns | Description |
| --- | --- | --- |
| `child.id()` | `Int` | Return the child identifier. |
| `child.wait()` | `ProcessReceipt !IOError` | Wait for completion and return the receipt. |
| `child.exited()` | `Bool !IOError` | Poll without waiting. |
| `child.kill()` / `.terminate()` / `.interrupt()` | `Unit !IOError` | Send the corresponding termination request. |
| `child.stdin.write(text)` / `.close()` | `Unit !IOError` | Write to or close a streamed child input. |
| `child.stdout.lines()` / `child.stderr.lines()` | line stream | Read lines from a streamed output pipe; the parent remains responsible for draining it. |

A terminal session is an opt-in process mode. It supplies terminal-sized input
and output through the terminal policy and fails with a typed error when the
selected backend cannot provide one; it is not an implicit replacement for
captured or inherited streams (D-PROCESS-SESSION1=A, D-PROCESS-SESSION2=D).

### `core.math` — numeric functions

`core.math` supplies integer operations, IEEE floating-point wrappers, numeric
predicates, checked and saturating arithmetic, and elementary functions. The
floating-point constant-like values are functions, so use `math.pi()` and
`math.e()` rather than treating them as fields.

```jet
use core.math as math

fn run() {
    angle := math.pi() / 4.0
    print(math.sin_cos(angle))
    print(math.checked_add(2, 3) ?? 0)
    print(math.is_finite(math.sqrt(2.0)))
}
```

| Function | Returns | Description |
| --- | --- | --- |
| `math.acos(x)`, `acosh(x)`, `asin(x)`, `asinh(x)`, `atan(x)`, `atan2(y, x)`, `atanh(x)` | `Float` | Inverse trigonometric and hyperbolic functions. The extended float family is part of the shared math surface (D-CORESURFACE1). |
| `math.abs(value: Int)` / `math.abs_float(value: Float)` | `Int` / `Float` | Absolute value for integer or floating-point input. |
| `math.min(a: Int, b: Int)` / `math.max(a: Int, b: Int)` | `Int` | Integer extrema. |
| `math.clamp(value, lo, hi)` | `Int` | Clamp an integer; reversed bounds are accepted by swapping the bounds. |
| `math.is_even(value)` / `math.is_odd(value)` | `Bool` | Test integer parity. |
| `math.sign(value)` | `Int` | Return the integer sign. |
| `math.isqrt(value)` | `?Int` | Return the integer square root when the input is nonnegative. |
| `math.gcd(a, b)` / `math.lcm(a, b)` | `Int` | Greatest common divisor or least common multiple. |
| `math.gcd_many(values)` / `math.lcm_many(values)` | `Int` | Fold integer gcd or lcm over a list. |
| `math.cos(x)`, `cosh(x)`, `sin(x)`, `sinh(x)`, `tan(x)`, `tanh(x)`, `math.cot(x)` | `Float` | Trigonometric and hyperbolic functions. |
| `math.exp(x)`, `exp2(x)`, `exp_m1(x)`, `expm1(x)`, `math.erf(x)`, `math.erfc(x)` | `Float` | Exponential and error functions. |
| `math.ln(x)`, `ln_1p(x)`, `log1p(x)`, `log10(x)`, `log2(x)`, `logb(x)`, `log(x, base)` | `Float` | Natural, base-specific, and compensated logarithms. |
| `math.sqrt(x)`, `cbrt(x)`, `hypot(x, y)`, `pow(x, y)` | `Float` | Roots, hypotenuse, and floating-point power. |
| `math.floor(x)` / `math.ceil(x)` / `math.trunc(x)` | `Float` | Directed or toward-zero rounding in floating-point form. |
| `math.fract(x)` | `Float` | Return the fractional part. |
| `math.gamma(x)` / `math.lgamma(x)` | `Float` | Gamma and log-gamma functions. |
| `math.degrees(x)` / `math.radians(x)` | `Float` | Convert between degrees and radians. |
| `math.lerp(a, b, t)` | `Float` | Linear interpolation. |
| `math.copysign(value, sign)` / `math.fma(a, b, c)` | `Float` | Copy a sign or perform fused multiply-add where supported by the numeric backend. |
| `math.muladd(a, b, c)` | `Float` | Multiply and add through the math wrapper. |
| `math.inv(x)` / `math.signum(x)` | `Float` | Reciprocal or floating-point signum. |
| `math.float32(x)` / `math.float64(x)` | `Float` | Convert through the corresponding floating-point representation. |
| `math.fabs(x)` / `math.real(x)` / `math.imag(x)` / `math.conj(x)` | `Float` | Absolute floating-point value and scalar component helpers. |
| `math.is_nan(x)`, `is_inf(x)`, `is_finite(x)` | `Bool` | Test NaN, infinity, or finite status. `isnan`, `isinf`, and `isfinite` are aliases. |
| `math.is_normal(x)`, `is_subnormal(x)`, `is_canonical(x)`, `is_signed(x)`, `is_zero(x)`, `is_integer(x)`, `sign_bit(x)` | `Bool` | Inspect floating-point representation properties. |
| `math.next_up(x)` / `next_down(x)` / `next_after(x, toward)` / `nextafter(x, toward)` | `Float` | Move to an adjacent representable value. |
| `math.significand(x)` / `math.ulp(x)` | `Float` | Return the significand or unit in the last place. |
| `math.ilogb(x)` / `math.ldexp(x, exponent)` / `math.scaleb(x, exponent)` / `math.radix(x)` | `?Int` or `Float` | Inspect or scale the floating-point exponent and radix. |
| `math.pi()` / `math.e()` / `math.tau()` / `math.tau_const()` | `Float` | Mathematical constants exposed as functions. |
| `math.infinity()` / `math.nan()` / `math.zero()` | `Float` | Construct the corresponding special floating-point values. |
| `math.round(x)` | `Int` | Round to the nearest integer, with ties away from zero. |
| `math.to_bits(x)` / `math.from_bits(bits)` | integer bits / `Float` | Convert between a floating value and its bit representation. |
| `math.sin_cos(x)` | `(cos: Float, sin: Float)` | Compute cosine and sine together. |
| `math.modf(x)` | `(fract: Float, whole: Float)` | Split a value into fractional and whole parts. |
| `math.frexp(x)` | `(exp: Int, frac: Float)` | Split a value into exponent and fraction. |
| `math.checked_abs(x)`, `checked_neg(x)`, `checked_add(a,b)`, `checked_sub(a,b)`, `checked_mul(a,b)`, `checked_div(a,b)`, `checked_rem(a,b)`, `checked_pow(a,b)` | `?Int` | Return `None` on integer overflow or invalid division/remainder; checked power also rejects a negative exponent. |
| `math.int_pow(base, exponent)` | `Int` | Integer exponentiation; a negative exponent produces `0`. |
| `math.saturating_add(a,b)`, `saturating_sub(a,b)`, `saturating_mul(a,b)` | `Int` | Perform integer arithmetic while clamping overflow to the representable endpoint. |
| `math.factorial(n)` / `math.binomial(n, k)` / `math.perm(n, k)` | `?Int` | Return optional combinatorial results when the input or result is outside the supported integer domain; `comb` aliases `binomial`. |

#### Measurement propagation

A measured value stores a numeric value and a nonnegative standard uncertainty.
The measurement operations use **first-order linear propagation with
uncorrelated inputs**: addition and subtraction use root-sum-square
uncertainty, and multiplication, division, and square root use the
corresponding first-order derivatives. Exact inputs have zero uncertainty.
**Correlated errors are out of scope** (D-TYPE2-UNCERT1).

#### Linear algebra

Fixed-size vectors and matrices use `Float` components. `Vec2`, `Vec3`, and
`Vec4` provide constructors, `splat`, `from_array`, `to_array`, checked lane
indexing, vector addition and subtraction, and element-wise multiplication.
`Vec3` also supports scalar multiplication and division, and the `dot`,
`cross`, `length`, and `normalize` operations. `Mat3` and `Mat4` constructors
use column-major storage; they provide `from_array`, `to_array`, addition,
subtraction, matrix multiplication, `matmul`, `transform`, and `transpose`.
Reductions use a scalar left-to-right fold, so the reduction order is part of
the reproducible fixed-array contract (D-LINALG1; `to_array` is the fixed-array bridge, D-FIXARR1).

```jet
fn rotate(v: Vec3, transform: Mat3) -> Vec3 {
    unit :: v.normalize()
    return transform.transform(unit)
}
```

#### SIMD lanes

The SIMD-like lane types have fixed-array semantics independent of whether a
backend selects vector instructions. Floating types are `F32x4`, `F64x2`,
`F32x8`, and `F64x4`. Signed integer types are `I8x16`, `I16x8`, `I32x4`,
`I64x2`, `I8x32`, `I16x16`, `I32x8`, and `I64x4`; unsigned types use the
corresponding `U` prefixes. Every lane type supports `splat`,
`from_array`, `to_array`, checked indexing, and `+`, `-`, `*`, and `/` with
matching lane types (D-SIMD1/D-SIMD2/D-SIMD3).

### `core.units` — measured quantities

`core.units` supplies named SI and binary-unit scales over
`Measurement<Float>`. It is a scalar conversion layer rather than a
user-defined dimension system. The module-global scale names are uppercase
(`NANO`, `MICRO`, `MILLI`, `CENTI`, `KILO`, `MEGA`, `GIGA`, `METER`, `METRE`,
`GRAM`, `SECOND`, `BYTE_UNIT`, `KIBIBYTE`, `MEBIBYTE`, and `GIBIBYTE`).

```jet
use core.units as units

fn run() {
    distance := units.meters(12.0)
    elapsed := units.seconds(3.0)
    print(units.show(distance))
    print(units.to_si(elapsed))
}
```

| Function | Returns | Description |
| --- | --- | --- |
| `units.from(magnitude, unit)` | `Measurement<Float>` | Build a measured quantity from a magnitude and a scalar unit. |
| `units.si(value)` | `Measurement<Float>` | Construct a value in the SI base scale. |
| `units.scale(value, factor)` | `Measurement<Float>` | Scale a measured value. |
| `units.add(a, b)` / `units.sub(a, b)` | `Measurement<Float>` | Add or subtract measured values with propagated uncertainty. |
| `units.mul(value, scalar)` / `units.div(value, scalar)` | `Measurement<Float>` | Multiply or divide a measured value by a scalar, propagating its uncertainty. |
| `units.convert(value, target)` | `Measurement<Float>` | Convert to a target scale; an invalid, nonfinite, or zero target leaves the value unchanged. |
| `units.to_si(value)` | `Float` | Return the SI scalar value. |
| `units.abs(value)` | `Measurement<Float>` | Take the absolute value while retaining uncertainty. |
| `units.show(value)` | `String` | Format a measured value and its uncertainty. |
| `units.meters(x)` / `metres(x)` | `Measurement<Float>` | Construct a metre-scale value. |
| `units.kilometers(x)` / `kilometres(x)` | `Measurement<Float>` | Construct a kilometre-scale value. |
| `units.grams(x)` / `kilograms(x)` | `Measurement<Float>` | Construct gram- or kilogram-scale values. |
| `units.seconds(x)` / `milliseconds(x)` | `Measurement<Float>` | Construct second- or millisecond-scale values. |
| `units.bytes_of(x)` / `kibibytes(x)` | `Measurement<Float>` | Construct byte or kibibyte-scale values. |
| `units.equals(a, b)` / `units.ratio(a, b)` | `Bool` / `Float` | Compare values or compute a ratio; a zero denominator gives ratio `0`. |
| `units.is_zero(value)` | `Bool` | Test whether the measured value is zero. |

### `core.math.random` — deterministic pseudorandom values

`core.math.random` is a deterministic pseudorandom generator, not a
cryptographic random source. Ambient draws use the `Rand` effect; explicit
`Rng` values make a stream visible and reproducible. Use `core.crypto.random`
for security-sensitive bytes or secrets (D-DET1).

```jet
use core.math.random as random

fn roll_at(rng: &Rng) -> String {
    value := rng.int(1, 6)
    return "roll=" + value.to_string()
}

fn run() {
    rng := random.rng(42)
    print(roll_at(rng))
}
```

| Function or method | Returns | Description |
| --- | --- | --- |
| `random.seed(value)` | `Unit -[Rand]>` | Seed the ambient deterministic stream. |
| `random.rng(seed)` | `Rng` | Create an explicit deterministic stream. |
| `random.split(seed)` | `Rng -[Rand]>` | Derive a stream from the ambient stream and a seed. |
| `random.int(lo, hi)` / `rng.int(lo, hi)` | `Int` | Draw an inclusive integer; reversed bounds return `lo` without a draw. |
| `random.float()` / `rng.float()` | `Float` | Draw from `[0, 1)`. |
| `random.float_range(lo, hi)` / `rng.float_range(lo, hi)` | `Float` | Draw from `[lo, hi)`; invalid or reversed bounds return `lo` without a draw. |
| `random.bool()` / `rng.bool()` | `Bool` | Draw a Boolean. |
| `random.bytes(n)` / `rng.bytes(n)` | `[U8]` | Draw bytes; a negative length gives an empty list. |
| `random.pick(items)` / `rng.pick(items)` | `?String` or `?T` | Pick one item; an empty collection returns `None` without advancing the stream. |
| `random.weighted_pick(items, weights)` / `rng.weighted_pick(items, weights)` | `?T` | Pick by nonnegative finite weights; mismatched, invalid, or zero-total inputs return `None` without a draw. |
| `random.shuffle(values)` / `rng.shuffle(values)` | `Unit` | Shuffle a mutable integer list in place. |
| `random.sample(values, n)` / `rng.sample(values, n)` | `[Int]` | Return a sample of the requested size. |
| `random.normal(mean, stddev)` / `rng.normal(mean, stddev)` | `Float` | Draw a normal value; invalid mean returns `0`, and a nonpositive or nonfinite standard deviation returns the mean. |
| `random.exponential(rate)` / `rng.exponential(rate)` | `Float` | Draw an exponential value; invalid parameters return `0`. |
| `rng.split()` | `Rng` | Derive a child stream without sharing mutable stream state. |
| `random.randint`, `uniform`, `normalvariate`, `gauss`, `expovariate`, `randbytes`, `getrandbits`, `randrange`, `choice`, `choices`, `triangular`, `gammavariate`, `betavariate`, `lognormvariate`, `paretovariate`, `weibullvariate`, `vonmisesvariate`, `binomialvariate` | varies | Compatibility aliases and distributions exposed by the random module. |
An explicit `Rng` is a seeded capability: its draw methods advance that
capability without requiring the ambient `Rand` effect (D-DET-CAPAPI).

### `core.compute.solve` — dense linear solves

`core.compute.solve` contains checked CPU linear solvers over
`core.compute` tensors. `solve.dense` validates a square coefficient matrix,
a rank-one or rank-two right-hand side, matching CPU device/profile, finite
values, and a non-singular factorization. `solve.lu` factors a matrix with
partial pivoting and returns a `LinearSolver` factorization carrier; malformed,
non-square, nonfinite, singular, unsupported-device, and limit failures are
`ComputeError` values.

| Function or type | Returns | Description |
| --- | --- | --- |
| `solve.dense(a: Tensor, b: Tensor)` | `Tensor !ComputeError` | Factor and solve a dense system in one operation. |
| `solve.lu(a: Tensor)` | `LinearSolver !ComputeError` | Build a partial-pivoting LU factorization carrier for a square CPU tensor. |
| `LinearSolver` | type | Stores solver kind, dimension, factors, pivots, and device. |

The supported strict reproducible profiles are `F64Strict+Reproducible` and
`F32Strict+Reproducible`. The solver is CPU-only and rejects tensors above the
configured element limit rather than silently moving them to another device.

### `core.game` — scene execution and replays

`core.game` provides a small scene runner with explicit scene, replay, and
backend values. A scene owns its name, frame limit, and tick callback; a replay
records a path; and the headless backend produces a deterministic frame
transcript without requiring a renderer or audio device.

```jet
use core.game as game

fn run() {
    scene := game.Scene.new("counter")
    scene.max_frames = 3
    scene.on_frame((frame) -> {
        print("frame=" + frame.to_string())
    })
    backend :: game.Backend.headless()
    replay :: game.Replay.record("build/counter.replay")
    receipt :: game.run(scene, replay: replay, backend: backend, frames: 3)
    print(receipt)
}
```

| Function or type | Returns | Description |
| --- | --- | --- |
| `game.Scene.new(name)` | `Scene` | Construct a scene with a name, frame policy, and tick callback. |
| `game.Replay.record(path)` | `Replay` | Select a replay destination. |
| `game.Backend.headless()` | `Backend` | Select a renderer-free deterministic backend. |
| `game.run(scene, replay, backend, frames)` | `String` | Run a mutable scene with optional replay, backend, and positive frame limit. |
| `Scene` | type | Scene state and the frame callback consumed by the runner. |
| `Replay` | type | Replay destination and transcript policy. |
| `Backend` | type | Backend name and running state. |

### `core.game.raylib` — display-gated drawing

`core.game.raylib` is a display-gated bridge. It has no host-side fallback: a
headless or unavailable display returns provider state rather than silently
pretending to draw. Textures, sounds, windows, and colors are typed handles;
resource loading requires the corresponding file and GPU authority.

| Function or type | Returns | Description |
| --- | --- | --- |
| `core.game.raylib.window_open(width, height, title)` | `RaylibWindow -[GPU]>` | Open a window; dimensions are clamped to `1..2,147,483,647`. |
| `raylib.window_ready(window)` / `window_should_close(window)` | `Bool -[GPU]>` | Query window readiness or close state. |
| `raylib.begin_drawing(window)` | `Unit -[GPU]>` | Begin a frame for a window. |
| `raylib.end_drawing()` | `Unit -[GPU]>` | Finish the active frame. |
| `raylib.clear_background(color)` | `Unit -[GPU]>` | Clear the active frame. |
| `raylib.close_window(window)` | `Unit -[GPU]>` | Close a window and release its display handle. |
| `raylib.color(r, g, b, a)` | `RaylibColor` | Clamp each component to `0..255` and construct a color. |
| `raylib.draw_rectangle(x, y, width, height, color)` | `Unit -[GPU]>` | Draw a rectangle. |
| `raylib.draw_text(text, x, y, size, color)` | `Unit -[GPU]>` | Draw text. |
| `raylib.draw_sprite(atlas, source, x, y)` | `Unit -[GPU]>` | Draw a sprite from a texture atlas. |
| `raylib.load_texture_atlas(path)` | `RaylibTextureAtlas -[FS, GPU]>` | Load a texture atlas through the authority boundary. |
| `raylib.load_sound(path)` | `RaylibSound -[FS, GPU]>` | Load a sound resource. |
| `raylib.play_sound(sound)` | `Bool -[GPU]>` | Start playback and report whether it was accepted. |
| `raylib.set_target_fps(fps)` | `Unit -[GPU]>` | Set a target frame rate, clamped to `1..240`. |
| `raylib.key_down(key)` / `gamepad_down(pad, button)` | `Bool -[GPU]>` | Read keyboard or gamepad button state. |
| `raylib.gamepad_axis(pad, axis)` | `Float -[GPU]>` | Read a gamepad axis. |
| `RaylibColor`, `RaylibWindow`, `RaylibTextureAtlas`, `RaylibSound` | types | Typed bridge handles; they are not interchangeable with ordinary strings or integers. |

### `core.perf` — fidelity scaling

`core.perf` maps a fidelity value to bounded work scaling. `fidelity()` reads
the environment-selected value; `default_fidelity()` is the pure value `1.0`.
`override_fidelity` and `reset_fidelity` modify the environment-scoped value.

| Function or type | Returns | Description |
| --- | --- | --- |
| `perf.fidelity()` | `Float -[Env]>` | Read the selected fidelity. |
| `perf.default_fidelity()` | `Float` | Return `1.0`. |
| `perf.override_fidelity(value)` | `Unit !String -[Env]>` | Set fidelity in the inclusive range `[0, 1]`; reject NaN and out-of-range values. |
| `perf.reset_fidelity()` | `Unit -[Env]>` | Restore the default selection. |
| `perf.of(value)` | `Perf` | Construct a bounded fidelity value; NaN becomes `1`. |
| `perf.is_full(value)` / `perf.is_low(value)` | `Bool` | Test full fidelity or fidelity below `0.25`. |
| `perf.scale(value, work)` | `Int` | Scale nonnegative work by fidelity. |
| `Perf` | type | A bounded fidelity value consumed by scaling operations. |

### `core.text` — UTF-8 text operations

`core.text` operates on UTF-8 strings. Scalar counts, trimming, case
conversion, and predicates use decoded Unicode scalars; named byte operations
use bytes. Jet uses its pinned Unicode 17.0.0 tables, not the host's Rust, OS,
locale, or terminal Unicode version. Empty strings are false for the
empty-sensitive predicates. `splitn` and `rsplitn` limit the number of
resulting parts (D-TEXTUNICODE1=A).

| Function | Returns | Description |
| --- | --- | --- |
| `text.byte_count(value)` | `Int` | Count UTF-8 bytes. |
| `text.scalar_count(value)` | `Int` | Count decoded Unicode scalars. |
| `text.is_ascii(value)` | `Bool` | Test whether every byte is ASCII. |
| `text.scalars(value)` | `[String]` | Return decoded Unicode scalars. |
| `text.lower(value)` / `upper(value)` / `casefold(value)` | `String` | Apply Unicode-aware scalar case mappings supported by the runtime. |
| `text.caseless_eq(a, b)` | `Bool` | Compare after case folding. |
| `text.nfc(value)` / `nfd(value)` / `nfkc(value)` / `nfkd(value)` | `String` | Apply the named normalization form. |
| `text.graphemes(value)` / `words(value)` / `sentences(value)` | `[String]` | Return grapheme, word, or sentence segments. |
| `text.char_indices(value)` | `[String]` | Return scalar strings with their UTF-8 byte indexes. |
| `text.display_width(value)` | `Int` | Compute terminal display width under the portable text-width policy (D-TEXTWIDTH1=B). |
| `text.grapheme_views(value)` / `word_views(value)` / `line_views(value)` / `byte_views(value)` | `[String]` | Produce views or segments for the requested boundaries. |
| `text.is_alphabetic(value)` / `is_numeric(value)` / `is_whitespace(value)` | `Bool` | Test the corresponding scalar property. |
| `text.trim(value)` / `trim_start(value)` / `trim_end(value)` | `String` | Remove supported whitespace at both or one end. |
| `text.pad_start(value, width, fill)` / `pad_end(value, width, fill)` | `String` | Pad to a requested display width. |
| `text.center(value, width, fill)` | `String` | Center a value in a requested width. |
| `text.starts_any(value, prefixes)` / `ends_any(value, suffixes)` | `Bool` | Test any of several prefixes or suffixes. |
| `text.splitn(value, separator, limit)` / `rsplitn(value, separator, limit)` | `[String]` | Split from the left or right with a maximum part count. |
| `text.inspect(value)` | `[String]` | Return escaped scalar inspection values. |
| `text.Cursor` / `text.cursor(value)` / `text.cursor_advance(text, cursor)` | type / cursor | Construct and advance a text cursor. |

`String.len()` is the compiler-owned receiver-first method and reports byte
length. The public convenience surface is the qualified `core.text` module;
the old catalogue of unqualified string methods is not a separate core module
contract (D-STR-DECLINE1=C).

### `core.time` — dates, durations, and clocks

`core.time` uses the proleptic Gregorian calendar, including year `0` (1 BCE),
and integer Unix conversion formulas. Date arithmetic and parsing helpers are
pure unless their signature carries `Time`; `now`, `now_utc`, `today`,
`instant`, `start`, `sleep`, and `sleep_until` read or wait on the host clock.
`zone` accepts UTC/GMT/Z and numeric offsets; the runtime host also resolves
named IANA zones from TZif data. On native hosts, lookup checks
`JET_TZDB_DIR`, `TZDIR`, `$JET_ROOT/Core/time/tzdb`, the repository's
`Core/time/tzdb`, `/usr/share/zoneinfo`, `/usr/share/lib/zoneinfo`, and
`/etc/zoneinfo`. Optional `posix/` and `right/` prefixes are stripped for
lookup. Web/WASM uses its embedded zone data. A name with `..`, a leading
`/`, or a leading `\` is rejected; an unavailable name returns `TimeError`
with an `unknown IANA time zone` message that points to `JET_TZDB_DIR` or
`TZDIR` (D-FREESTAND-TIME1).

```jet
use core.time as time

fn run() {
    stamp :: time.parse_rfc3339("2025-01-02T03:04:05Z") ?? return
    print(time.isoformat(stamp))
    clock :: time.Clock.new(0)
    &clock.tick(1000)
    print(clock.now())
}
```

| Function or type | Returns | Description |
| --- | --- | --- |
| `time.is_leap_year(year)` | `Bool` | Test leap-year status under the proleptic Gregorian calendar. |
| `time.days_in_month(year, month)` | `Int` | Return the number of days; an invalid month returns `0`. |
| `time.period(start, end)` / `period_days(n)` / `period_months(n)` / `period_years(n)` | `Period` | Construct calendar periods. |
| `time.utc()` / `time.zone(name)` | `Zone` / `Zone !TimeError` | Construct UTC, a numeric-offset zone, or a named IANA TZif zone. |
| `time.utcoffset(zone)` | `Int` | Return the zone offset represented by the zone. |
| `time.gmtime(seconds)` / `time.ctime(seconds)` / `time.asctime(date_time)` | date or `String` | Convert Unix time or format a date. |
| `time.time(hour, minute, second)` / `time.local_time(hour, minute, second)` | `LocalTime` | Construct a local time; seconds through `60` can represent an input leap-second shape. |
| `time.new(year, month, day)` | `LocalDate` | Construct a date, clamping month and day to the accepted calendar ranges. |
| `time.datetime(year, month, day, hour, minute, second)` | `DateTime` | Construct a date-time value. |
| `time.from_timestamp(seconds)` / `from_unix_seconds(seconds)` / `from_unix_ms(ms)` / `from_unix_microseconds(us)` / `from_unix_nanoseconds(ns)` | `DateTime` | Convert Unix-based values at the named precision. |
| `time.from_iso_week(year, week, weekday)` / `parse_iso_week_date(text)` | `LocalDate !TimeError` | Convert or parse an ISO week date. |
| `time.parse(text)` / `parse_time(text)` | `LocalDate !TimeError` / `LocalTime !TimeError` | Parse supported date or time text, including the documented leap-second shape. |
| `time.parse_rfc3339(text)` | `DateTime !TimeError` | Parse an RFC3339 timestamp through the zoned parser. |
| `time.parse_zoned(text)` | `ZonedDateTime !TimeError` | Parse an RFC9557 date-time with a bracketed IANA zone and a matching UTC offset. |
| `time.zoned_local(date, local_time, zone, disambiguation)` | `ZonedDateTime !TimeError` | Resolve local time in a zone with `compatible`, `earlier`, `later`, or `reject` policy. |
| `time.now()` / `now_utc()` / `today()` / `start()` | time value `-[Time]>` | Read the host wall clock. |
| `time.instant()` | `Instant -[Time]>` | Read the monotonic clock as a Time point: `t + 5min` is an `Instant`, `b - a` and `t.elapsed()` are `Duration`s. An `Instant` has no epoch and no fields (D-TIME-INSTANT1=A). |
| `time.unix_ns()` | `Int -[Time]>` | Wall-clock Unix nanoseconds, for timestamps, nonces, and expiry checks. |
| `time.sleep(duration)` / `sleep_until(deadline)` | `Unit -[Time]>` | Wait on the host clock. |
| `time.add_days(date, days)` | `LocalDate` | Add calendar days. |
| `time.compare_date(a, b)` / `date_equals(a, b)` | ordering / `Bool` | Compare dates. |
| `time.duration_as_seconds(duration)` / `elapsed(start)` | `Int` / `Duration` | Convert or measure elapsed time. |
| `time.isoformat(value)` / `isoformat_date(value)` / `isoformat_time(value)` | `String` | Format supported date, time, and zoned values. |
| `time.unix_ms(value)` / `unix_seconds(value)` | `Int` | Convert a time value to Unix units. |
| `time.weekday(value)` | `Int` | Return the weekday index. |
| `time.Clock.new(seed)` | `Clock` | Construct a deterministic manually advanced clock. |
| `time.Clock.system()` | `Clock -[Time]>` | Construct a clock backed by the host time source. |
| `clock.now()` | `Int` | Read clock milliseconds. |
| `clock.tick(milliseconds)` / `clock.advance(to_ms)` | `Int` | Advance a manual clock and return its new millisecond value. |
| `clock.wait(duration)` | `Int -[Time]>` | Advance or wait according to the clock policy and return the new value. |
| `Duration.nanoseconds(n)` / `microseconds(n)` / `milliseconds(n)` / `seconds(n)` / `minutes(n)` / `hours(n)` | `Duration !RangeError` | Construct a checked duration from the named unit. |
| `duration.in(unit)` / `total_in(unit)` | `Int !RangeError` / `Float` | Convert a duration to a named unit. |
| `duration.is_zero()` / `total_seconds()` / `difference(other)` / `round(unit)` / `abs()` / `negated()` / `sign()` | varies | Inspect or transform a duration. |

#### Named zones and local-time resolution

`parse_zoned(text)` accepts RFC9557-style text with a bracketed zone name,
for example `2025-01-02T03:04:05-05:00[America/New_York]`. The bracketed
name is resolved through the same TZif lookup as `zone(name)`, and the
numeric offset must be valid for that local date and time in the zone.
Missing brackets, an unknown zone, malformed offsets, and an offset that does
not match the zone return `TimeError`.

`zoned_local(date, local_time, zone, disambiguation)` resolves a local
date-time against zone transitions. `compatible` is the default; for an
ambiguous fall-back time, `compatible` and `earlier` choose the earlier
instant, while `later` chooses the later instant. For a spring-forward gap,
`compatible` and `later` use the pre-transition offset, while `earlier` uses
the post-transition offset. `reject` reports ambiguous, nonexistent, or
otherwise unresolved local times instead of choosing one.

Duration literals use the supported `ns`, `us`, `ms`, `s`, `min`, `h`, and `d`
suffixes. A runtime `Duration` carries checked nanoseconds; a deterministic
function should receive a `Clock` explicitly rather than reading `time.now()`
internally. The manual clock then makes time an ordinary input that can be
replayed (D-TIMERES1=A; D-DET-CAPAPI).

### `core.time.calendar` — calendar formatting

`core.time.calendar` provides fixed English calendar names and Monday-based
week calculations without locale tables. Its module-global weekday constants
are `MONDAY :: 0` through `SUNDAY :: 6`.

| Function or value | Returns | Description |
| --- | --- | --- |
| `calendar.isleap(year)` | `Bool` | Test leap-year status. |
| `calendar.leapdays(y1, y2)` | `Int` | Count leap days in a half-open year range. |
| `calendar.weekday(year, month, day)` | `Int` | Return Monday `0` through Sunday `6`. |
| `calendar.monthrange(year, month)` | `(Int, Int)` | Return the first weekday and day count. |
| `calendar.monthcalendar(year, month)` / `monthcalendar_start(year, month, firstweekday)` | `[[Int]]` | Build a month grid. |
| `calendar.yearcalendar(year)` | `[[[Int]]]` | Build a year calendar. |
| `calendar.day_name(index)` / `day_abbr(index)` | `String` | Return fixed English day names. |
| `calendar.month_name(index)` / `month_abbr(index)` | `String` | Return fixed English month names. |
| `calendar.weekheader(width, firstweekday)` | `[String]` | Format abbreviated weekday headers. |
| `calendar.formatmonth(year, month, width)` / `formatyear(year)` | `String` | Render a month or year calendar. |
| `calendar.timegm(year, month, day, hour, minute, second)` | `Int` | Convert a UTC calendar tuple to Unix seconds. |

### `core.encoding` — bounded value codecs

`core.encoding` converts complete values to and from `DataTree`. Objects retain
insertion order, integers and floating-point numbers remain distinct, and
malformed input returns a typed `EncodingError`. The whole-value codecs are
pure; readers and writers expose the same boundaries incrementally. The
shared codec shape and value tree follow D-ENC1, D-JSONVERB1, and D-SERDE13.

| Function or type | Returns | Description |
| --- | --- | --- |
| `encoding.DataTree` | type | Dynamic tree of null, Boolean, integer, float, lexical number, typed text, string, bytes, array, and ordered-object values. |
| `encoding.DataEvent` | type | Event emitted by an incremental reader. |
| `encoding.EncodingCause`, `EncodingErrorKind`, `EncodingError` | types | Structured encoding failure information. |
| `encoding.EncodingFormat` | type | Format selector used by generic readers and writers. |
| `encoding.EncodingLimits` | type | Depth, item-size, and format limits. |
| `encoding.Reader` / `encoding.Writer` | types | Generic incremental reader and writer interfaces. |
| `encoding.bytes_to_hex(bytes)` / `hex_to_bytes(text)` | `String` / `[U8] !HexError` | Convert bytes to or from lowercase hexadecimal. |
| `encoding.hex_nibble(value)` / `hex_value(value)` | `U8` / `?Int` | Encode a nibble or look up a hexadecimal value. |
| `encoding.wrap32(value)` | `Int` | Wrap an integer to 32 bits. |

The standard formats are exposed as `core.encoding.base32`, `base64`, `binary`,
`cbor`, `csv`, `hex`, `ini`, `json`, `jsonl`, `toml`, `xml`, and `yaml`.

#### Typed codecs and validation

A parser returns a `DataTree` (or a format-specific document). A typed codec
maps a declared Jet type to and from that tree. `#Codable`, `#Encode`, and
`#Decode` derive the typed encode/decode routes; field attributes such as
`#Rename`, `#Skip`, `#Flatten`, `#RenameAll`, `#DenyUnknownFields`,
`#Discriminant`, and `#Untagged` describe the wire shape. Unknown fields are
ignored unless a type opts into rejection. A field conversion or missing
required field is reported as a `FieldError` rather than silently defaulting to
an unrelated type.
`#Codable` requests both directions; `#Encode` and `#Decode` request one-way derivation, and derivation is compiler-owned rather than macro- or reflection-driven (`D-SERDE1–8`). Typed `decode<T>` produces `T ![FieldError]` (or `[T] ![FieldError]` for CSV); each failure preserves its path and reason, and generic codec bounds are injected at the use site (`D-GENERIC-CALL1`; `D-SERDE6`; `D-SERDE9–12`). Hand-written codecs use the same `Encode`/`Decode` protocol and field-path accumulation (`D-SERDE2`; `D-SERDE13–16`).

Validation can be declared as `validate { check(...) }`: all failed checks accumulate, and conditions and messages are purity-checked (`D-VALIDATE1`; `D-FIELDPOL1`; `S60/E3401`).

The concrete format sources live under
[`Core/encoding/`](../../../Core/encoding/), and typed CSV/JSON convenience
functions are also exported by [`Core/data/data.jet`](../../../Core/data/data.jet).
A decoder is only type-safe when its destination has an explicit shape; do not
use an untyped string round trip as a substitute for a typed codec.

### `core.encoding.json` — JSON trees

`core.encoding.json` parses strict RFC 8259-style JSON into `DataTree`. Integer
literals remain exact integers; fractional values are floats. NaN and infinity
syntax is rejected. `decode` has the same strict behavior as `parse`, despite
its historical name. Compact output retains insertion order, pretty output
uses two-space indentation, and `canonical` is RFC 8785 JCS with UTF-16 key
order. `parse`, `decode`, and `reader` reject a repeated object name with its
path and position; the `_allow_duplicates` siblings are the explicit opt-in.
The reader enforces depth `256` and a maximum item size of `16,777,216` bytes.

| Function or type | Returns | Description |
| --- | --- | --- |
| `json.parse(text)` / `decode(text)` | `DataTree !EncodingError` | Parse one strict JSON value. |
| `json.loads(text)` / `load(text)` | `DataTree !EncodingError` | Compatibility aliases for parsing text. |
| `json.dumps(value)` / `dump(value)` / `to_string(value)` | `String` | Emit compact JSON. |
| `json.to_string_pretty(value)` | `String` | Emit two-space pretty JSON. |
| `json.canonical(value)` | `String !EncodingError` | Emit RFC 8785 JCS bytes: UTF-16 key order, ECMAScript numbers; reject NaN, infinities, and an `Int` that binary64 cannot hold exactly. |
| `json.parse_allow_duplicates(text)` | `DataTree !EncodingError` | Parse like `parse`, but keep the later value of a repeated object name. |
| `json.pointer(tree, path)` | `DataTree !EncodingError` | Select by RFC 6901 JSON Pointer; a missing member or bad step is a located error, JSON `null` is a value. |
| `json.patch(doc, ops)` / `patch_with_limits(doc, ops, limits)` | `DataTree !EncodingError` | Apply an RFC 6902 operation array atomically; `doc` is unchanged on failure. |
| `json.events(value)` | `[DataEvent]` | Produce the canonical event sequence for a tree. |
| `json.reader(text)` / `json.reader_allow_duplicates(text)` / `json.writer()` | `JSONReader` / `JSONWriter` | Construct a pull reader (rejecting, or reporting every repeated name) or a value writer. |
| `JSONReader.next()` | `DataEvent !EncodingError` | Return the next event. |
| `JSONWriter.write(value)` / `to_string()` | `Unit` / `String` | Feed values and finish an encoded value. |

### `core.encoding.jsonl` — line-delimited JSON

`core.encoding.jsonl` parses one JSON value per nonblank line. Blank lines are
skipped, each nonblank line is parsed independently, and one malformed line
returns an `EncodingError`. Compact output writes one value and a trailing
newline per row.

| Function or type | Returns | Description |
| --- | --- | --- |
| `jsonl.parse(text)` / `loads(text)` | `[DataTree] !EncodingError` | Parse all nonblank lines. |
| `jsonl.to_string(values)` / `dumps(values)` | `String` | Encode values as compact JSON lines with a trailing newline per row. |
| `jsonl.count_rows(text)` | `Int` | Count nonblank rows. |
| `jsonl.append_line(text, value)` | `String` | Append one encoded row. |
| `jsonl.first(text)` | `?DataTree !EncodingError` | Return the first nonblank row. |
| `jsonl.reader(text)` / `jsonl.writer()` | `JSONLReader` / `JSONLWriter` | Construct incremental line reader or writer. |
| `JSONLReader.next()` | `?DataTree !EncodingError` | Return the next row or stable end-of-input. |
| `JSONLWriter.write(value)` / `to_string()` | `Unit` / `String` | Write rows and finish the line stream. |

### `core.encoding.csv` — comma-separated rows

`core.encoding.csv` exposes physical CSV rows and a tree representation. A
`CSVRow` records its physical line and fields. `csv.rows` does not discard the
first row when `header` is false; `skip_blank` only skips blank physical rows.
The query helper is filesystem-only and supports `SELECT *` and
`SELECT COUNT(*)`.

| Function or type | Returns | Description |
| --- | --- | --- |
| `csv.rows(text, header, skip_blank)` | `[CSVRow] !EncodingError` | Parse physical rows with explicit header and blank-line policy. |
| `csv.parse(text)` / `decode(text)` | `DataTree !EncodingError` | Parse rows as an array of arrays. |
| `csv.to_string(rows)` | `String` | Encode `CSVRow` values. |
| `csv.reader(text)` / `csv.writer()` | `CSVReader` / `CSVWriter` | Construct incremental CSV objects. |
| `CSVReader.next()` | `?CSVRow` | Return the next physical row. |
| `CSVWriter.write_row(fields)` / `to_string()` | `Unit` / `String` | Write a string-field row and finish the CSV value. |
| `csv.query(path, sql)` | `DataTree !EncodingError -[FS]>` | Run the bounded `SELECT *` or `SELECT COUNT(*)` filesystem query. |
| `CSVRow` | type | Physical line number and parsed string fields. |

### `core.encoding.toml` — TOML values

`core.encoding.toml` supports the common TOML 1.0 document forms: comments,
dotted keys, tables, array tables, strings, integers, floats, booleans,
inline tables, and arrays. TOML datetimes remain text in `DataTree`. Unsupported
syntax fails closed, and output is minimal rather than comment-preserving.

| Function | Returns | Description |
| --- | --- | --- |
| `toml.parse(text)` / `decode(text)` / `loads(text)` / `load(text)` | `DataTree !EncodingError` | Parse a TOML document or a compatibility alias. |
| `toml.to_string(value)` | `String` | Emit a minimal TOML document. |

### `core.encoding.yaml` — bounded YAML input

`core.encoding.yaml` accepts JSON-compatible YAML 1.2 plus simple block maps,
lists, comments, and `|` or `>` block scalars. Unknown or unsupported syntax
fails closed. This module is an input parser and does not expose a YAML
serializer.

| Function | Returns | Description |
| --- | --- | --- |
| `yaml.parse(text)` / `decode(text)` | `DataTree !EncodingError` | Parse supported YAML input. |

### `core.encoding.xml` — bounded XML trees

`core.encoding.xml` parses well-formed XML 1.0 without DTDs into ordered trees
with `name`, `attrs`, and `children`. Only predefined and numeric entities are
accepted. Resource limits apply to depth and input size. `canonical` means
parse followed by this module's deterministic serializer; it is not W3C XML
Canonicalization.
| Signature | Result | Description |
| --- | --- | --- |
| `xml.parse(text)` / `decode(text)` / `parse_bytes(bytes)` | `DataTree !EncodingError` | Parse XML text or bytes without DTD processing. |
| `xml.parse_with(text, limits)` | `DataTree !EncodingError` | Parse with explicit bounded limits. |
| `xml.to_string(tree)` / `to_bytes(tree)` | `String` / `[U8]` | Serialize an XML tree. |
| `xml.canonical(text)` | `String !EncodingError` | Parse and produce deterministic module serialization, not W3C C14N. |
| `xml.reader(text)` | `XMLReader !EncodingError` | Construct an incremental XML reader. |
| `xml.attribute(name, value)` / `content(text)` / `expanded_name(name)` / `root(tree)` | `DataTree` or `String` | Construct or inspect XML attributes, text content, expanded names, or root. |
| `xml.XMLWriter.write(value)` / `to_string()` / `flush()` / `finish()` | varies | Build and finish an XML stream. |

### `core.encoding.cbor` — definite-length CBOR

`core.encoding.cbor` handles RFC 8949 definite-length major types `0` through
`5` and common simple and float64 values. It rejects indefinite-length items,
tags, half and single floats, and byte strings that cannot be represented in
`DataTree`. Canonical output sorts map keys by encoded byte order.

| Function or type | Returns | Description |
| --- | --- | --- |
| `cbor.parse(bytes)` / `decode(bytes)` | `DataTree !EncodingError` | Parse supported definite-length CBOR. |
| `cbor.to_bytes(value)` / `to_bytes_canonical(value)` | `[U8]` | Encode ordinary or canonical CBOR. |
| `cbor.reader(bytes)` / `writer()` | `CBORReader` / `CBORWriter` | Construct byte reader or writer carriers. |
| `CBORWriter.write(value)` / `write_canonical(value)` / `to_bytes()` | `Unit` / `[U8]` | Append ordinary or canonical values and finish the byte stream. |

### `core.encoding.hex` — hexadecimal bytes

`core.encoding.hex` emits lowercase hexadecimal without prefixes or separators.
Decoding accepts mixed case but rejects whitespace, odd length, and other
non-hex characters.

| Function | Returns | Description |
| --- | --- | --- |
| `hex.encode(bytes)` / `decode(text)` | `String` / `[U8] !HexError` | Encode or decode hexadecimal. |
| `hex.encode_upper(bytes)` / `encode_prefixed(bytes)` | `String` | Emit uppercase or `0x`-prefixed hexadecimal. |
| `hex.is_hex(text)` | `Bool` | Test whether text is valid hexadecimal. |
| `hex.hexlify` / `unhexlify` / `b2a_hex` / `a2b_hex` | varies | Compatibility aliases for byte-to-hex and hex-to-byte operations. |
| `hex.crc_hqx(bytes, value)` / `crc32(bytes)` | `Int` | Compute CRC-CCITT or CRC-32. |

### `core.encoding.base32` — RFC 4648 base32

`core.encoding.base32` uses the RFC 4648 canonical uppercase padded alphabet.
Whitespace, aliases, and nonzero unused bits are rejected. The hexadecimal
alphabet is exposed separately.

| Function | Returns | Description |
| --- | --- | --- |
| `base32.encode(bytes)` / `decode(text)` | `String` / `[U8] !Base32Error` | Encode or decode canonical padded base32. |
| `base32.is_base32(text)` | `Bool` | Validate canonical base32 text. |
| `base32.b32encode` / `b32decode` | varies | Compatibility aliases for standard base32. |
| `base32.b32hexencode` / `b32hexdecode` | varies | Use the base32 hexadecimal alphabet. |

### `core.encoding.base64` — standard and URL-safe base64

`core.encoding.base64` supports standard padded and URL-safe unpadded forms.
Whitespace, mixed alphabets, invalid padding, and nonzero unused bits are
rejected.

| Function | Returns | Description |
| --- | --- | --- |
| `base64.encode(bytes)` / `decode(text)` | `String` / `[U8] !Base64Error` | Encode or decode standard padded base64. |
| `base64.encode_url(bytes)` / `decode_url(text)` | `String` / `[U8] !Base64Error` | Encode or decode URL-safe unpadded base64. |
| `base64.b64encode` / `b64decode` / `standard_b64encode` / `standard_b64decode` | varies | Compatibility aliases for standard operations. |
| `base64.encodebytes` / `decodebytes` / `b2a_base64` / `a2b_base64` | varies | Byte-wrapped or line-wrapped base64 forms. |

### `core.encoding.binary` — checked binary packing

`core.encoding.binary` packs and unpacks fixed-width values from byte buffers.
Its format grammar accepts optional `<` little-endian or `>`/`!` big-endian
prefixes, repeat counts, and the `x`, `c`, `b`, `B`, `h`, `H`, `i`, `l`, `I`,
`L`, `q`, and `Q` integer codes. Native-alignment `@` is not part of this
portable grammar. The source is
[`Core/encoding/binary.jet`](../../../Core/encoding/binary.jet).

| Signature | Result | Description |
|---|---|---|
| `pack_u8(n: Int) -> [U8] !BinaryError` / `pack_i8(n: Int) -> [U8] !BinaryError` | bytes | Pack checked 8-bit values. |
| `pack_u16le(n: Int)`, `pack_u16be(n: Int) -> [U8] !BinaryError` | bytes | Pack checked 16-bit values in either byte order. |
| `pack_u32le(n: Int)`, `pack_u32be(n: Int) -> [U8] !BinaryError` | bytes | Pack checked 32-bit values in either byte order. |
| `pack_u64le(n: Int)`, `pack_u64be(n: Int) -> [U8] !BinaryError` | bytes | Pack checked 64-bit values in either byte order. |
| `unpack_u8(data: [U8], offset: Int) -> ?Int` | option | Read an unsigned byte at an offset. |
| `unpack_u16le(data, offset) -> ?Int` / `unpack_u16be(data, offset) -> ?Int` | option | Read a 16-bit integer. |
| `unpack_u32le(data, offset) -> ?Int` / `unpack_u32be(data, offset) -> ?Int` | option | Read a 32-bit integer. |
| `unpack_u64le(data, offset) -> ?Int` / `unpack_u64be(data, offset) -> ?Int` | option | Read a 64-bit integer. |
| `pack_f64le(n: Float) -> [U8] !BinaryError` / `pack_f64be(n: Float) -> [U8] !BinaryError` | bytes | Pack IEEE-754 64-bit bits in either order. |
| `unpack_f64le(data, offset) -> ?Float` / `unpack_f64be(data, offset) -> ?Float` | option | Decode IEEE-754 64-bit bits. |
| `calcsize(fmt: String) -> Int !BinaryError` | size | Calculate a format's byte width. |
| `pack(fmt: String, values: [Int]) -> [U8] !BinaryError` | bytes | Pack values under the portable format grammar. |
| `unpack(fmt: String, data: [U8]) -> [Int] !BinaryError` | values | Decode one record and reject short or malformed input. |
| `iter_unpack(fmt: String, data: [U8]) -> [[Int]] !BinaryError` | rows | Decode repeated records. |
| `sign_extend(n: Int, bits: Int) -> Int` | integer | Sign-extend a fixed-width integer. |

`BinaryError` distinguishes range, format, value, and buffer failures. Use the
consuming `Reader` for a cursor over an existing byte buffer; use this module
when the wire format itself is the primary description.

### `core.encoding.ini` — ordered INI documents

`core.encoding.ini` parses ordered sections and key/value pairs. Both `#` and
`;` introduce comments, duplicate keys use the last value, and interpolation
is not performed. `Ini`, `Section`, and `Pair` retain the document structure.

| Function or type | Returns | Description |
| --- | --- | --- |
| `ini.parse(text)` | `Ini !INIError` | Parse an ordered INI document. |
| `ini.to_string(value)` | `String` | Serialize an INI value. |
| `ini.sections(doc)` / `has_section(doc, name)` / `has_option(doc, section, key)` | varies | Inspect section names and option presence. |
| `ini.get(doc, section, key)` / `get_or(doc, section, key, fallback)` | `?String` / `String` | Read a value with or without a fallback. |
| `ini.get_int(doc, section, key)` / `get_bool` / `get_float` | typed optional | Parse a typed value. |
| `ini.set(doc, section, key, value)` / `remove_option(doc, section, key)` | `Ini` | Set or remove a key while retaining section order. |
| `ini.items(doc, section)` / `options(doc, section)` | `[Pair]` / `[String]` | List pairs or option names. |
| `Ini`, `Section`, `Pair` | types | Ordered document, section, and pair carriers. |

### `core.data` — typed readers and queries

`core.data` combines typed source loaders, bounded materialization, query
plans, statistics, and text/SVG renderers. A loader owns source identity,
authority, freshness, invalidation, and an optional last-good snapshot;
`Query<T>` owns a deterministic row operation plan. Data errors carry a kind,
operation, location, reason, and optional cause rather than silently falling
back to another provider.

```jet
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

fn run() {
    rows :: data.csv<Ticket>("team,minutes\nCore,4.0\nCore,8.0\nTools,5.0") ?? return
    counts :: data.query(rows)
        .group_by(t -> t.team)
        .count()
        .collect() ?? return
    chart :: data.bar_text(counts) ?? return
    print(chart)
}
```

| Function or type | Returns | Description |
| --- | --- | --- |
| `DataErrorKind` | type | Enumerates `Decode`, `Limit`, `IO`, `Empty`, `InvalidArgument`, `NonFinite`, `Overflow`, `State`, `Bridge`, `Unsupported`, `DuplicateKey`, `MissingKey`, `WrongOwner`, `StaleRevision`, and `InvalidValue`. |
| `DataError` | type | Carries kind, operation, row/column/index location, reason, and cause. |
| `DataLimits.safe()` | `DataLimits` | Uses `EncodingLimits.safe()`, `max_groups=100000`, `max_sort_rows=1000000`, `max_join_rows=1000000`, and `max_output_rows=1000000`. |
| `DataLimits` | type | Bounds encoding, groups, sorting, joins, and output materialization. |
| `DataFreshness` / `DataInvalidationCause` | types | Describe pending/fresh/stale/error/offline/cancelled state and its invalidation cause. |
| `DataStatus` | type | Reports `step`, `path`, `clone_value`, `ownership`, `trust`, `fallback`, and `replacement`. |
| `DataSourceIdentity` / `DataSnapshotIdentity` / `DataProvenance` / `DataSchema` | types | Describe source identity, snapshot identity, provenance, and schema. |
| `DataSnapshot<T>` / `DataLoader<T>` / `DataLoaderStatus` / `DataWatchStatus` | types | Own typed snapshots and loader/watch lifecycle state. |
| `DataStream<T>` | type | One-shot bounded cursor over typed rows. |
| `DataJoin<L, R>` / `Group<K, V>` | types | Join rows and grouped reduction rows; a left join uses `?R`. |
| `Query<T>` / `DataTracked<T, K>` / `DataWatch<T>` | types | Query operation carrier, maintained keyed state, and update watcher. |
| `track<T, K>(rows, row -> key)` | `DataTracked<T, K> !DataError` | Attach maintained keyed state to owned rows. |
| `file<T>(path, format, limits)` / `file_member<T>(path, member, format, limits)` | `DataLoader<T> !DataError` | Build a typed file or archive-member loader. |
| `url<T>(url, format, authority, limits)` / `database<T>(query, parameters, authority, limits)` | `DataLoader<T> !DataError` | Build an authority-bound remote or SQL loader. |
| `value<T>(value, limits)` / `load_default<T>(locator)` / `load<T>(locator, limits)` | `DataLoader<T> !DataError` | Load an owned value or source locator with explicit limits. |
| `csv<T>(text)` / `json<T>(text)` | `[T] !DataError` | Decode typed CSV or JSON input. |
| `csv_reader<T>(reader, limits)` / `json_reader<T>(reader, limits)` | `DataStream<T> !DataError` | Decode a typed reader over a `FileReader` under limits. |
| `snapshot(loader)` | `DataSnapshot<T> !DataError -[FS, Net, DB]>` | Materialize a source snapshot. |
| `schema(rows)` | `[DataColumn]` | Inspect the columns of typed rows. |
| `count(value)` | `Int` | Count rows in a list or materialized query result. |
| `describe(values: [Float])` | `DataSummary !DataError` | Produce bounded descriptive statistics. |
| `sum(values)` / `mean(values)` | `Float !DataError` | Reduce finite numeric values. |
| `min(values)` / `max(values)` | `Float !DataError` | Select finite numeric extrema. |
| `variance(values)` / `stddev(values)` | `Float !DataError` | Compute finite numeric dispersion. |
| `median(values)` / `quantile(values, q)` | `Float !DataError` | Compute bounded order statistics. |
| `rolling_mean(values, window)` | `[Float] !DataError` | Compute a bounded rolling mean. |
| `query(rows)` | `Query<T>` | Start a deterministic query plan. |
| `inner_join(left, right, left -> key, right -> key)` | `[DataJoin<T, U>] !DataError` | Preserve duplicate matches. |
| `left_join(left, right, left -> key, right -> key)` | `[DataJoin<T, ?U>] !DataError` | Retain unmatched left rows as `?U`. |
| `pivot_sum(rows, row -> key, row -> column, row -> value)` | `[DataPivotCell] !DataError` | Group and sum by a row and column key. |
| `plot<T>(rows)` | `JetDataPlot<T> !DataError` | Build a bounded plot plan from encodable rows. |
| `inspect(plot)` / `inspect_json(plot)` | inspection / `String !DataError` | Inspect a plot plan as text or JSON. |
| `text(plot)` / `svg(plot)` | `String !DataError` | Render plot text or SVG. |
| `show(plot)` / `render(plot, backend)` | `JetDataPlotRender !DataError` | Render through the selected backend. |
| `bar_text(groups)` / `bar_svg(groups)` | `String !DataError` | Render `Group<String, Int>` bars as text or SVG. |
| `line_text(groups, options)` / `line_svg(groups, options)` | `String !DataError` | Render `Group<String, Float>` lines with `DataLineOptions`. |
| `status()` | `[DataStatus]` | Report native and bridge path, ownership, trust, fallback, and replacement facts. |
| `require_bridge(provider)` | `Unit !DataError` | Fail closed when a requested `py`, `r`, or `gpu` bridge is unavailable. |

Query methods filter, sort, map, extrema, joins, grouping, collection, and
watching while retaining typed positions for errors. `Query<T>.plan()` returns
deterministic operation names; `collect()` materializes `[T] !DataError`.
Grouped `count`, `sum`, and `mean` return `Query<Group<K, V>>`, and `mean`
requires a floating numeric selector. `DataTracked<T, K>` supports `query`,
`insert`, `replace`, and `remove`; `DataWatch<T>` supports `get`, `status`, and
`cancel` (D-QUERY-RETAIN1).
The bounded SQL kernel accepts `SELECT *` or explicit fields, the aggregates
`COUNT`, `SUM`, `AVG`, `MIN`, and `MAX`, and optional `WHERE`, `GROUP BY`,
`ORDER BY`, and `LIMIT` clauses. It validates projected and grouped fields
against the inferred schema before materializing a result (D-SQL-SURFACE1).

`DataStatus.clone_value` is the copy-related field. Bridge rows remain separate
from native rows, and `require_bridge` returns `DataErrorKind.Bridge` instead
of silently substituting a native or unavailable provider (D-DATA-BRIDGE1).

### `core.data.loader` — source lifecycle

`core.data.loader` binds a provider payload to a typed loader. Supported source
identities include URL, database, archive-member, and file payloads. Binding
checks limits before publishing a pending payload; a loader may retain its
last-good snapshot for an explicit offline read. Cancellation and invalidation
are terminal or revisioned lifecycle operations, not ordinary query filters.

| Function | Returns | Description |
| --- | --- | --- |
| `loader.bind<T>(target, payload)` / `bind_text<T>(target, text)` | `Unit !DataError` | Bind provider bytes or text to an existing loader under its limits. |
| `loader.authority(loader)` | `DataAuthority` | Inspect the loader authority. |
| `loader.status(loader)` | `DataLoaderStatus` | Inspect freshness, source, snapshot, and error state. |
| `loader.ready(loader)` | `Bool` | Test whether a usable snapshot is available. |
| `loader.needs_refresh(loader)` | `Bool` | Test refresh state. |
| `loader.snapshot_reusable(previous, current)` | `Bool` | Compare source, content, schema, format, and snapshot identities. |
| `loader.source_identity(loader)` | `DataSourceIdentity` | Inspect the source identity. |
| `loader.cancel(loader)` | `Unit` | Cancel the loader operation. |
| `loader.offline(loader, enabled)` | `Unit` | Enable or disable offline reads from a retained last-good snapshot. |
| `loader.invalidate(loader, cause)` | `Unit` | Invalidate a snapshot with a typed cause. |
| `loader.stream<T>(loader)` | `DataStream<T> !DataError` | Open a bounded stream; no payload or cancellation is an error. |

The loader stream supports CSV, JSON, and JSONL payloads subject to
`DataLimits.max_output_rows`; unsupported formats and unavailable providers
return structured errors.

### `core.data.stream` — one-shot cursors

`core.data.stream` is a one-shot cursor. `next` returns each row once, then
stable `None` at end of input. A terminal error remains latched; cancelling a
stream is terminal and does not resume iteration.

| Function | Returns | Description |
| --- | --- | --- |
| `stream.from_items(items)` | `DataStream<T>` | Construct a cursor over owned items. |
| `stream.next(stream)` | `?T !DataError` | Return the next item, stable `None` at EOF. |
| `stream.collect(stream)` | `[T] !DataError` | Consume and materialize the remaining rows. |
| `stream.cancel(stream)` | `Unit` | Cancel the cursor and latch its terminal state. |
| `stream.skip(stream, n)` | `Unit` | Consume up to `n` rows. |
| `stream.take_n(stream, n)` | `[T] !DataError` | Consume at most `n` rows. |
| `stream.len(stream)` / `is_empty(stream)` | `Int` / `Bool` | Inspect remaining rows without changing row ownership. |

### `core.data.plot` — deterministic data plots

`core.data.plot` turns aligned labels and numeric values into a deterministic
plan and renders that plan as text or SVG. The source implementation is a
portable inspection renderer: it does not require a GPU, and it makes missing
values explicit as zero. The module contract is defined in
[`Core/data/plot.jet`](../../../Core/data/plot.jet) and exercised by the plot
corpus under [`tests/conformance/corpus/core/data/plot/`](../../../tests/conformance/corpus/core/data/plot/).


A plot aligns values by position with `labels`. If there are fewer values than
labels, the missing positions render as zero; values beyond the label count are
not rendered. `plot` builds a `JetDataPlotPlan` with a 320-pixel width, a
height based on the number of labels, and a text-number schema. `bar_*` and
`line_*` helpers are convenient renderers; `render` produces an SVG record
with its `format` and `body`.

| Signature | Result | Description |
|---|---|---|
| `plot(mark: JetDataPlotMark, labels: [String], values: [Float]) -> JetDataPlotPlan` | plan | Build a bar, line, or point plan. |
| `bar_text(labels: [String], values: [Int]) -> String` | text | Render aligned integer values as a text bar chart. |
| `bar_svg(labels: [String], values: [Int]) -> String` | SVG text | Render the same bar data as SVG. |
| `line_text(labels: [String], values: [Int]) -> String` | text | Render aligned integer values as a text line chart. |
| `line_svg(labels: [String], values: [Int]) -> String` | SVG text | Render the same line data as SVG. |
| `inspect(labels: [String], values: [Int]) -> String` | text | Inspect integer data using the text-bar renderer. |
| `inspect_json(plan: JetDataPlotPlan) -> String` | JSON text | Serialize the plan for inspection. |
| `render(plan: JetDataPlotPlan) -> JetDataPlotRender` | render | Produce a render record with `format` and `body`. |
| `svg(plan: JetDataPlotPlan) -> String` | SVG text | Return the plan's SVG body. |
| `text(plan: JetDataPlotPlan) -> String` | text | Return the plan's text body. |
| `show(plan: JetDataPlotPlan) -> String` | text | Select the inspectable display body. |


### `core.text.fmt` — explicit text and number formatting

`core.text.fmt` provides deterministic formatting without locale-dependent
output. Radix digits are lower-case. Padding widths count Unicode scalar
values, not bytes; binary-size formatting uses binary units.

| Signature | Result | Description |
|---|---|---|
| `number(value: Int) -> String` | text | Format an integer with grouping separators. |
| `bin(value: Int) -> String` | text | Format an integer in base 2. |
| `oct(value: Int) -> String` | text | Format an integer in base 8. |
| `hex(value: Int, width: Int) -> String` | text | Format lower-case hexadecimal with the requested width. |
| `ordinal(value: Int) -> String` | text | Add an English ordinal suffix. |
| `plural(value: Int, singular: String, many: String) -> String` | text | Choose a singular or plural word from the value. |
| `pad_left(text: String, width: Int, fill: String) -> String` | text | Pad on the left. |
| `pad_right(text: String, width: Int, fill: String) -> String` | text | Pad on the right. |
| `pad_center(text: String, width: Int, fill: String) -> String` | text | Pad on both sides. |
| `pad(text: String, width: Int, fill: String) -> String` | text | Apply the module's default padding direction. |
| `grouped(value: Float, frac: Int) -> String` | text | Add grouping separators and keep `frac` fractional digits. |
| `decimal(value: Float, frac: Int) -> String` | text | Format fixed-point decimal output. |
| `percent(value: Float, frac: Int) -> String` | text | Format a percentage with `frac` fractional digits. |
| `sci(value: Float, frac: Int) -> String` | text | Format scientific notation. |
| `pretty(value: String) -> String` | text | Expand braces, brackets, and commas into an indented display. |
| `bytes(value: Int) -> String` | text | Format bytes with `B`, `KiB`, `MiB`, `GiB`, or `TiB`. |
| `duration(milliseconds: Int) -> String` | text | Format a duration using at most three nonzero units. |

The implementation is in [`Core/text/fmt.jet`](../../../Core/text/fmt.jet).
`bytes` deliberately uses powers-of-two units, and `duration` does not emit
unbounded trailing zero units.
The library-call boundary is deliberate: ordinary human formatting remains explicit (`D-HUMANFMT1`), and `pretty` follows the expanded deterministic Debug shape (`D-FMT-PRETTY1`).

### `core.log` — structured logging

`core.log` writes structured records through a configured sink. Levels are
`debug`, `info`, `warn`, `error`, `critical`, and `fatal`; `warning` is an
alias for `warn`. A record can carry typed fields and a redaction marker. The
package source is [`Core/log/log.jet`](../../../Core/log/log.jet).

| Signature | Result | Description |
|---|---|---|
| `debug(message: String) -[Log]>` / `info(message: String) -[Log]>` / `warn(message: String) -[Log]>` | — | Emit a record at the named level. |
| `error(message: String) -[Log]>` / `critical(message: String) -[Log]>` / `fatal(message: String) -[Log]>` | — | Emit a high-severity record. |
| `warning(message: String) -[Log]>` | — | Use the Python-compatible warning spelling for `warn`. |
| `log(level: String, message: String) -[Log]>` | — | Emit at a caller-selected level. |
| `debug_fields(message: String, fields: [LogField]) -[Log]>` | — | Emit debug data with structured fields. |
| `info_fields(message: String, fields: [LogField]) -[Log]>` | — | Emit info data with structured fields. |
| `warn_fields(message: String, fields: [LogField]) -[Log]>` | — | Emit warning data with structured fields. |
| `error_fields(message: String, fields: [LogField]) -[Log]>` | — | Emit error data with structured fields. |
| `field(key: String, value: String) -> LogField` | field | Create a string field. |
| `int(key: String, value: Int) -> LogField` | field | Create an integer field. |
| `float(key: String, value: Float) -> LogField` | field | Create a floating-point field. |
| `bool(key: String, value: Bool) -> LogField` | field | Create a Boolean field. |
| `redact(key: String) -> LogField` | field | Mark a key as redacted. |
| `span(name: String) -[Log]> LogSpan` | span | Open a named span. |
| `enter(span: LogSpan) -[Log]>` / `close(span: LogSpan) -[Log]>` | — | Enter or close a span. |
| `set_sink(kind: String, path: String) -[Log]>` / `set_level(level: String) -[Log]>` | — | Configure output and filtering. |
| `sample_every(n: Int) -[Log]>` | — | Sample every positive number of records. |
| `counter(name: String, value: Int) -> LogField` | field | Build a counter field. |
| `otlp_file(path: String) -[Log, FS]>` | — | Select a file sink for OTLP output. |
| `set_trace_id(id: String) -[Log]>` | — | Attach a trace identifier to later records. |
| `setup(spec: String) -[Log]>` | — | Apply comma-separated `key=value` settings. |
| `enabled(level: String) -[Log]> Bool` | Boolean | Test whether a level is enabled. |
| `flush() -[Log]>` / `disable() -[Log]>` | — | Flush records or disable logging. |

Sink names are normalized to `stderr`, `stdout`, `json`, `jsonl`, or `text`.
`jsonl` and `text` sinks require a path. Keep secrets in `redact` fields so
the sink, rather than each caller, owns the redaction decision.
`fatal` emits, flushes, and terminates the process with status 1; `disable`
suppresses later emission until process end.

### `core.tasks` — cooperative tasks, channels, and timers

`core.tasks` supplies cooperative scheduling primitives. A task yields at
explicit suspension points; the scheduler does not turn a blocking computation
into preemptive parallelism. Mutable task-owned values stay with their task;
values crossing a task or channel boundary must satisfy Jet's sendability
rules. The implementation and exported helper surface are in
[`Core/tasks/tasks.jet`](../../../Core/tasks/tasks.jet).

| Signature | Result | Description |
|---|---|---|
| `after(delay: Duration, value: Int{0}) -[Time]> Receiver<Int>` | receiver | Deliver one value after a duration. |
| `interval(period: Duration) -[Time]> Receiver<Int>` | receiver | Deliver successive tick counts starting at one. |
| `yield_now() -[Time]>` | — | Yield to another runnable task. |
| `current_task() -> String` | text | Identify the running task. |
| `sleep(milliseconds: Int) -[Time]>` | — | Suspend for a nonnegative duration. |
| `timeout(delay: Duration) -[Time]>` | — | Wait for a duration before returning. |
| `try_recv(rx: Receiver<Int>) -> ?Int` | option | Poll a scheduler receiver without waiting. |
| `cancel(rx: Receiver<Int>) -[Time]> Receiver<Int>` | receiver | Close a receiver and wake blocked consumers. |
| `is_timer(rx)`, `is_interval(rx)`, `is_cancelled(rx)`, `is_ready(rx)` | `Bool` | Inspect scheduler-receiver state. |
| `delay_ms(rx) -> Int` | integer | Report a timer delay. |
| `channel(capacity: Int) -> TaskState` | state | Create a pure integer queue with explicit capacity. |
| `put(state: TaskState, value: Int) -> TaskState` | state | Enqueue unless the state is closed or full. |
| `clear(state: TaskState)`, `shutdown(state: TaskState)`, `stop(state)` | state | Clear values or close the pure queue. |
| `reset(state: TaskState) -> TaskState` | state | Clear values and reopen the pure queue. |
| `lock() -> TaskState` / `acquire(state)` / `release(state)` / `notify(state)` | state | Coordinate the pure queue's lock fields. |
| `start(rx: Receiver<Int>) -> Receiver<Int>` / `run() -[Time]>` | receiver/— | Start a receiver or yield through the scheduler. |
| `recv(rx: Receiver<Int>) -[Time]> Int`, `get(rx) -[Time]> Int`, `result(rx) -[Time]> Int`, `wait(rx) -[Time]> Int` | integer | Receive an integer, using `-1` for a closed receiver. |
| `waitall(receivers: [Receiver<Int>]) -[Time]> [Int]` / `waitany(receivers: [Receiver<Int>]) -[Time]> Int` | values | Wait for all or the first convenience receiver. |
| `is_closed(state)`, `is_locked(state)`, `size(state)`, `capacity(state)`, `generation(state)` | scalar | Inspect `TaskState`. |
| `spawn_name(name: String) -> String` | text | Preserve a task name for the scheduler boundary. |

`TaskState` is the pure integer convenience queue; generic task and channel
handles are compiler-owned scheduler surfaces. A capacity of zero means that
the convenience queue has no finite limit. `put` returns the unchanged state
when it is closed or at capacity, so callers that require backpressure must use
the scheduler receiver surface. Cancellation closes the receiver and leaves
its handle available for identity and ownership.
Generic channels return `(Sender<T>, Receiver<T>)`; tasks send owned values and do not use `async`/`await` (`D-CONC-CHAN1`; `D-SHAPE-COPY1`). Cancellation and pause/resume are control-plane requests on owned task handles (`D-COROUTINE1`), while shared memory uses `Shared<T>` guards and `Condition` rather than a raw lock (`D-SHAREDGUARD1`).

### `core.prelude` — small compiler-owned helpers

`core.prelude` is available without a separate package import. Its functions
are deliberately small barriers and predicates used by generic code and
constant evaluation; they are not alternate syntax for bindings. See
[`Core/prelude/prelude.jet`](../../../Core/prelude/prelude.jet).

| Signature | Result | Description |
|---|---|---|
| `keep<T>(value: T) -> T` | `T` | Preserve a value at an explicit use site. |
| `identity(value: ^String) -> String` | text | Return a borrowed string unchanged. |
| `always(value: Bool) -> Bool` | Boolean | Return a Boolean unchanged. |
| `identity_int(value: Int) -> Int`, `identity_float(value: Float) -> Float`, `identity_bool(value: Bool) -> Bool` | same type | Typed identity helpers for primitive values. |
| `const_int(value: Int, ignored: Int) -> Int` | integer | Select the first integer in a constant-evaluation expression. |
| `const_bool(value: Bool, ignored: Bool) -> Bool` | Boolean | Select the first Boolean in a constant-evaluation expression. |
| `not_bool(value: Bool) -> Bool` | Boolean | Negate a Boolean. |
| `min_int(a: Int, b: Int) -> Int` / `max_int(a: Int, b: Int) -> Int` | integer | Select the smaller or larger integer. |
`keep` is the approved use-site identity sink when measurement must observe a value (`D-BENCH-KEEP1=A`).

### `core.testing` — deterministic test helpers

`core.testing` provides deterministic values and comparison helpers for tests;
it does not replace `jet test`'s test discovery. A test can use a fake clock,
fake random source, fake data, temporary directory, fixture, corpus, snapshot,
or golden file. The public source is
[`Core/testing/testing.jet`](../../../Core/testing/testing.jet).

| Signature | Result | Description |
|---|---|---|
| `assert_equal(comparison: TestComparison) -> Bool` | Boolean | Check the comparison's recorded proof fields. |
| `compare(cases: [DataTree], reference: fn(DataTree) -> DataTree, candidate: fn(DataTree) -> DataTree, relation: String) -> TestComparison` | comparison | Compare reference and candidate outputs over explicit cases. |
| `snap(name: String, value: String) -[FS]> Bool` | Boolean | Check a named snapshot. |
| `golden(name: String, value: String) -[FS]> Bool` | Boolean | Check a named golden value. |
| `fixture(name: String) -[FS]> String` | text | Load a named fixture. |
| `temp_dir(prefix: String) -[FS]> String` | path | Allocate an isolated temporary directory. |
| `corpus(glob: String) -[FS]> [String]` | paths | Select files matching a corpus glob. |
| `fake_clock(unix_ms: Int) -> Clock` | clock | Create a deterministic clock at a Unix-millisecond value. |
| `fake_rng(seed: Int) -> Rng` | RNG | Create deterministic random state. |
| `fake_data(seed: Int) -> Fake` | fake | Create a deterministic fake-data carrier. |
| `test_suite() -> TestSuite` | suite | Create the default suite record. |
| `world(body: fn(DeterministicWorld))` | — | Run a body with deterministic world services. |
| `status(comparison: TestComparison) -> String` | text | Read the comparison status. |

`#Test` is the only test declaration syntax (`D-TESTKIT1`).
Use `#Test` for test declarations and keep assertions about behavior rather
than implementation details. The compiler and test runner own test discovery;
the helper types only control inputs and observations.

### `core.regex` — bounded regular expressions

`core.regex` is a UTF-8 pattern engine with an explicit finite execution
budget. It supports concatenation, alternation, groups, `*`, `+`, `?`, bounded
repetition, `.`, `^`, `$`, and the `\d`, `\w`, and `\s` classes. Flags include
ASCII `ignore_case`, `multiline`, and `dot_matches_newline`. A malformed
pattern is a typed `RegexError`; a match operation cannot run without a finite
budget. The implementation is in
[`Core/regex/regex.jet`](../../../Core/regex/regex.jet).

The budget is an execution bound, not a promise of linear-time matching. The
engine does not expose named capture groups: `Match.groups` is a positional
list. Use the compiled `Pattern`/`Regex` values when the same expression is
used repeatedly, and call `escape` for literal user input.

| Signature | Result | Description |
|---|---|---|
| `flags(ignore_case: Bool, multiline: Bool, dot_matches_newline: Bool) -> RegexFlags` | flags | Build explicit compilation flags. |
| `compile(pattern: String) -> Regex !RegexError` | regex | Compile a pattern with default flags. |
| `compile_with(pattern: String, flags: RegexFlags) -> Regex !RegexError` | regex | Compile with explicit flags. |
| `escape(text: String) -> String` | text | Escape regex metacharacters. |
| `is_match(pattern: Regex, text: String) -> Bool` | Boolean | Test whether any match exists. |
| `full_match(pattern: Regex, text: String) -> Bool` | Boolean | Require the entire text to match. |
| `match(pattern: Regex, text: String) -> ?Match` | match | Return the first positional match record. |
| `find(pattern: Regex, text: String) -> ?String` | text | Return the first matched text. |
| `find_all(pattern: Regex, text: String) -> [String]` | text list | Return all matched text values. |
| `finditer(pattern: Regex, text: String) -> [Match]` | matches | Return positional match records. |
| `matches(pattern: Regex, text: String) -> [Match]` | matches | Return all positional match records. |
| `split(pattern: Regex, text: String) -> [String]` | text list | Split on matches. |
| `split_limit(pattern: Regex, text: String, n: Int) -> [String]` | text list | Split with a maximum number of matches. |
| `replace(pattern: Regex, text: String, replacement: String) -> String` | text | Replace every match. |
| `replace_first(pattern: Regex, text: String, replacement: String) -> String` | text | Replace only the first match. |
| `expand(match: Match, template: String) -> String` | text | Expand numbered capture references. |
| `purge() !RegexError` | — | Report that the source engine has no mutable cache to purge. |

The provider also exposes the concise names `regex`, `join`, `search`,
`findall`, `fullmatch`, `sub`, and `subn` for code that uses the corresponding
standard-library vocabulary.

### `core.ui` — portable node trees and backends

`core.ui` represents a UI as a typed node tree plus geometry and accessibility
metadata. It provides portable node construction and backend adapters; it does
not pretend that a desktop, phone, or terminal backend has the same input or
rendering capabilities. The source is [`Core/ui/ui.jet`](../../../Core/ui/ui.jet).

| Signature | Result | Description |
|---|---|---|
| `point(x: Float, y: Float) -> Point` | point | Construct a point. |
| `size(width: Float, height: Float) -> Size` | size | Construct a size. |
| `rect(x: Float, y: Float, width: Float, height: Float) -> Rect` | rectangle | Construct a rectangle. |
| `constraint(min_width: Float, min_height: Float, max_width: Float, max_height: Float) -> SizeConstraint` | constraint | Describe allowed layout size. |
| `node(label: String, width: Float, height: Float) -> UiNode` | node | Construct a sized node. |
| `box(children: [UiNode]) -> UiNode` | node | Group child nodes. |
| `text(value: String) -> UiNode` | node | Construct a text node. |
| `preview(name: String, viewport: ?UiPreviewViewport, body: fn() -> UiNode) -> UiPreview` | preview | Describe a named preview. |
| `playground(name: String, viewport: ?UiPreviewViewport, body: fn() -> UiNode) -> UiPreview` | preview | Describe a named interactive playground. |
| `desktop() -> UiPreviewViewport`, `phone() -> UiPreviewViewport`, `tablet() -> UiPreviewViewport` | viewport | Select standard preview dimensions. |
| `key_event(code: String) -> InputEvent` | event | Create a key event. |
| `resize_event(width: Float, height: Float) -> InputEvent` | event | Create a resize event. |
| `reactive_render(body: fn())` | — | Ask the provider to render a reactive body. |
| `gtk_backend() -> GtkBackend`, `tui_backend() -> TuiBackend`, `null_backend() -> NullBackend` | backend | Select a backend handle. |
| `aria_role_button() -> UiAriaRole`, `aria_role_container() -> UiAriaRole` | role | Construct standard accessibility roles. |
| `aria_role_label() -> UiAriaRole`, `aria_role_text_input() -> UiAriaRole` | role | Construct label or text-input roles. |
| `node_accessibility(n: UiNode, metadata: UiAccessibility) -> UiNode` | node | Attach accessibility metadata. |
| `node_color(label: String, width: Float, height: Float, color: String) -> UiNode` | node | Construct a colored node. |
| `node_role(label: String, width: Float, height: Float, role: UiAriaRole) -> UiNode` | node | Construct a node with a role. |
| `node_shortcut(n: UiNode, shortcut: UiShortcut) -> UiNode` | node | Attach a shortcut descriptor. |

Backends expose their own command or event projection through the backend
handle. The provider/catalog may add interactive node and mount operations,
but those operations remain typed provider boundaries. Build a portable tree
first, then use backend-specific capabilities; do not infer that a node tree
supplies a platform-native callback API.

### `core.reactive` — explicit signals and dependency tracking

`core.reactive` provides opt-in, dependency-tracked state. A `Signal<T>` holds
a value and version; `computed` and `derived` values track reads; an `Effect`
can be unsubscribed. The source carrier is immutable, while the runtime
provider supplies synchronized boxes and dependency tracking. See
[`Core/reactive/reactive.jet`](../../../Core/reactive/reactive.jet) and the
reactive conformance corpus.

| Signature | Result | Description |
|---|---|---|
| `signal<T>(value: T) -> Signal<T>` | signal | Create mutable reactive state. |
| `computed<T>(compute: fn() -> T) -> Computed<T>` | computed | Compute an initial value from a body. |
| `derived<T>(compute: fn() -> T) -> Derived<T>` | derived | Compute an initial derived value. |
| `effect(body: fn()) -> Effect` | effect | Run a body once and retain an active effect record. |
| `get<T>(sig: Signal<T>) -> T` / `version<T>(sig: Signal<T>) -> Int` | value/version | Read a signal and its version. |
| `set<T>(sig: Signal<T>, value: T) -> Signal<T>` | signal | Set a signal value and increment its version. |
| `update(sig: Signal<Int>, delta: Int) -> Signal<Int>` | signal | Add a delta to an integer signal. |
| `computed_get<T>(c: Computed<T>) -> T` / `computed_version<T>(c) -> Int` | value/version | Read a computed value and version. |
| `computed_set<T>(c: Computed<T>, value: T) -> Computed<T>` | computed | Set and version a computed value. |
| `computed_update(c: Computed<Int>, delta: Int) -> Computed<Int>` | computed | Add a delta to an integer computed value. |
| `effect_run(e: Effect, value: Int) -> Effect` / `effect_run_if(e, value: Int) -> Effect` | effect | Record an effect value unconditionally or when changed. |
| `effect_last(e: Effect) -> Int` / `effect_changed(e, value: Int) -> Bool` | scalar | Read or compare an effect's last integer. |
| `unsubscribe(e: Effect) -> Effect` | effect | Return an inactive effect record. |
| `is_active(e: Effect) -> Bool` | Boolean | Test whether an effect is active. |
| `set_if_changed(sig: Signal<Int>, value: Int) -> Signal<Int>` | signal | Avoid an update when an integer value is equal. |
| `changed(sig: Signal<Int>, value: Int) -> Bool` | Boolean | Compare an integer signal with a value. |

Reactive updates are explicit: use `set` or `update`, read through `get`, and
release effects with `unsubscribe`. A derived computation does not become a
hidden global dataflow graph.
Reactivity is an opt-in library rather than ambient language semantics (`D-REACT1`); `computed` is the canonical alias for the derived-value constructor (`D-SIGNAL1`).

### `core.reactive.loadable` — asynchronous value state

`core.reactive.loadable` models a value that is idle, loading, loaded, or
failed. The state carries the successful value or failure reason and provides a
retry transition; callers can inspect it without using sentinel values. Its
source is [`Core/reactive/loadable.jet`](../../../Core/reactive/loadable.jet).

| Signature | Result | Description |
|---|---|---|
| `idle<T, E>() -> Loadable<T, E>` | state | Construct an idle value. |
| `loading<T, E>() -> Loadable<T, E>` | state | Construct a loading value. |
| `loaded<T, E>(value: T) -> Loadable<T, E>` | state | Construct a loaded value. |
| `failed<T, E>(reason: E) -> Loadable<T, E>` | state | Construct a failed value. |
| `is_idle(state)`, `is_loading(state)`, `is_loaded(state)`, `is_failed(state)` | Boolean | Inspect the state variant. |
| `value<T, E>(state) -> ?T` | option | Read a loaded value. |
| `reason<T, E>(state) -> ?E` | option | Read a failure reason. |
| `retry<T, E>(state) -> Loadable<T, E>` | state | Return the retry state. |

### `core.event` — typed synchronous and asynchronous events

`core.event` defines typed event values, scopes, listeners, policies, and
bounded asynchronous delivery. It does not use a stringly named global event
bus. The source contract is [`Core/event/event.jet`](../../../Core/event/event.jet),
and the event conformance corpus covers listener lifetime, policy, and
async-result behavior.

| Signature | Result | Description |
|---|---|---|
| `scope() -> EventScope` | scope | Create the provider-owned event scope. |
| `new<T>() -> Event<T>` | event | Create a typed event. |
| `with_policy<T>(policy: EventPolicy) -> Event<T>` | event | Create an event with an explicit policy. |
| `hook<T, R>(fallback: R) -> Hook<T, R>` | hook | Create a typed hook with a fallback result. |
| `decision_hook<T, E>(policy: HookPolicy) -> DecisionHook<T, E>` | hook | Create a hook with an explicit decision policy. |
| `policy_sync() -> EventPolicy` | policy | Select synchronous delivery policy. |
| `async_result<T, E>(policy: AsyncPolicy, failures: FailurePolicy) -> AsyncEvent<T, E> !EventConfigError` | event | Configure bounded asynchronous delivery. |

Provider event handles expose `.on`, `.emit`, `.listener_count`, and `.close`;
subscriptions expose `.is_active`. Async events expose `.emit_async` and a
joinable result/report. Synchronous policies define registration and delivery
order, including once-only listeners. Asynchronous policies make capacity and
overflow behavior explicit; `Block`, drop, and failure behavior are policy
choices, not implicit scheduler behavior. A scope owns the lifetime of its
subscriptions.
The event family is a typed Core value with no additional event syntax (`D-EVENT1`). Scope ownership, ordered dispatch, explicit cancellation, and bounded asynchronous pressure remain the governing rules (`D-EVENT2=A`; `D-EVENT-CONTINUE1=C`).

### `core.web` — server-rendered applications and live state

`core.web` builds server-rendered applications on top of `core.http`. An
`App` owns routes, live state, sessions, and storage adapters; the browser
surface is represented by typed effects rather than an assumed browser DOM.
The module source is [`Core/web/web.jet`](../../../Core/web/web.jet).

| Signature | Result | Description |
|---|---|---|
| `app() -> App` | app | Create the default application carrier. |
| `page(title: String, body: String) -> WebPage` | page | Build a server-rendered page. |
| `form(input: WebFormInput, action: String) -> WebFormTyped !WebFormError` | form | Convert form input through the typed form boundary. |
| `on(path: String, title: String, handler: fn(WebEvent)) -[Browser]>` | — | Register a browser-facing handler. |
| `openapi(router: HTTPRouter) -> String` | text | Render the router's OpenAPI description. |
| `value(key: String) -[Browser]> String` | text | Read a browser value. |
| `storage() -> String` | text | Select the local storage namespace. |
| `auth(kind: String) -> Auth` | auth | Build an authentication carrier. |
| `auth_oauth(auth: Auth, client_id: String) -> Auth` | auth | Add OAuth client identity to an auth carrier. |
| `auth_routes(auth: Auth) -> [String]` | paths | Return authentication route paths. |
| `auth_show(auth: Auth) -> String` | text | Render the source auth summary. |
| `live(key: String, data: String) -> LiveQuery` | query | Create a live-query carrier. |
| `subscribe(key: String) -> LiveQuery` | query | Create a subscribed live-query carrier. |
| `invalidate(key: String) -> Int` / `transact_invalidate(key: String) -> Int` | token | Create invalidation tokens. |
| `live_get(q: LiveQuery) -> String` / `live_show(q: LiveQuery) -> String` | text | Read or display live-query data. |
| `live_stats() -> String` | text | Read provider live-query statistics. |
| `signal_push(q: LiveQuery, payload: String) -> LiveQuery` | query | Push data and advance query generation. |
| `sync(key: String, data: String) -> String` | text | Format a synchronized key/value update. |

`core.web` delegates HTTP transport to `core.http`; route handlers should use
that typed request/response contract rather than opening sockets directly.
Storage adapters are named below because they are separate built modules.

### `core.web.storage` — browser storage adapters

`core.web.storage` names the storage interface used by web applications, while
`core.web.storage.local` and `core.web.storage.session` provide browser-backed
variants. Values are keyed strings; an adapter owns persistence and scope.

| Signature | Result | Description |
|---|---|---|
| `local() -> WebStorage` | namespace | Select origin-local storage. |
| `session() -> WebStorage` | namespace | Select tab-scoped session storage. |
| `kind_local() -> String` / `kind_session() -> String` | text | Return the namespace labels. |
| `core.web.storage.local.get(key: String) -> ?String` | option | Read an origin-local key. |
| `core.web.storage.local.set(key: String, value: String)` | — | Write an origin-local key. |
| `core.web.storage.local.remove(key: String)` / `clear()` | — | Remove one or all local keys. |
| `core.web.storage.local.get_or(key, fallback) -> String` / `has(key) -> Bool` | text/Boolean | Read with a fallback or test local presence. |
| `core.web.storage.session.get(key: String) -> ?String` | option | Read a session key. |
| `core.web.storage.session.set(key: String, value: String)` | — | Write a session key. |
| `core.web.storage.session.remove(key: String)` / `clear()` | — | Remove one or all session keys. |
| `core.web.storage.session.get_or(key, fallback) -> String` / `has(key) -> Bool` | text/Boolean | Read with a fallback or test session presence. |
### `Cell<T>` — local interior mutability

`Cell<T>` stores private mutable state for one task. It is not an operating
system lock and does not make a value safe to send across tasks, channels, or
parallel adapters; use the appropriate shared or channel abstraction for that
boundary.

| Signature | Result | Description |
|---|---|---|
| `Cell.new(value) -> Cell<T>` | cell | Create a cell and infer `T`. |
| `cell.get() -> T` | value | Copy the current value under the copy law. |
| `cell.set(value)` | — | Replace the current value. |
| `cell.replace(value) -> T` | old value | Replace the value and return the old one. |
| `cell.get_or_set(init) -> T` | value | Initialize an empty cell once. |
| `cell.read(body) -> R` | result | Run a body under one read loan. |
| `cell.edit(body) -> R` | result | Run a body under one exclusive edit loan. |
| `cell.guard_read() -> CellReadGuard<T>` | guard | Keep a read loan across calls. |
| `cell.guard_edit() -> CellEditGuard<T>` | guard | Keep an exclusive edit loan across calls. |
| `guard.map(project)` | guard | Project one field while retaining the loan. |
| `guard.split(first, second)` | guards | Project two disjoint fields under one loan. |

Many read guards can coexist, while an edit guard excludes all other guards.
Mapped guards release the original loan only after their last derived guard is
dropped. A runtime conflict reports `Cell borrow conflict`. Guards can cross a
named helper boundary as local names or tuples, but cannot be stored in a user
struct, enum, list, map, `Option`, `Result`, `Shared`, another `Cell`, union, or
lambda.

### `core.mem` — arenas, regions, and raw memory

`core.mem` contains explicit allocator families and audited raw-memory helpers.
Arenas are the safe fast-allocation primitive; raw pointers and volatile access
are separate unsafe operations. The source is
[`Core/mem/mem.jet`](../../../Core/mem/mem.jet).

An allocation view borrows its allocator's storage. It cannot be returned,
stored, sent, or used after the allocator resets or closes: the checker reports
**E0631** for an escaping view and **E0632** for a view used after invalidation.
Copy a value out before leaving the allocator's region. `#Region(name)` creates
an explicit region when inferred scope is too broad or two allocators must
share a lexical lifetime.

```jet
use core.mem

fn run() {
    #Region(scratch) {
        arena :: mem.Arena.new()
        bump :: mem.Bump.new(capacity: 256)
        first :: arena.alloc(1)
        second :: bump.alloc(2)
        print(first)
        print(second)
    }
}
```

| Signature | Result | Description |
|---|---|---|
| `Arena.new()` / `Arena.new(capacity: Int)` | arena | Create a grow-only arena. |
| `Bump.new(capacity: Int)` | allocator | Create a contiguous monotonic allocator. |
| `Pool.new<T>(slots: Int)` | pool | Reuse fixed-size slots by alignment class. |
| `Fixed.new(size: Int)` | allocator | Create positive-comptime inline backing storage. |
| `Fixed.over(&bytes)` | allocator | Borrow one mutable byte buffer for the handle's scope. |
| `allocator.alloc(value)` | view | Store a value and return a scope-bound view. |
| `allocator.reset()` | — | Drop values while retaining reusable storage. |
| `close(^allocator)` | — | Release the allocator resource permanently. |
| `address_of<T>(value: T) -> Int` | address | Return an inert address value. |
| `from_addr<T>(address: Int) -> *T` | pointer | Reconstitute a pointer; requires an audited unsafe context. |
| `volatile_read<T>(pointer: *T) -> T` | value | Perform a volatile read; requires unsafe context. |
| `volatile_write<T>(pointer: *T, value: T)` | — | Perform a volatile write; requires unsafe context. |

`Fixed` never grows or falls back to the heap. It reserves allocation and
reverse-drop space from opposite ends of its buffer and fails before moving a
cursor when they would collide. `reset` is rejected while allocation views are
live; after a valid reset, the same bytes can be reused. Fixed handles and
views cannot be returned, captured, stored, or sent across a task boundary.

### `core.text.parse` — text parsing and scanning

`core.text.parse` contains destination-independent string operations and
Option-returning convenience parsers. `Int.parse` and `Float.parse` are
fallible destination-owned conversions; do not confuse their `Result` return
with the module's Option helpers. The implementation is in
[`Core/text/parse.jet`](../../../Core/text/parse.jet).

| Signature | Result | Description |
|---|---|---|
| `split(text: String, sep: String) -> [String]` | list | Split on a separator. |
| `rsplit(text: String, sep: String, maxsplit: Int) -> [String]` | list | Split from the right with a maximum. |
| `split_once(text: String, sep: String) -> (found: Bool, head: String, tail: String)` | tuple | Split at the first separator and report whether it was found. |
| `partition(text: String, sep: String) -> (head: String, sep: String, tail: String)` | tuple | Return text before, separator, and after. |
| `rpartition(text: String, sep: String) -> (head: String, sep: String, tail: String)` | tuple | Partition at the last separator. |
| `find(text: String, needle: String) -> Int` / `rfind(text, needle) -> Int` | index | Find the first or last byte index. |
| `count(text: String, needle: String) -> Int` | integer | Count non-overlapping occurrences. |
| `replace(text: String, old: String, new: String, count: Int) -> String` | text | Replace up to `count` occurrences; a negative count means all. |
| `starts_with(text: String, prefix: String) -> Bool` / `ends_with(text, suffix) -> Bool` | Boolean | Test a prefix or suffix. |
| `strip_prefix(text: String, prefix: String) -> String` / `strip_suffix(text, suffix) -> String` | text | Remove a matching edge. |
| `parse(text: String) -> ?Int` / `parse_int(text: String) -> ?Int` | option | Parse a decimal integer. |
| `parse_int_base(text: String, base: Int) -> ?Int` | option | Parse an integer in base 2 through 36. |
| `parse_float(text: String) -> ?Float` | option | Parse a floating-point value. |
| `join(parts: [String], sep: String) -> String` | text | Join strings. |
| `splitlines(text: String, keepends: Bool) -> [String]` | list | Split on line boundaries with optional endings. |
| `lstrip(text)`, `rstrip(text)`, `strip(text)` | text | Remove ASCII edge whitespace. |
| `ljust(text, width, fill)`, `rjust(text, width, fill)`, `center(text, width, fill)` | text | Pad text to a byte width. |
| `zfill(text: String, width: Int) -> String` | text | Zero-fill a numeric-looking string. |
| `capitalize(text)`, `title(text)`, `capwords(text)`, `swapcase(text)` | text | Apply case transformations. |
| `lower(text)`, `upper(text)` | text | Apply ASCII case conversion. |
| `is_digit(text)`, `is_alnum(text)`, `is_space(text)` | Boolean | Test text categories. |
| `is_lower(text)`, `is_upper(text)`, `is_title(text)`, `is_ascii(text)` | Boolean | Test ASCII case or character properties. |
| `is_identifier(text: String) -> Bool` | Boolean | Test an ASCII identifier spelling. |
| `find_from(text, needle, start)`, `rfind_from(text, needle, end)` | index | Search from a byte position. |
| `index(text, needle) -> Int` / `contains(text, needle) -> Bool` | index/Boolean | Return or test a match index. |
| `parse_bool(text: String) -> ?Bool` | option | Parse common Boolean spellings. |
| `parse_kv(text: String, sep: String) -> (key: String, ok: Bool, value: String)` | tuple | Split and trim a key/value pair. |
| `split_ws(text: String) -> [String]` | list | Split on ASCII whitespace. |
| `unescape_c(text: String) -> String` / `escape_c(text: String) -> String` | text | Decode or encode C-style escapes. |
| `expandtabs(text: String, tabsize: Int) -> String` | text | Expand tabs to the requested columns. |
| `startswith`, `endswith`, `removeprefix`, `removesuffix`, `rindex`, `encode` | aliases | Python-compatible aliases for the corresponding helpers. |

`Int.parse(text)`, `Int.from_radix(text, base)`, and `Float.parse(text)` return
`!ParseError` and should be handled with `?` or `??`; `Int.from_radix` also
reports a base outside `2..=36` as a parse error. `Cursor.over(text)` is the consuming scanner for
structured text: `skip_ws`, `take_pattern`, and `take_until` advance the cursor
and return an ordinary error value on a miss. The scanner example is
[`Examples/features/parsing/text-cursor.jet`](../../../Examples/features/parsing/text-cursor.jet).

### `Cursor` — consuming text scanner

`Cursor.over(text)` wraps a string with a byte position. Its reads consume a
prefix and advance; a failed pattern or delimiter returns an ordinary error
value. `take_pattern` uses the literal-hole pattern grammar, so the caller
chooses the destination type for each hole.
The consuming cursor and its literal-hole grammar are the text shift boundary (`D-SHIFT1`; `D-PARSESTR1`).

| Signature | Result | Description |
|---|---|---|
| `Cursor.over(text: String) -> Cursor` | cursor | Wrap text in a consuming scanner. |
| `cursor.take_pattern(pattern) -> (holes…) !String` | tuple | Match and consume a literal prefix. |
| `cursor.take_until(delimiter: String) -> String !String` | text | Consume text up to, but not including, a delimiter. |
| `cursor.skip_ws()` | — | Skip leading ASCII whitespace. |

### `U8` — byte values and consuming readers

`U8` represents one byte in `0..255`; a literal outside that range is the
compile-time error **E1003**. `String.bytes()` returns UTF-8 bytes,
`String.from_bytes` validates UTF-8, and `String.from_bytes_lossy` uses
replacement characters. `Reader.over` is a consuming in-memory byte scanner:
every read advances, and a bounds miss is an ordinary error rather than a
panic or silent truncation.
`Reader` is the byte-mode consuming shift boundary (`D-SHIFT1`), and `take_pattern` uses the checked binary-pattern grammar (`D-BINPAT1`).

| Signature | Result | Description |
|---|---|---|
| `String.bytes() -> [U8]` | bytes | Encode a string as UTF-8 bytes. |
| `String.from_bytes(bytes: [U8]) -> String !UTF8Error` | text | Decode bytes strictly as UTF-8. |
| `String.from_bytes_lossy(bytes: [U8]) -> String` | text | Decode bytes with replacement characters. |
| `U8.from_int(n: Int) -> U8 !String` | byte | Perform a checked integer-to-byte conversion. |
| `Int.from_u8(b: U8) -> Int` | integer | Widen a byte to `Int`. |
| `Reader.over(bytes: [U8]) -> Reader` | reader | Wrap an in-memory byte buffer. |
| `reader.read_u8() -> U8 !String` | byte | Read one byte. |
| `reader.read_u16_le()` / `read_u16_be()` | `U16 !String` | Read a 16-bit unsigned value. |
| `reader.read_i8()` / `read_i16_le()` / `read_i16_be()` | signed value | Read signed fixed-width values. |
| `reader.read_u32_le()` / `read_u32_be()` | `U32 !String` | Read a 32-bit unsigned value. |
| `reader.read_i32_le()` / `read_i32_be()` | `I32 !String` | Read a 32-bit signed value. |
| `reader.read_u64_le()` / `read_u64_be()` | `U64 !String` | Read a 64-bit unsigned value. |
| `reader.read_i64_le()` / `read_i64_be()` | `I64 !String` | Read a 64-bit signed value. |
| `reader.read_f32_le()` / `read_f32_be()` | `F32 !String` | Read a 32-bit float. |
| `reader.read_f64_le()` / `read_f64_be()` | `Float !String` | Read a 64-bit float. |
| `reader.peek() -> U8 !String` | byte | Read without advancing. |
| `reader.seek(position: Int)`, `reader.skip(n: Int)` | `Unit !String` | Move by an in-range position or count. |
| `reader.take(n) -> [U8] !String` | bytes | Consume a known byte block. |
| `reader.remaining() -> Int` / `reader.is_at_end() -> Bool` | scalar | Inspect unread bytes. |
| `reader.take_pattern(pattern) -> (holes…) !String` | tuple | Match and consume a literal binary prefix. |

Use `fs.read_bytes` and `fs.write_bytes` for raw file bytes. The complete
scanner examples are [`binary-reader.jet`](../../../Examples/features/parsing/binary-reader.jet)
and [`text-cursor.jet`](../../../Examples/features/parsing/text-cursor.jet).

## Numeric surface

`Int` and `Float` are the default numeric types: `Int` is exact arbitrary
precision with a machine-word fast path, and `Float` is 64-bit. Expert and FFI
code can select `I8`, `I16`, `I32`, `I64`, `U8`, `U16`, `U32`, `U64`, `F32`, or
`F64`. A destination-owned literal is range-checked at compile time; a value
outside the destination range reports **E1003**.
The integer default and its operation rules are the numeric contract (`D-INTBIG1`; `D-NUMOPS1`). Whole-number literals use a destination-owned fixed-width peer when one is available (`D-INTLIT-WIDTH1=F`; `D-NUMLIT-PEER1=A`).

Jet applies one widening law to operators, arguments, returns, and assignments.
A value widens only when the destination contains every source value; Jet does
not search through a third type and never narrows implicitly. `F32` widens to
`Float`. Small integer-to-float crossings that are always exact are allowed;
other crossings check exactness at runtime. `approx(value)` explicitly accepts
possible precision loss for one crossing. Incomparable operator types report
**E0109**; invalid destination types report **E0112** or **E0108**. Sized
numbers erase to their fixed-width ABI representations at code generation, so
the ABI boundary is value-based (`S59`).

Plain arithmetic on a fixed-width integer traps on overflow. Use one of the
explicit operation policies when wrapping or clamping is part of the contract:

| Form | Result | Description |
|---|---|---|
| `a + b`, `a - b`, `a * b`, `a / b` | `T` | Trap if a fixed-width result is out of range. |
| `wrapping(a + b)` | `T` | Wrap in the operand's width. |
| `saturating(a + b)` | `T` | Clamp to the operand's bounds. |
| `checked(a + b)` | `Option<T>` | Return no value on overflow. |
| `value.wrapping_add(other)` | `T` | Receiver form of wrapping addition. |
| `value.saturating_add(other)` | `T` | Receiver form of saturating addition. |
| `value.checked_add(other)` | `Option<T>` | Receiver form of checked addition. |

The wrappers accept exactly one integer arithmetic operation; another expression
reports **E1005**. Integer types expose `MIN`, `MAX`, `count_ones`,
`count_zeros`, `leading_zeros`, and `trailing_zeros`. Float types expose
`INFINITY`, `NEG_INFINITY`, `NAN`, and `EPSILON`, plus `is_nan`, `is_infinite`,
and `is_finite`. Bitwise operators preserve operand width. A shift keeps its
left operand's width, and its count may be any integer width (`U64{byte} << n`
with `n: U8`); a count past the value's width traps rather than leaking a
host-language panic.

Explicit narrowing is destination-owned and fallible. `U8.from_int`,
`I16.from_int`, `F32.from_float`, `Int.from_float`, and the corresponding
fixed-width conversions reject values outside the target's finite range;
integer-to-float conversion truncates only where the destination contract says
so. Handle each conversion's named error rather than relying on implicit
narrowing.


### `core.net` — sockets and DNS

`core.net` is the typed low-level socket layer. It covers TCP, UDP, Unix
sockets, address conversion, readiness, timeouts, and DNS. Calls that need a
provider report a typed `NetError` or `IOError`; the module does not silently
turn a missing transport into a successful no-op. The implementation is
[`Core/net/net.jet`](../../../Core/net/net.jet).

| Signature | Result | Description |
|---|---|---|
| `gethostname() -> String` | text | Read the local host name. |
| `ip_addr(text: String) -[Net, Time.Wait]> IPAddr !NetError` | address | Parse an IP address. |
| `ip_to_string(addr: IPAddr) -> String` / `ip_is_ipv4(addr: IPAddr) -> Bool` | text/Boolean | Render an address or test its family. |
| `socket_addr(host: String, port: Int) -[Net, Time.Wait]> SocketAddr !NetError` | address | Build an address after checking the port. |
| `socket_addr_parse(text: String) -[Net, Time.Wait]> SocketAddr !NetError` | address | Parse a rendered socket address. |
| `socket_host(addr: SocketAddr) -> String` / `socket_port(addr) -> Int` | scalar | Read address components. |
| `socket_to_string(addr: SocketAddr) -> String` / `socket_type(stream: TCPStream) -> String` | text | Render an address or identify a stream. |
| `tcp_listen(addr: String) -[Net, Time.Wait]> TCPListener !NetError` / `tcp_listen_addr(addr: SocketAddr) -[Net, Time.Wait]> TCPListener !NetError` | listener | Bind a TCP listener. |
| `tcp_accept(listener: TCPListener) -[Net, Time.Wait]> TCPStream !NetError` | stream | Accept one connection. |
| `tcp_connect(addr: String) -[Net, Time.Wait]> TCPStream !NetError` / `tcp_connect_addr(addr: SocketAddr) -[Net, Time.Wait]> TCPStream !NetError` | stream | Connect to a TCP endpoint. |
| `tcp_connect_timeout(addr: SocketAddr, timeout_ms: Int) -[Net, Time.Wait]> TCPStream !NetError` | stream | Connect with a nonnegative timeout. |
| `tcp_connect_happy(host: String, port: Int, timeout_ms: Int) -[Net, Time.Wait]> TCPStream !NetError` | stream | Connect through the address-family race. |
| `create_connection(host: String, port: Int) -[Net, Time.Wait]> TCPStream !NetError` / `create_server(host, port) -[Net, Time.Wait]> TCPListener !NetError` | endpoint | Build host/port TCP endpoints. |
| `send(stream: TCPStream, text: String) -[Net, Time.Wait]> Int !NetError` | count | Write text. |
| `tcp_read(stream: TCPStream) -[Net, Time.Wait]> String !NetError` / `tcp_read_bytes(stream, n) -[Net, Time.Wait]> [U8] !NetError` | text/bytes | Read from a TCP stream. |
| `tcp_read_text(stream, n) -[Net, Time.Wait]> String !NetError` | text | Read and decode up to `n` bytes. |
| `tcp_write(stream, text: String) -[Net, Time.Wait]> Unit !NetError` / `tcp_write_bytes(stream, bytes) -[Net, Time.Wait]> Int !NetError` | unit/count | Write text or bytes. |
| `tcp_write_all_bytes(stream, bytes) -[Net, Time.Wait]> Unit !NetError` / `tcp_write_text(stream, text) -[Net, Time.Wait]> Unit !NetError` | unit | Write all bytes or text. |
| `tcp_shutdown(stream, how: NetShutdown) -[Net, Time.Wait]> Unit !NetError` / `tcp_close(stream) -[Net, Time.Wait]> Unit !NetError` | — | Shut down or close a stream. |
| `tcp_reply(stream, status: String, body: String) -[Net, Time.Wait]> Unit !NetError` | — | Write one checked reply with a three-digit status. |
| `tcp_ready(stream, interest: NetReadyInterest, deadline_ms: Int) -[Net, Time.Wait]> NetReady !NetError` | readiness | Wait for a readiness interest until a deadline. |
| `ready_readable(ready: NetReady) -> Bool` / `ready_writable(ready) -> Bool` | Boolean | Inspect readiness. |
| `tcp_local_addr(stream)`, `tcp_peer_addr(stream)` | `String !NetError` | Read rendered stream endpoints. |
| `tcp_local_socket_addr(stream)`, `tcp_peer_socket_addr(stream)`, `listener_local_socket_addr(listener)` | `SocketAddr !NetError` | Read typed stream/listener endpoints. |
| `set_timeout(stream, timeout_ms: Int) -[Net, Time.Wait]> Unit !NetError` | — | Set a nonnegative timeout. |
| `set_read_timeout(stream, timeout_ms: Int) -[Net, Time.Wait]> Unit !NetError` / `set_write_timeout(stream, timeout_ms: Int) -[Net, Time.Wait]> Unit !NetError` | — | Set read or write timeouts in milliseconds. |
| `nodelay(stream) -[Net, Time.Wait]> Bool !NetError` / `set_nodelay(stream, on: Bool) -[Net, Time.Wait]> Unit !NetError` | option | Read or set TCP no-delay. |
| `ttl(stream) -[Net, Time.Wait]> Int !NetError` / `set_ttl(stream, hops: Int) -[Net, Time.Wait]> Unit !NetError` | option | Read or set the TTL. |
| `sendfile(stream, path: String) -[Net, FS, Time.Wait]> Int !NetError` | count | Send file bytes through a TCP stream. |
| `udp_bind(addr: String) -[Net, Time.Wait]> UDPSocket !NetError` / `udp_bind_addr(addr: SocketAddr) -[Net, Time.Wait]> UDPSocket !NetError` | socket | Bind a UDP socket. |
| `udp_local_addr(socket: UDPSocket) -[Net, Time.Wait]> SocketAddr !NetError` / `udp_set_timeout(socket, timeout_ms: Int) -[Net, Time.Wait]> Unit !NetError` | address/— | Inspect a UDP endpoint or set its timeout. |
| `udp_send_to(socket, text, addr) -[Net, Time.Wait]> Int !NetError` / `udp_send_bytes_to(socket, bytes, addr) -[Net, Time.Wait]> Int !NetError` | count | Send text or bytes to a datagram address. |
| `udp_recv_from(socket, limit) -[Net, Time.Wait]> UDPPacket !NetError` / `udp_receive(socket, limit) -[Net, Time.Wait]> UDPPacket !NetError` | packet | Receive a bounded datagram. |
| `udp_packet_data(packet)`, `udp_packet_bytes(packet)`, `udp_packet_addr(packet)` | scalar | Read packet text, bytes, or address. |
| `udp_packet_original_len(packet) -> Int` / `udp_packet_truncated(packet) -> Bool` | scalar | Inspect datagram length and truncation. |
| `unix_listen(path: String) -[Net, FS, Time.Wait]> UnixListener !NetError` / `unix_connect(path) -[Net, FS, Time.Wait]> UnixStream !NetError` | endpoint | Use Unix-domain sockets. |
| `unix_accept(listener) -[Net, Time.Wait]> UnixStream !NetError` / `unix_read(stream) -[Net, Time.Wait]> String !NetError` | stream/text | Accept or read a Unix stream. |
| `unix_write(stream, text) -[Net, Time.Wait]> Unit !NetError` / `unix_read_bytes(stream, n) -[Net, Time.Wait]> [U8] !NetError` | unit/bytes | Write text or read bytes. |
| `unix_write_all_bytes(stream, bytes) -[Net, Time.Wait]> Unit !NetError` / `unix_shutdown(stream, how) -[Net, Time.Wait]> Unit !NetError` / `unix_close(stream) -[Net, Time.Wait]> Unit !NetError` | — | Write all bytes or close a Unix stream. |
| `dns_a(name: String, ms: Int) -[Net, Time.Wait]> [IPAddr] !NetError` / `dns_aaaa(name, ms) -[Net, Time.Wait]> [IPAddr] !NetError` | addresses | Resolve IPv4 or IPv6 records. |
| `dns_a_at(server: String, name: String, ms: Int) -[Net, Time.Wait]> [IPAddr] !NetError` / `dns_aaaa_at(server, name, ms) -[Net, Time.Wait]> [IPAddr] !NetError` | addresses | Resolve A or AAAA records through an explicit server. |
| `dns_txt(name: String, ms: Int) -[Net, Time.Wait]> [String] !NetError` / `dns_ptr(addr, ms) -[Net, Time.Wait]> [String] !NetError` | records | Resolve TXT or PTR records. |
| `dns_txt_at(server: String, name: String, ms: Int) -[Net, Time.Wait]> [String] !NetError` / `dns_srv_at(server, name, ms) -[Net, Time.Wait]> [DNSSrv] !NetError` | records | Resolve TXT or SRV records through an explicit server. |
| `dns_srv(name: String, ms: Int) -[Net, Time.Wait]> [DNSSrv] !NetError` | records | Resolve SRV records. |
| `dns_srv_target(record: DNSSrv) -> String` / `dns_srv_port(record) -> Int` / `dns_srv_priority(record) -> Int` / `dns_srv_weight(record) -> Int` | fields | Inspect an SRV record. |
| `gethostbyname(name: String) -[Net, Time.Wait]> String !NetError` / `gethostbyaddr(addr) -[Net, Time.Wait]> String !NetError` | hosts | Read one host name/address result. |
| `getservbyname(name: String) -[Net, Time.Wait]> Int !NetError` / `getservbyport(port: Int) -[Net, Time.Wait]> String !NetError` | service | Resolve a service name or port. |
| `error_operation(err: NetError) -> String` / `error_message(err) -> String` / `error_address(err) -> ?String` / `error_name(err) -> ?String` / `error_os_code(err) -> ?Int` | diagnostics | Inspect structured network-error details. |
| `addressfamily(addr: SocketAddr) -> String` | family | Report `IPv4` or `IPv6`. |

The `*_at` DNS forms take an explicit server, and `error_operation`,
`error_message`, `error_address`, `error_name`, and `error_os_code` inspect a
`NetError`. Ports are checked in `0..=65535`; timeout and deadline values are
nonnegative milliseconds. A blocking socket call declares `Net` and
`Time.Wait`, so the suspension boundary remains visible.

### `core.net.tls` — TLS stream wrapper

`core.net.tls` upgrades a connected TCP stream to a verified TLS stream. Its
configuration carriers are validated before network use, and configured ALPN
protocols are passed through the native TLS provider. See [`Core/net/tls.jet`](../../../Core/net/tls.jet).

| Signature | Result | Description |
|---|---|---|
| `ClientConfig.default()` | `ClientConfig` | Start with system trust and inclusive TLS 1.2–1.3 version bounds. |
| `ClientConfig.default().with_alpn(protocols)` | `ClientConfig !IOError` | Validate and offer the configured ALPN protocol list. |
| `RootCertificates.from_pem(bytes)` | `RootCertificates !IOError` | Validate a custom PEM root bundle before network use. |
| `ClientIdentity.from_pem(cert_chain: bytes, private_key: bytes)` | `ClientIdentity !IOError` | Validate a PEM certificate chain and matching private key. |
| `ClientConfig.default().with_trust(policy)` | `ClientConfig !IOError` | Select `.System`, `.SystemPlus(roots)`, or `.CustomOnly(roots)` trust. |
| `ClientConfig.default().with_client_identity(identity)` | `ClientConfig !IOError` | Add a validated mTLS client identity to an immutable configuration. |
| `ClientConfig.default().with_version_bounds(min: version, max: version)` | `ClientConfig !IOError` | Select inclusive `.Tls12` / `.Tls13` bounds; reversed bounds fail before network use. |
| `tls.client(^tcp, server_name:, config:, deadline:)` | `TLSStream !NetError` | Consume the connected TCP stream, verify the server name, apply configuration, and use the earliest handshake deadline. |
| `client(host: String, port: Int)` | `TLSStream !IOError` | Convenience form that connects and verifies a host and port. |
| `read(stream: TLSStream, n: Int)` / `read_text(stream, n)` | `[U8] !IOError` / `String !IOError` | Read bounded TLS bytes or checked UTF-8 text. |
| `write(stream: TLSStream, bytes: [U8])` / `write_all(stream, bytes)` | `Int !IOError` / `!IOError` | Write application bytes, partially or completely. |
| `write_text(stream: TLSStream, text)` / `close(stream)` | `!IOError` | Write text or send close-notify; repeated close is harmless. |
| `stream.peer_identity()` | `TLSPeerIdentity` | Read the verified server name, certificate chain, cipher suite, and negotiated TLS version. |
| `stream.read(limit, deadline:)` / `stream.write_all(bytes, deadline:)` | matching results | Apply explicit per-call deadlines on the same TLS handle. |
| `stream.close_write(deadline:)` | `!IOError` | Flush close-notify and close only writes while reads continue. |

TLS handshake, read, write, and close-notify use the consumed socket's shared
readiness path. Handshake failures use `NetError`; stream failures use the
`IOError` tree, including cancellation, timeout, closed, and protocol errors.

### `core.db` — checked database boundaries

`core.db` exposes connections, pools, scoped SQL, policy checks, transactions,
migrations, and typed row decoding. SQL interpolation binds values; raw SQL
requires the explicit `SQL.raw` escape. Database operations carry `DB` (and,
when they wait, `Time.Wait`) effects. The module source is
[`Core/db/db.jet`](../../../Core/db/db.jet).
The backend-neutral driver/policy boundary keeps query operations and the user policy attached to the typed scope (`D-DBDRIVER1=A`; `D-DBPOLICY-BIND1=A`).

| Signature | Result | Description |
|---|---|---|
| `open(url: String) -[DB]> DBConnection` | connection | Open a database URL. |
| `open_memory() -[DB]> DBConnection` | connection | Open an in-memory database. |
| `pool(url: String, max: Int) -[DB]> DBPool DBError!` | pool | Create a bounded connection pool (`DBPool` → `DBLease`, `DBPoolReceipt`). |
| `policy(table: String, expression: String) -[DB]> RowPolicy DBError!` | policy | Compile a checked row-access policy. |
| `policy_audit(scope: DBScope) -[DB]> String` | text | Inspect policy decisions for a scope. |
| `row_value(row: [String: DBValue], column: String) -[DB]> DBValue DBError!` | value | Read an untyped row value. |
| `row_int(row: [String: DBValue], column: String) -[DB]> Int DBError!` / `row_float(row, column) -[DB]> Float DBError!` | scalar | Read numeric row values. |
| `row_text(row: [String: DBValue], column: String) -[DB]> String DBError!` / `row_bool(row, column) -[DB]> Bool DBError!` | scalar | Read text or Boolean row values. |
| `decode<T: Decode>(row: [String: DBValue]) -[DB]> T [FieldError]!` | `T` | Decode a row into a typed value. |
| `transaction(scope: DBScope, name: String, steps: [SQL]) -[DB, Time.Wait]> Int DBError!` | count | Execute a checked transaction. |
| `migrate(scope: DBScope, name: String, steps: [SQL]) -[DB, Time.Wait]> Int DBError!` | count | Apply a migration under its checksum. |

Scoped connection and pool handles provide checked `query`, `query_one`,
`execute`, lease, commit, rollback, and close operations. A policy expression
is intentionally small: the supported forms are `true` and `owner == user`
with a table identifier. Policy text has a bounded size. Schema and transaction
control belong to the migration/control path, not to arbitrary interpolated
application values. Migrations record their name and checksum so a changed
migration cannot silently replace an applied one.

### `core.compute` — checked tensors and explicit devices

`core.compute` provides dense tensors, shape-checked operations, sparse CSR
helpers, value-based differentiation, linear algebra, FFT, and serialization.
`Auto` selects the CPU reference path; an explicit unsupported device returns a
typed capability error rather than silently falling back. The source is
[`Core/compute/compute.jet`](../../../Core/compute/compute.jet), with executable
examples under [`Examples/features/tooling/`](../../../Examples/features/tooling/).
This is one ranked Tensor operation family with an explicit placement contract (`D-COMPUTE1=D`; `D-COMPUTE-TYPE1=D`; `D-COMPUTE-PLACE1=D`). Gradient and value-and-gradient calls remain explicit, checked value-based transforms (`D-COMPUTE-GRAD1=E`).

| Signature | Result | Description |
|---|---|---|
| `device_cpu() -> ComputeDevice`, `device_auto() -> ComputeDevice` | device | Select CPU or automatic placement. |
| `device_cuda() -> ComputeDevice`, `device_metal() -> ComputeDevice`, `device_vulkan() -> ComputeDevice`, `device_webgpu() -> ComputeDevice` | device | Request an explicit provider device. |
| `device(tensor: Tensor) -> String` | text | Read the tensor's device name. |
| `zeros(shape: [Int]) -> Tensor !ComputeError`, `ones(shape)`, `full(shape, value)` | tensor | Construct checked dense tensors. |
| `eye(n: Int) -> Tensor !ComputeError` | tensor | Construct a square identity tensor. |
| `from_list(values: [Float]) -> Tensor !ComputeError` | tensor | Construct a one-dimensional tensor. |
| `vec(length: Int, value: Float) -> Tensor !ComputeError` / `matrix(rows: Int, cols: Int, value: Float) -> Tensor !ComputeError` | tensor | Construct filled vectors or matrices. |
| `to_list(tensor) -> [Float]`, `shape(tensor) -> [Int]`, `rank(tensor) -> Int`, `numel(tensor) -> Int` | metadata | Inspect or copy tensor storage. |
| `placement(tensor: Tensor) -> String` | text | Report placement and numeric profile. |
| `on_device(tensor, target: ComputeDevice) -[Env]> Tensor !ComputeError` / `transfer(tensor, target) -[Env]> Tensor !ComputeError` | tensor | Move to an explicit provider or reject it. |
| `transfer_show(tensor: Tensor) -> String` | text | Show the source-tier transfer receipt. |
| `profile_f32_strict() -> String` / `profile_show() -> String` | text | Show the F32 strict profile receipt. |
| `kernel_bounds_ok(shape: [Int], index: [Int]) -> Bool !ComputeError` | Boolean | Check rank and bounds. |
| `get(tensor: Tensor, index: [Int]) -> Float !ComputeError` | scalar | Read a checked element. |
| `set(tensor: &Tensor, index: [Int], value: Float) !ComputeError` | — | Mutate a checked element in place. |
| `reshape(tensor, new_shape: [Int]) -> Tensor !ComputeError` | tensor | Change shape without changing element count. |
| `broadcast_to(tensor, new_shape: [Int]) -> Tensor !ComputeError` | tensor | Materialize checked broadcasting. |
| `transpose(tensor: Tensor) -> Tensor !ComputeError` | tensor | Transpose a rank-two tensor. |
| `add`, `sub`, `mul`, `div`, `maximum`, `minimum` | tensor | Apply shape-compatible elementwise operations. |
| `abs`, `negate`, `exp`, `log`, `sqrt` | tensor | Apply checked elementwise math. |
| `matmul(a, b) -> Tensor !ComputeError` / `matmul_f32_tile(a, b) -> Tensor !ComputeError` | tensor | Multiply compatible matrices under a profile. |
| `sum_axis(tensor, axis: Int) -> Tensor !ComputeError` | tensor | Reduce one axis. |
| `mse_loss(pred, target) -> Tensor !ComputeError` | tensor | Compute a one-element mean-squared-error tensor. |
| `sgd_step(weights, grad, lr: Float) -> Tensor !ComputeError` | tensor | Apply a value-based SGD step. |
| `value_and_gradient(pred, target) -> VjpRun !ComputeError`, `vjp(pred, target) -> VjpRun !ComputeError` | result | Evaluate the fixed MSE value and gradient. |
| `gradient(pred, target) -> Tensor !ComputeError`, `jvp(pred, target, tangent) -> Tensor !ComputeError` | tensor | Return gradient or tangent result for that contract. |
| `det(tensor) -> Float !ComputeError`, `inv(tensor) -> Tensor !ComputeError`, `solve(a, b) -> Tensor !ComputeError` | linear algebra | Perform checked square-matrix operations. |
| `fft(tensor: Tensor) -> Tensor !ComputeError` | tensor | Return a rank-one interleaved real/imaginary spectrum. |
| `to_sparse(tensor) -> SparseTensor !ComputeError` / `sparse_nnz(sp) -> Int` | sparse | Convert dense rank-two CPU data and count nonzeros. |
| `sparse_show(sp: SparseTensor) -> String` / `sparse_mv(sp, x) -> Tensor !ComputeError` | sparse | Inspect or multiply CSR data. |
| `serialize(tensor: Tensor) -> String !ComputeError` / `deserialize(text: String) -> Tensor !ComputeError` | text/tensor | Use the canonical checked tensor text format. |
| `stream_new() -> ComputeStream`, `stream_new_on(target) -> ComputeStream !ComputeError` | stream | Create a CPU or explicitly requested stream. |
| `stream_show(stream) -> String`, `stream_sync(stream) !ComputeError` | text/— | Inspect or synchronize a stream. |

The tensor shape is checked for every operation. `fft` uses a radix-2 path or a
bounded DFT for non-power-of-two sizes and emits interleaved real/imaginary
values; there is no implicit complex tensor. Serialization records shape,
data, numeric profile, and a 16-digit checksum; corrupt or malformed data is
rejected. `profile_f32_strict` and `profile_show` expose numeric-profile
selection, while `kernel_bounds_ok` checks a kernel index contract without
claiming that an arbitrary closure is a native kernel.

### `core.service` — bounded service trees and delivery

`core.service` describes named service trees, bounded mailboxes, restart
strategies, and durable delivery. The topology and delivery records are
structured values, not an untyped supervisor log. The source is
[`Core/service/service.jet`](../../../Core/service/service.jet); the service
runtime model is also exercised by the service examples and tests.
The tree is the single public topology boundary; workers, delivery, and restart policies remain typed rather than string-keyed (`D-SERVICE1=D`).

| Signature | Result | Description |
|---|---|---|
| `tree(name: String) -> ServiceTree` | tree | Create a named service topology. |
| `tree.worker<T>(name: String, handler, capacity: Int) -> ServiceWorker<T>` | worker | Add a bounded-mailbox worker. |
| `tree.start()` / `tree.stop()` | — | Start or stop the service tree. |
| `tree.show() -> DataTree` | tree | Inspect topology and delivery state. |
| `tree.set_restart(strategy: ServiceRestart)` | — | Select one-for-one, all-for-one, or rest-for-one restart. |
| `runtime(store: String, retention: Duration) -> ServiceRuntime` | runtime | Describe runtime storage and retention. |
| `state_store(path: String) -> ServiceStateStore !ServiceError` | store | Open a durable state store. |
| `restart_one_for_one()`, `restart_one_for_all()`, `restart_rest_for_one()` | strategy | Construct restart strategies. |
| `delivery_at_most_once()`, `delivery_durable()` | policy | Select at-most-once or durable delivery. |

A bounded mailbox makes admission visible. Delivery transitions are
`Pending`, `Accepted`, `Delivering`, `Delivered`, `DeadLettered`, and
`Cancelled`; durable delivery records the authority needed for replay and
recovery. A restart strategy controls which siblings are restarted after a
worker failure. A service handler must report or persist its outcome according
to the selected delivery policy rather than assuming retries are free.

### `core.archive` — byte containers and checksummed compression

`core.archive` contains bounded archive and compression primitives. The Jet
implementation lives under [`Core/archive/`](../../../Core/archive/); it is not
the retired Rust `corelib/core.archive` path. Archive output is capped at
67,108,864 bytes and archive entry counts at 4,096; file-oriented gzip and
zstd helpers use their own 64 MiB output cap.
Each codec has one public home. Compose a container and a stream codec explicitly for `tar.gz`: build TAR bytes with `core.archive`, then compress those bytes with `core.archive.gzip` (`D-CORE-COMPRESS1=A`).

| Signature | Result | Description |
|---|---|---|
| `crc32(bytes: [U8]) -> Int` / `adler32(bytes: [U8]) -> Int` | checksum | Compute nonnegative checksums. |
| `deflate(bytes: [U8]) -> [U8] ArchiveError!` | bytes | Emit an RFC 1951 stored-block stream; input over 64 MiB is `Unsupported`. |
| `inflate(bytes: [U8]) -> [U8] ArchiveError!` | bytes | Decode stored, fixed, or dynamic deflate blocks. |
| `compress(bytes: [U8]) -> [U8] ArchiveError!` | bytes | Add a zlib wrapper around deflate output; input over 64 MiB is `Unsupported`. |
| `decompress(bytes: [U8]) -> [U8] ArchiveError!` | bytes | Validate and decode a zlib stream. |
| `zip_decompress(data: [U8]) -> [U8] ArchiveError!` | bytes | Return the first ZIP file's data. |
| `zip_names_json(archive: [U8]) -> String ArchiveError!` / `list(archive) -> [String] ArchiveError!` | names | List ZIP entry names as JSON or values. |
| `zip_open(archive: [U8]) -> [U8] ArchiveError!` | reader | Validate and retain an immutable ZIP buffer. |
| `zip_next(reader: [U8], index: Int) -> String ArchiveError!` | name | Read the name at an entry index. |
| `zip_read(reader: [U8], name: String) -> [U8] ArchiveError!` | bytes | Read one named ZIP entry. |
| `zip_write(writer: [U8], name: String, data: [U8]) -> [U8] ArchiveError!` | bytes | Return a ZIP buffer with an entry written. |
| `zip_close(writer: [U8]) -> [U8] ArchiveError!` | bytes | Validate and finish a ZIP buffer. |
| `zip_extract(archive: [U8], name: String) -> [U8] ArchiveError!` / `unzip(archive, name) -> [U8] ArchiveError!` | bytes | Extract one named ZIP entry. |
| `tar_add(archive: [U8], name: String, data: [U8]) -> [U8] ArchiveError!` | bytes | Add a TAR entry; an absolute, `..`, NUL or backslash name is `Malformed`, and a result over 64 MiB is `Unsupported`. |
| `tar_get(archive: [U8], name: String) -> [U8] ArchiveError!` | bytes | Read one TAR entry. |
| `tar_names_json(archive: [U8]) -> String ArchiveError!` | names | List TAR entry names as JSON. |
| `create(name: String, data: [U8]) -> [U8] ArchiveError!` | bytes | Create a one-entry ZIP archive; an unsafe name is `Malformed`, and data over 64 MiB is `Unsupported`. |

The ZIP helpers use immutable byte buffers rather than a hidden reader/writer
object. Check archive errors before trusting names, sizes, or extracted bytes;
limits are part of the safety boundary. Every decoder bounds its decoded
output, never only its compressed input, so a small input that expands still
stops at the 64 MiB cap with `Unsupported`. Encoders and writers never signal
failure with an empty or unchanged buffer; they return the typed error.
`Examples/features/io/archive_hostile.jet` exercises truncated, bad-checksum,
output-bomb, 4,097-entry, and traversal-name inputs for every codec.

### `core.archive.gzip` — bounded gzip files

`core.archive.gzip` handles whole-buffer gzip streams and text/file adapters.
It validates the gzip magic, size, CRC, and supported method before returning
bytes. See [`Core/archive/gzip.jet`](../../../Core/archive/gzip.jet).

| Signature | Result | Description |
|---|---|---|
| `compress(data: [U8]) -> [U8] GzipFileError!` | bytes | Encode a gzip buffer; input over 64 MiB is `Unsupported`. |
| `decompress(data: [U8]) -> [U8] GzipFileError!` | bytes | Decode and validate a gzip buffer. |
| `is_gzip(data: [U8]) -> Bool` | Boolean | Test the gzip magic and method. |
| `magic() -> [U8]` | bytes | Return the gzip magic bytes. |
| `isize(data: [U8]) -> Int` / `peek_isize(data: [U8]) -> Int` | size | Read the stored uncompressed size. |
| `crc(data: [U8]) -> Int` | checksum | Compute the gzip CRC-32 value. |
| `compress_text(text: String) -> [U8] GzipFileError!` | bytes | Encode UTF-8 text. |
| `decompress_text(data: [U8]) -> String GzipFileError!` | text | Decode gzip bytes as UTF-8. |
| `compress_file(src: String, dst: String) -[FS]> Bool GzipFileError!` | Boolean | Compress a file under the file capability. |
| `decompress_file(src: String, dst: String) -[FS]> Bool GzipFileError!` | Boolean | Decompress a file under the file capability. |

`GzipFileError` distinguishes read, write, malformed, unsupported, and
checksum failures. The output limit is 64 MiB.

### `core.archive.zstd` — bounded Zstandard files

`core.archive.zstd` handles the supported subset of Zstandard whole-buffer
streams and file adapters. It emits standards-valid single-segment raw blocks;
compressed blocks and dictionaries are rejected explicitly. The source is
[`Core/archive/zstd.jet`](../../../Core/archive/zstd.jet).

| Signature | Result | Description |
|---|---|---|
| `compress(data: [U8]) -> [U8] ZstdFileError!` | bytes | Encode a Zstandard buffer; input over 64 MiB is `Unsupported`. |
| `decompress(data: [U8]) -> [U8] ZstdFileError!` | bytes | Decode supported raw or RLE blocks. |
| `is_zstd(data: [U8]) -> Bool` | Boolean | Test the Zstandard magic. |
| `magic() -> [U8]` | bytes | Return the Zstandard magic bytes. |
| `compress_text(text: String) -> [U8] ZstdFileError!` | bytes | Encode UTF-8 text. |
| `decompress_text(data: [U8]) -> String ZstdFileError!` | text | Decode a Zstandard text buffer. |
| `compress_file(src: String, dst: String) -[FS]> Bool ZstdFileError!` | Boolean | Compress a file under the file capability. |
| `decompress_file(src: String, dst: String) -[FS]> Bool ZstdFileError!` | Boolean | Decompress a file under the file capability. |

`ZstdFileError` distinguishes read, write, malformed, unsupported, and checksum
failures. The output limit is 64 MiB, and unsupported compressed blocks or
external dictionaries are never silently substituted with another codec.

## Common mistakes

Use Jet's names and binding forms instead of importing conventions from another
language:

- `println(...)` → `print(...)`
- `eprintln(...)` → `term.eprint(...)`
- `open("file")` or `File.open` → `fs.read(...)` and `fs.write(...)`
- `getenv("X")` or `os.environ` → `env.get("X")`
- `import core.files` → `use core.files`
- Assignment that should not change → `name :: value`
- Assignment that should change → `name := value`

`String.bytes()` and `String.from_bytes(...)` are the explicit text/byte
boundary. Use `Int.parse`/`Float.parse` for named parse errors, and
`core.text.parse.parse_int`/`parse_float` when an Option result is the desired
API.

## `core` — built Core modules

The canonical module registry is [`Core.jet`](../../../crates/jet-codegen/src/Prelude/Core.jet),
and the loader's exported list is [`CoreModuleExports.rs`](../../../crates/jet-foundation/src/CoreModuleExports.rs).
The list below is the source-facing module inventory. A module may be a
namespace-only provider boundary; its presence in the registry does not imply
that every provider operation is available on every target.
The built-module inventory is a source-facing registry, not a missing-domain ledger (`D-STDLIBLEDGER1`). Memory management is opt-in scoped automatic GC through `#Policy(gc)`; collector state remains compiler-private while ordinary code keeps bare owned values (`D-OPTGC1`).

- `app`
- `core`
- `core.models`
- `core.devtools`
- `core.archive`
- `core.archive.gzip`
- `core.archive.zstd`
- `core.args`
- `core.auth`
- `core.build`
- `core.compiler`
- `core.compiler.lang`
- `core.collections`
- `core.collections.set`
- `core.compute`
- `core.compute.solve`
- `core.crypto`
- `core.crypto.expert`
- `core.crypto.random`
- `core.crypto.uuid`
- `core.crypto.vault`
- `core.data`
- `core.data.arrow`
- `core.data.loader`
- `core.data.stream`
- `core.data.plot`
- `core.data.sketch`
- `core.data.sketch.cms`
- `core.data.sketch.hll`
- `core.data.sketch.reservoir`
- `core.data.sketch.tdigest`
- `core.db`
- `core.email`
- `core.encoding`
- `core.encoding.base32`
- `core.encoding.base64`
- `core.encoding.binary`
- `core.encoding.cbor`
- `core.encoding.csv`
- `core.encoding.hex`
- `core.encoding.ini`
- `core.encoding.json`
- `core.encoding.jsonl`
- `core.encoding.toml`
- `core.encoding.xml`
- `core.event`
- `core.files`
- `core.encoding.yaml`
- `core.files.path`
- `core.font`
- `core.game`
- `core.game.raylib`
- `core.http`
- `core.http.client`
- `core.http.server`
- `core.jobs`
- `core.log`
- `core.math`
- `core.math.random`
- `core.math.combinatorics`
- `core.math.stats`
- `core.mem`
- `core.mem.scope`
- `core.mod`
- `core.net`
- `core.net.mime`
- `core.net.url`
- `core.net.tls`
- `core.net.ws`
- `core.net.ip`
- `core.perf`
- `core.plugin`
- `core.prelude`
- `core.process`
- `core.reactive`
- `core.reactive.loadable`
- `core.reflect`
- `core.regex`
- `core.rt`
- `core.service`
- `core.sync`
- `core.sys`
- `core.tasks`
- `core.term`
- `core.testing`
- `core.text`
- `core.text.fmt`
- `core.text.html`
- `core.text.wrap`
- `core.text.parse`
- `core.time`
- `core.time.calendar`
- `core.time.expiring`
- `core.ui`
- `core.tui`
- `core.ui.host`
- `core.ui.host.clipboard`
- `core.ui.host.ime`
- `core.ui.host.drag_drop`
- `core.ui.host.shortcuts`
- `core.ui.host.accessibility`
- `core.units`
- `core.watcher`
- `core.web`
- `core.web.browser`
- `core.web.devserver`
- `core.web.forms`
- `core.web.query`
- `core.web.router`
- `core.web.storage`
- `core.web.storage.local`
- `core.web.storage.session`
- `core.web.store`
- `core.web.table`
- `core.web.virtual`

### Writing Core in Jet

The Rust-hosted compiler remains Jet's production and reference compiler. Jet
source packages under [`Core/`](../../../Core/) are compiled by that normal
frontend; they are not evidence that the compiler is self-hosted. The staged
Jet-authored compiler in [`Compiler/`](../../../Compiler/) is a separate
bootstrap project, while Rust emission, rustc/LLVM, and Cranelift remain part
of the host implementation.

`Core.jet` is the declaration registry, `Core/**/*.jet` is the source for the
Jet-owned package surface, and provider code supplies operations that require
an external runtime or operating-system capability. A change to an exported
signature must update the source module, the registry, the relevant conformance
corpus, and any examples that exercise the contract. The registry and loader
list are the executable authority; this reference explains the durable rules
and points to those files instead of duplicating their generated details.
The audited intrinsic/ABI kernel is the only provider seam; ordinary Core packages remain Jet source. Archive byte-format calls cross that seam only where the source implementation requires them.

### Examples in the repository

Executable examples are organized by capability. Useful entry points for this
part include:

- [`Examples/features/parsing/text-cursor.jet`](../../../Examples/features/parsing/text-cursor.jet)
  and [`binary-reader.jet`](../../../Examples/features/parsing/binary-reader.jet)
  for consuming scanners;
- [`Examples/features/io/files.jet`](../../../Examples/features/io/files.jet)
  and [`Examples/features/io/cli.jet`](../../../Examples/features/io/cli.jet)
  for capability-bound I/O;
- [`Examples/features/serde/json_integer_fidelity.jet`](../../../Examples/features/serde/json_integer_fidelity.jet)
  for typed encoding behavior;
- [`Examples/features/tooling/compute_tensor.jet`](../../../Examples/features/tooling/compute_tensor.jet)
  and [`app_live.jet`](../../../Examples/features/tooling/app_live.jet)
  for compute and web surfaces;
- [`tests/conformance/corpus/core/`](../../../tests/conformance/corpus/core/)
  for small, checked examples of the exported Core contracts.

Use the source module and its conformance corpus together when documenting an
API: the source establishes the export and error boundary, while the corpus
shows the syntax and boundary behavior that a caller can rely on.
