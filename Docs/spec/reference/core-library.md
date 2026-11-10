# Core library (`core`)

Vocabulary: [Jet vocabulary](../vocabulary.md).

This reference is for Jet application authors who need the standard modules for
files, data, networking, security, command-line programs, and system
integration. The executable declarations live in [`Core/`](../../../Core/);
the Prelude registry lists exports and module paths in
[`Core.jet`](../../../Compiler/Prelude/Core.jet), and the
[`Examples/`](../../../Examples/) and [`tests/`](../../../tests/) directories
exercise the contracts. The Jet compiler in [`Compiler/`](../../../Compiler/)
checks and builds them.

Application and `#Test` helper functions retain their canonical entry-module
call identity in both single-file and multi-file packages; the absence of
sibling modules does not change callable resolution.
The private testing evidence and temporary-path records are ordinary Core
source records; lowering uses their checked declarations rather than synthetic
compiler-owned records. Their raw host transports retain paired Prelude carrier
name mappings until the host runtime is replaced at cutover.

Compiler-private `Core/*/runtime` imports join the loaded Core source closure
on demand. They retain their local aliases as ordinary checked module edges;
applications still cannot import those units. Unicode String methods and text
helpers share these runtime kernels. Segmentation lookup binds a stable read
window before passing table bytes to `View<U8>` parameters, without copying
the table or an owned slice.

Whole-file filesystem checks, atomic-write sequencing, scoped path selection,
logical environment policy, signal masks, and line decoding live in Jet source.
Their reserved raw bindings preserve the existing host handles and shared read
buffers until the Jet runtime replaces those leaves at cutover. File lines trim
all trailing CR/LF bytes; process-pipe lines remove only one LF and its preceding
CR, preserving an unterminated final CR.
`ProcessStdin.close` registers its existing provider close directly under the
canonical receiver name. Taking the optional writer drops the sole native
descriptor; repeated closes are harmless. There is no extra Jet forwarding
facade or source-level close policy at this host-only boundary.

Caller-owned `RNG`, `testing.RNG`, and `math.random.RNG` name one Jet nominal:
`RNG { state: U64 }`. It is not implicitly copyable; an explicit clone creates an
independent stream. Display is `RNG { .. }`. SplitMix64 transitions, bounded
rejection sampling, distributions, and collection draws are source behavior;
`bool(p)` retains its existing arity-specific receiver row. Bare source-loading
triggers remain limited to URL/url, Perf, and RNG; unrelated Core aliases do not
become bare names, and Date retains its existing annotation identity.
Bounded caller-owned draws keep the complete 64-by-64 product in exact `Int`
before selecting its high and low words. Rejection thresholds and stream
consumption remain identical to the native SplitMix64 seed.

`Fraction` keeps its reduced exact-`Int` parts in private storage, distinct from
the public `numerator()` and `denominator()` accessors. Field and method names
share one member namespace; the storage names are not a construction API.

Manual Core `Debug` implementations use the Prelude's consuming `debug(&&self)`
contract; `Display` reads `self`. Neither formatting projection changes its text.
Data errors provide their explicit `Display` and `Debug` implementations beside
the error declaration; the `#Error` rule does not need a second formatting
implementation. Database values and pool receipts use `Display`, not a separate
`Show` trait.
`WrappedVaultKey` likewise keeps encrypted bytes in private storage separate
from `bytes()`, which still returns an owned copy.

File-backed JSON and CBOR writers pass borrowed byte views across the write
boundary. Private CBOR sorting uses integral heap indexes and swaps paired
key/value entries in place. Logical environment removal uses an explicit list
slot; Dotenv replacement preserves sibling order and moves only the selected
entry, rather than copying its subtree.
The XML reader likewise binds a caller-owned byte window before handing it to
the scanner; the read buffer is not copied into an owning slice at that boundary.

HTTP response construction moves each owned body and header carrier into the
transport exactly once. `core.http.server.json` reports serialization refusal
as `HTTPError.InvalidFraming`, including non-finite JSON numbers, rather than
claiming an infallible encoding operation.
Repeated value arguments replace their existing ordered object slot. Service
state-path grammar selects Windows versus POSIX rules at compile time.
The private XML engine uses `UTF16Le` and `UTF16Be` consistently; incremental
wire decoding is distinct from the type's derived `decode` constructor.

Caller-owned `RNG`, `testing.RNG`, and `math.random.RNG` name one Jet nominal:
`RNG { state: U64 }`. It is not implicitly copyable; an explicit clone creates an
independent stream. Display is `RNG { .. }`. SplitMix64 transitions, bounded
rejection sampling, distributions, and collection draws are source behavior;
`bool(p)` retains its existing arity-specific receiver row. Bare source-loading
triggers remain limited to URL/url, Perf, and RNG; unrelated Core aliases do not
become bare names, and Date retains its existing annotation identity.
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
Provider record slots preserve optional presence when their host ABI uses
either native `Option` or the checked outcome carrier. Integer payload
narrowing and widening do not change which carrier is present or absent.

Source-owned declarations retain their checked signatures, effects, and authority
requirements in the Core registry. They have no host symbol: calls execute the
owning Jet source function, not a separate Core dispatcher implementation. Host
operations beneath that source may remain registered raw leaves while their
runtime layer is ported; this does not create a second implementation of the
public declaration's policy.
Private source-part registration explicitly records built-in receiver
implementation ownership in `impl_types`. An empty list does not suppress the
part's own nominal declarations.
Directory creation shares the existing `jet_std_fs_create_dir_all_raw` host
operation; its callers keep their own fault preflight and error resource.

Devtools checked publication constructs its package/module/panel identities
and ordered, JSON-escaped field envelope in `Core/Devtools/Devtools.jet`.
The private typed-payload leaf retains the existing validated event and global
sink, including silent rejection of invalid events and no durable-delivery
claim when no sink is installed. Volatile memory access retains the checked
compiler intrinsic and its audited pointer obligations.
Encoding stream capacity/reservation/layout operations use their canonical
private `core.mem.__list_capacity`, `__list_reserve_exact`, and `__type_size`
leaves directly; no public wrapper or duplicate alias is introduced.

Compiled-module loading sequences grant/read, header checks, digest, staging,
native mapping, export resolution, and Text release-symbol checks in Jet.
The private context owns one file buffer and parsed metadata; the header borrow
never escapes a raw operation. Ordinary Close first unloads any opened native
library, then removes its staged payload, matching the verification provider's
error path. Successful finish transfers those resources into the existing Mod.

The canonical `core.files.__create_dir_all_raw` leaf calls the shared recursive
directory-creation host operation without a second fault probe; source callers
retain their own FS.Write preflight and the host's Write/path error context.
Filesystem internal host calls use their canonical `core.files.__*` names
directly. There is no second `__files7_*` alias layer: the Prelude's existing
registry entries provide the raw host boundary until Jet runtime L7 replaces
them at Rust cutover.
The Rust verification tier single-homes filesystem syscall tails in
`jet_std_fs_*_raw` functions. Native seed entry points retain their original
fault probes and call those tails; Jet entry points can perform the same
preflight once before the raw call. Temporary and lock results still own the
native cleanup resource, rather than substituting a path-only record.
Logging and browser filesystem facts follow the same canonical boundary:
Jet owns filtering, field normalization, configuration precedence and lock
validation. Registered host leaves retain the existing log sink/span state
and exact host path/metadata behavior until their runtime layers cut over.
Web live/store snapshots project the resident locked state; they do not keep
another registry. Table source helpers retain shared column and state identity
through the registered carrier leaves. Browser reports and server owners keep
the existing serializer and process resources behind that same boundary.
Table page loaders, generation checks, and publication stores operate on those
same carriers. Cross-page key validation lends the existing page-key vectors
under the registry's one lock before committing the accepted page.
Client table ordering stays in Jet source: host leaves expose checked column
accessors, shared row snapshots, and canonical page construction without a
second key/filter/sort implementation.
Qualified calls to a source-only Core export resolve to the loaded declaration,
including calls from its own Core module (`core.sys.usize_max()` is one such
export). Same-module native provider rows retain their host boundary when that
row exists; qualified spelling does not invent another implementation.
The same direct-boundary rule applies to the held reader, directory, xattr,
writer-stat and lock operations: canonical private names select the existing
host kernels without source pass-through facades. Path permission and ownership
policy uses the compile-target OS family, not a duplicate platform detector.
`walk` and `walk_files` own traversal in Jet: depth starts at zero for root
entries, results sort by shown path, ancestor symlinks are refused, and each
directory inherits the existing ignore matcher. The walk is depth first and
sorts one directory at a time: each child directory is walked where its path
and `/` fall among its siblings, which is exactly where its entries sort, so
the whole result is never sorted. The private context preserves
native path identities (including non-Unicode names) until its scope guard
releases it. A directory that fails is left out while the walk continues;
the earliest error is selected by native directory-path ordering.
Directory removal/listing, hard links, socket mode checks, canonicalization,
and temporary directory/lock creation also perform their fault policy in Jet.
Temporary directory and file names share the same Jet prefix, process-ID,
clock-fallback, and host-path policy. Glob and directory copy retain native
dispatch until their traversal, matching, and copy-selection policies move to Jet.
Terminal reads and prompt/style/progress policy likewise use source-owned
declarations. Their registered private facts call the single existing terminal
and stdin kernels; the Rust seed entry points retain their fault preflight.
`take` forwards through the registered source target `__take_bytes`, preserving
the keyword spelling without a parser exception. Jet rejects a negative count
before the read fault probe; the existing stdin kernel performs the one raw read.
Font shaping returns the existing
host result through a private leaf; the Jet entry owns the exact failure panic
and never selects an approximate fallback implicitly.

Types and error enums use PascalCase (`String`, `IOError`, `JSON`); functions
and module segments use snake_case (`read`, `core.files`) (S54). Acronyms keep
their declared spelling; see S66.

## Quick start

```jet
use {
    fs: core.files
    term: core.term
    sys: core.sys
    process: core.process
}

run: fn() {
    args: process.args()
    if args.len() < 1 {
        term.eprint("usage: greet <name>")
        return
    }
    name: args.get(0) ?? return
    greeting: sys.get("GREETING") ?? "hello"
    fs.write("/tmp/greet.txt", "{greeting}, {name}!") ?? return
    print(fs.read("/tmp/greet.txt") ?? return)
}
```

Everything after `--` is forwarded to the program:

```sh
jet run tool.jet -- World --verbose
jet build tool.jet
```

`process.args()` supplies only the words after the program name; the program
path comes from `sys.executable()`. The process and subprocess API is documented in the
[core.process](#coreprocess--process-execution) section below.

## Importing modules and compile-time views

Core modules use `use` without quotes. Quoted imports name `.jet` files, not
compiler-known Core modules:

```jet
use {
    fs: core.files
    json: core.encoding.json
}
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

A Core module's API is the list of names it exports: the items this reference
documents for it, recorded as its `module core.X exports { … }` row in
`Compiler/Prelude/Core.jet`. Every other declaration in its source belongs to
Core alone, including internal helpers and constants and handle types such as
`core.db`'s `CachedStatement` or `core.crypto`'s `JetcFd`, whose fields are
live engine or native handles. Code outside Core can't name them:
`use core.db.[CachedStatement]` and a `db.CachedStatement{…}` literal through
an alias are both **E0609**. A call to
an unexported function through an alias is E1004, so safe code can't build a
handle from a made-up word.

### `core.compiler.lang` — language declarations

`core.compiler.lang` publishes the closed compiler vocabulary used by typed
marker arguments. These declarations are ordinary generated enums and structs;
the marker registry, diagnostics, completion, documentation, and reflection
use the same names.

| Declaration | Kind | Purpose |
| --- | --- | --- |
| `ABI`, `FFILanguage`, `Target`, `Track` | enum | Calling convention, foreign language, code-generation target, and tracking policy values |
| `ArithmeticMode`, `InlineMode`, `JobScope`, `KernelMode`, `Layout`, `MemoBound` | enum | Arithmetic, inlining, publication scope, parallel-kernel, representation, and memoization choices |
| `Effect`, `Maturity`, `NamingCase`, `ObligationMode`, `PolicySetting`, `Site`, `TaintKind` | enum | Effect roots, maturity, field naming, proof obligations, policy, marker sites, and taint values |
| `Path`, `State` | struct | Marker path text and compiler-threaded typestate names |

For example, `#Inline` accepts an imported enum value or a dot literal:

```jet
use lang: core.compiler.lang

#Inline(lang.InlineMode.Always)
parse_fast: fn(text: String) -> Int {
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

The source declarations supply the public typed vocabulary; calls retain the
compiler-owned compile-time provider route even when that source package is loaded.
In a `comptime` binding or `build: fn`, the evaluator hands each `lex`, `parse`,
or `check` call to the compiler, which answers with its own lexer, parser, and
check query over the call's argument (the same check `jet inspect compiler
check` runs). A comptime constant that holds one of these values is read back
as the compiler's value for that constant, so `PARSED: comptime { … }` can
feed `compiler.check(PARSED)`.

The package-view field matrix is versioned by
`PACKAGE_MODEL_SCHEMA_VERSION = 1` (D-PACKAGE-MODEL1). The four views have
separate meanings:

| View | Shape | Ordering and scope |
| --- | --- | --- |
| `manifest()` | `CompilerManifest` | The uncomposed `package.jet` or inline package carrier. `dependencies` and `outputs` use manifest map-key order; `packages` and `build_profiles` keep declaration order. |
| `package()` | `CompilerPackage` | The package after declared Config files compose successfully. It uses the manifest fields and the canonical current-package identity; `name` and `version` remain the `#package` values. |
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
Declared Config files contribute package facts and build identity, not ordinary
source declarations; a Config binding cannot shadow a package function.

```jet
use compiler: core.compiler

@manifest: compiler.manifest() ?? panic("manifest")
@package: compiler.package() ?? panic("package")
@lock: compiler.lock() ?? panic("lock")
@profiles: compiler.profiles() ?? panic("profiles")
```

### `core.mod` — pinned library loading

`core.mod.load` loads a project-contained compiled Jet library under an explicit
read grant. The provider validates the artifact, package boundary, and ABI
before library code runs; the returned `Mod` owns its provider handle and
releases it when dropped. The statically linked native runtime has no dynamic
loader: it grants, reads, checks, digests and stages the library, then refuses
to map it with `ModError.Unsupported`.

```jet
use library: core.mod

run: fn() {
    library.load(".jet/build/loadable.jetlib", grant: {read: [".jet/build"]}) ?? return
}
```

The grant must be explicit and non-empty. `core.mod` does not dynamically
search arbitrary paths, bypass a package boundary, or manufacture a detached
handle. The source-facing `CompiledModule` inspection helpers are:

| API | Result | Description |
| --- | --- | --- |
| `load(path, grant)` | `-[FS, Exec]> Mod ModError!` | Load one granted compiled library. |
| `is_loaded(compiled)` | `Bool` | Inspect a `CompiledModule` carrier. |
| `path(compiled)` | `String` | Return the carrier's recorded path. |
| `unload(compiled)` | `-[FS]> CompiledModule ModError!` | Return `ModError.Unsupported`; provider-owned `Mod` handles unload on drop. |
| `ModError` | `#Error` enum | D-MOD-ERR1=A. `Denied(reason)`: the grant refuses the path, or the library declares an effect the load site does not grant (E1339). `NotFound(reason)`: nothing is at the path. `Invalid(reason)`: the file is damaged or is not a library. `Unsupported(reason)`: another Jet version, ABI or platform built it (E1338, E1341), or loading is unavailable on this target. |

## Errors and optional values

Core's fallible calls use a success value plus a named error carrier. A plain
call propagates a failure from the enclosing function; `??` handles it as a
fallback, and `?(text)` adds context while preserving propagation:

```jet
use fs: core.files

run: fn() {
    text: fs.read("data.txt")
    fs.write("out.txt", text.to_upper())
}

text: fs.read("data.txt") ?? ""
annotated: fs.read("data.txt")?("reading the input for out.txt")
```

A function without an explicit error type uses the default `Err`. Core's
standard error family, including `IOError` and `EncodingError`, converts into
that default carrier. For an application-owned target, declare one conversion
and name it in the function contract:

```jet
#Error
AppErr: enum { Read(IOError) }

impl IOError -> AppErr { return AppErr.Read(self) }

load: fn() -> String AppErr! {
    text: fs.read("data.txt")
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
price: lookup_price(id)
qty: lookup_qty(id)
total: price.zip(qty).map((pair) -> pair.a * pair.b)
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
| `[T]` | `[a, b]` | Eager closure methods, sequence adapters, reducers, `lazy`, `to_set`, `join`, `compare`, `split`, `starts_with`, `ends_with`, `slice`, `fill`, `fill_repeat`, `copy`, and `equal`. |
| `[K:V]` | `["a": 1]`, `Map.new()`, `Map.from_keys(keys, default)` | Ordered `keys`, `values`, `get`, `has_key`, `add`, `add_new`, `remove`, `pop`, `merge`, `copy`, `equal`, `map`, `filter`, `flat_map`, `fold`, `intersection`, `slice`, `top_n`, and reducers. |
| `Set<T>` | `Set.new()`, `Set.from(xs)` | Hash-set `add`, `remove`, `pop`, `has`, set algebra, subset/disjoint checks, `copy`, `first`, `values`, closure methods, `to_list`, and size/clear operations. |
| `Rank<T>` | `Rank.new()`, `Rank.from(xs)`, `Rank.by(key)`, `Rank.by_desc(key)`, `Rank.from_by(xs, key)`, `Rank.from_by_desc(xs, key)` | Ordered set insertion, removal, membership, first/last, set algebra, and list conversion. `new` and `from` keep items in their own ascending order. The `by` forms order by a pure key function (`(w: String) -> w.len()`, an `Int`, `Float`, `String` or other natively ordered key): `by` keeps the smallest key first and `by_desc` the largest, and items with equal keys stay in the order they went in. Membership is still the item's own equality, so two items with equal keys are both kept. Set algebra results keep the receiver's order. A key function with effects is rejected (E0747). |
| `Queue<T>` | `Queue.new()`, `Queue.init(xs)` | Double-ended push/pop and peek, capacity, lookup/delete, list conversion, join, reverse, split, and size/clear operations. |
| `PriorityQueue<T>` | `PriorityQueue.new()`, `PriorityQueue.from(xs)`, `PriorityQueue.by(key)`, `PriorityQueue.by_desc(key)`, `PriorityQueue.from_by(xs, key)`, `PriorityQueue.from_by_desc(xs, key)` | `push`, `pop`, `peek`, `to_sorted_list`, `remove`, and size/clear operations. `new` and `from` pop the largest item first by its own order. The `by` forms order by a pure key function (`(s: Score) -> s.points`, an `Int`, `Float`, `String` or other natively ordered key): `by` pops the smallest key first and `by_desc` the largest; items with equal keys pop in the order they went in, and `to_sorted_list` lists them in pop order. A key function with effects is rejected (E0747). |
| `Cache<K,V>` | `Cache<K,V>.new(capacity)` | Bounded least-recently-used `add`, `add_new`, `get`, `remove`, `has_key`, `keys`, `capacity`, and size/clear operations. The constructor names its key and value types (`Cache<String, Int>.new(2)`): no argument carries them, and a later line never settles them (D-INFER-LEVEL1). `add` and `get` make a key the most recent; `add` returns a held key's old value, and a new key into a full cache evicts the least recent entry. `add_new` on a held key and `has_key` leave the order alone. `keys` lists the most recent first. A negative capacity is 0, which keeps nothing. |
| `Tally<T>` | `Tally.new()`, `Tally.from(xs)` | Counting `add`, `remove`, `has`, `count`, list conversion, and size/clear operations. |
| `Bits` | `Bits.new()` | Integer-set `add`, `remove`, `has`, `count`, list conversion, `Bits.copy()` (an independent set: changing the copy leaves the original as it was), and size/clear operations. |
| `Bytes` | `Bytes.new()`, `Bytes.with_capacity(n)`, `Bytes.from(bytes)` | Byte-buffer reads/writes, cursor movement, text conversion, splitting/joining, comparison, copying, flushing, and closing. |

`partition` is stable, and the list sort family preserves source order for equal
keys. `min_by` and `max_by` retain the last source item for equal keys.
`sort`, `sort_desc`, `min` and `max` on named tuples compare members in label
order, not written order (S73, D-TUPLE-ORDER1=A). So `(key, index)` sorts by
`index`, then `key`; use `sort_by` to put another member first.
`binary_search` returns a matching index, not an insertion position or a
promise to return the first equal item. `counts()` returns a map of element
counts; `top_n(n)` orders by descending count and ascending key for ties.
`para_map` preserves source order, and `para_map(f, limit: n)` bounds workers
for that call.

`xs.fill(range, value)` sets every element of `xs[range]` to `value`, in
place, as Rust's `slice.fill` and C++'s `std::fill` do.
`xs.fill_repeat(range, pattern)` repeats the list `pattern` across the same
span: element `range.start + i` becomes `pattern[i % pattern.len()]`, so a
four-byte RGBA pixel or a gradient row tiles a pixel row. A span that is not a
whole number of patterns ends with a partial pattern. Neither method clamps:
a range outside `0..xs.len()` stops with the bounds fault that `xs[range]`
gives, and an empty `pattern` stops with the bounds fault of `pattern[0]`,
even over an empty range. Both write plain data such as `[U8]` as block
stores, not a loop of element stores: a pattern of 1, 2, 4 or 8 bytes as one
repeated word, any other as block copies.

```jet
pixels := [U8]{0, 0, 0, 0, 0, 0, 0, 0, 0, 0}
pixels.fill(0..<2, 255)
pixels.fill_repeat(2..<10, [U8]{1, 2, 3})
print(pixels) // [255, 255, 1, 2, 3, 1, 2, 3, 1, 2]
```

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
is not a Core API. An `Iter<T>` `fill` adapter, `cycle_n`, and `duplicate`
are declined or mapped to the existing names under D-ITER-DECLINE1; the list
method `fill` above writes in place.

`next()` is the one partial pull (D-ITER-RESUME1, which amends
D-ITER-DECLINE1 for `next` only). It needs exclusive access to a writable
binding, written `items.next()`, pulls exactly one item, and leaves the
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
left: [1, 2, 3]
right: [10, 20]

loop row in left.zip_pad(right, fill: 0) {
    print(row.b)
}
```

Examples covering the eager/lazy boundary and the adapter ledger are in
[`iter_adapters.jet`](../../../Examples/Collections/Iterators/IterAdapters.jet),
[`lazy_iter.jet`](../../../Examples/Collections/Iterators/IterAdapters.jet),
[`list_surface.jet`](../../../tests/run/CollectionsAndMethods/ListSurface.jet),
[`map_surface.jet`](../../../tests/run/CollectionsAndMethods/MapSurface.jet), and
[`set.jet`](../../../Examples/Collections/Set.jet).

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
D-CORE-EAGER2). [`operation_contract.jet`](../../../Examples/Collections/OperationContract.jet)
runs each of these cases.

<!-- BEGIN GENERATED COLLECTION OPERATIONS -->
<!-- Source: Compiler/JetFoundation/Source/Collections.jet; regenerate with jet Tools/agent/CoreSourceRows.jet -->

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
| Disjoint | eager proof, borrowed windows | tracked exclusive ViewMut slots; edit_disjoint returns Unit | edit_disjoint invokes a callback with two lent views; the other methods have none | no callback runs for an invalid selection | String error for invalid bounds or overlapping selections | view descriptors only; the owner's elements are never copied |
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
| `Set<T>` | `filter((item) -> Bool)` | borrows | Visit | empty list | current |
| `Set<T>` | `each((item) -> T)` | borrows | Each | Unit | current |
| `Set<T>` | `all((item) -> Bool)` | borrows | Decide | true | current |
| `Set<T>` | `flat_map((item) -> T)` | borrows | Visit | empty iterator | current |
| `Set<T>` | `fold(seed, (acc, item) -> acc)` | borrows | Fold | the seed | current |
| `Set<T>` | `min()` | borrows | Lookup | None | current |
| `Set<T>` | `max()` | borrows | Lookup | None | current |
| `Set<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
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
| `[T]` | `fill(Range, item)` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `fill_repeat(Range, [item])` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `get(Int)` | borrows | Lookup | None | current |
| `[T]` | `index_of(item)` | borrows | Lookup | None | current |
| `[T]` | `index(item)` | borrows | Lookup | None | current |
| `[T]` | `contains(item)` | borrows | Measure | false | current |
| `[T]` | `count(item)` | borrows | Measure | 0 | current |
| `[T]` | `update_first((item) -> Bool, item)` | exclusive, in place | Decide | false | current |
| `[T]` | `split_write(Int)` | exclusive, in place | Disjoint | Ok(two empty write views) at zero | current |
| `[T]` | `get_disjoint_write([Int])` | exclusive, in place | Disjoint | Ok(empty view list) for no indexes | current |
| `[T]` | `edit_disjoint([Int], (ViewMut<item>, ViewMut<item>) -> T)` | exclusive, in place | Disjoint | Err for a selection that does not name two items | current |
| `[T]` | `replace(item, item)` | exclusive, in place | Decide | false | current |
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
| `[T]`, `Iter<T>` | `filter((item) -> Bool)` | borrows a list; consumes an iterator | Visit | empty list or iterator | current |
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
| `[T]` | `last()` | borrows | Lookup | None | current |
| `Iter<T>` | `to_set()` | consumes | Drain | empty Set | current |
| `[T]`, `Iter<T>` | `try_collect()` | borrows a list; consumes an iterator | Drain | Ok(empty list) | current |
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
| `[K:V]` | `len()` | borrows | Measure | 0 | current |
| `[K:V]` | `is_empty()` | borrows | Measure | true | current |
| `[K:V]` | `replace(key, value)` | exclusive, in place | Mutate | None | current |
| `[K:V]` | `keys()` | borrows | Adapt | empty iterator | current |
| `[K:V]` | `values()` | borrows | Adapt | empty iterator | current |
| `[K:V]` | `first()` | borrows | Lookup | None | current |
| `[K:V]` | `min()` | borrows | Lookup | None | current |
| `[K:V]` | `max()` | borrows | Lookup | None | current |
| `[K:V]` | `to_list()` | borrows | Build | empty list | current |
| `[K:V]` | `items()` | borrows | Build | empty list | current |
| `View<T>` | `len()` | borrows | Measure | 0 | current |
| `View<T>` | `is_empty()` | borrows | Measure | true | current |
| `View<T>` | `get(Int)` | borrows | Lookup | None | current |
| `View<T>` | `index_of(item)` | borrows | Lookup | None | current |
| `View<T>` | `first()` | borrows | Lookup | None | current |
| `View<T>` | `last()` | borrows | Lookup | None | current |
| `View<T>` | `contains(item)` | borrows | Measure | false | current |
| `View<T>` | `join(String)` | borrows | Build | empty string | current |
| `Bits` | `len()` | borrows | Measure | 0 | current |
| `Bits` | `count()` | borrows | Measure | 0 | current |
| `Bits` | `is_empty()` | borrows | Measure | true | current |
| `Bits` | `to_list()` | borrows | Build | empty list | current |

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
`Path` value through `String | Path` parameters. Read access borrows either
member; widening a path argument does not consume the caller's value.
Filesystem calls carry the `FS` effect; current-directory and
environment-derived helpers also carry `Env` (D-FILES-WRITE1).
`read_bytes` also borrows a String view (for example, `path.trim()`); the
runtime does not materialize an owned path just to perform the read.

A relative path is resolved from the process working directory. An entry-local
file can be opened directly with `fs.read("input.txt")`; do not read the
working directory and join it to an argument that is already relative. Use a
`Path` when intentionally composing components.

```jet
use fs: core.files

run: fn() {
    path: "/tmp/notes.txt"
    fs.write(path, "hello\n") ?? return
    fs.append_all(path, "world\n") ?? return
    print(fs.read(path) ?? return)
    print(fs.exists(path))
    print(fs.is_dir("/tmp"))
    entries: fs.list_dir("/tmp") ?? return
    print(entries.len())
}
```

Whole-file and metadata functions. `read`, `read_bytes`, `write`,
`write_bytes`, `append_all`, `map`, `read_at`, `write_at`, `write_atomic` and
`fsync` take their `path` as a `String` or a `Path`:

| API | Result | Description |
| --- | --- | --- |
| `read(path)` | `String !IOError` | Read a UTF-8 text file. Invalid UTF-8 is `IOError.InvalidInput` with cause `stream did not contain valid UTF-8`. |
| `read_bytes(path)` | `[U8] !IOError` | Read raw bytes. |
| `write(path, text)` | `!IOError` | Create or replace a text file. |
| `write_bytes(path, bytes)` | `!IOError` | Create or replace raw bytes. |
| `append_all(path, text)` | `!IOError` | Append text in one whole-file call. |
| `map(path)` | `MappedFile !IOError` | Map a file into the checked byte-view carrier. |
| `read_at(path, offset, count)` | `[U8] !IOError` | Read one bounded byte range; negative offset or count is clamped to zero. |
| `write_at(path, offset, bytes)` | `!IOError` | Write bytes at an offset; negative offset is clamped to zero. |
| `write_atomic(path, bytes)` | `!IOError` | Reserve one of 128 same-directory `.jet_tmp_<pid>_<sequence>` names, write and sync it, preserve available target permissions, close, then rename; on Unix, sync the parent after publication. |
| `fsync(path)` | `!IOError` | Flush a file, or a folder so earlier renames inside it are durable. |
| `space(path)` | `DiskSpace !IOError` | The room on the file system holding `path` (statvfs), in bytes: `total`, `free`, and `available` (what an unprivileged program may fill). `used()` is `total - free`; `used_percent()` is `df`'s figure, used over used plus available, 0.0 to 100.0. |
| `can_read(path)` / `can_write(path)` / `can_execute(path)` | `Bool` | May this program read, write or run (search, for a directory) `path` now, judged with its effective user and groups (faccessat, AT_EACCESS). A missing or unreadable path is `false`. The answer can change before the next call, so code that must open the file handles the open's error instead. |
| `open(path, links:)` / `create(path, if_exists:, mode:, links:)` / `append(path, mode:, links:)` | `FileReader` / `FileWriter !IOError` | Open a close-on-exec stream handle. `if_exists: .Refuse` creates exclusively, `mode` sets a new file's bits before the umask, and `links: .Refuse` refuses a final symbolic link. |
| `open_directory(path)` | `Directory !IOError` | Hold a directory whose last part is a real directory, not a link. |
| `samefile(a, b)` | `Bool !IOError` | Compare the two entries' `FileID`s. |
| `extended_attribute_names(path)` / `extended_attribute(path, name)` / `set_extended_attribute(path, name, value)` | `[String]` / `[U8]?` / `!IOError` | Act on the path itself, never a final link's target. Names are sorted. |
| `exists(path)` / `is_dir(path)` / `is_file(path)` | `Bool` | Return false when the path cannot be observed; these helpers do not raise a missing-path error. |
| `stat(path)` / `lstat(path)` | `Stat !IOError` | Describe the entry itself, never a final link's target. |
| `set_mode(path, mode)` | `!IOError` | Set Unix mode bits; non-Unix providers use their documented readonly mapping. |
| `list_dir(path, matching: String?{None})` | `[DirEntry] !IOError` | List entries in name order. `matching` keeps the names that match a shell pattern (`*` any bytes, `?` one byte), so `list_dir(dir, matching: "*.files")` is `dir/*.files`. |
| `create_dir_all(path)` | `!IOError` | Create a directory and any missing parents; an existing directory is success. |
| `remove(path)` / `remove_dir(path)` / `remove_all(path)` | `!IOError` | Remove a file, empty directory, or tree. |
| `copy(from, to)` / `copy_dir(from, to)` | `!IOError` | Copy one regular file or a directory tree. Directory copy does not dereference symlinks or special entries. |
| `rename(from, to, if_exists:)` | `!IOError` | Rename or move a path; `.Replace` (default), `.Refuse`, or `.Swap`. |
| `symlink(original, link, replace: Bool{false})` / `hard_link(original, link)` | `!IOError` | Create symbolic or hard links. `replace: true` first removes a link or file at `link`, so a link to a directory is replaced rather than followed (`ln -sfn`); a real directory there is an error. |
| `read_link(path)` | `String !IOError` | Read a symbolic-link target. |
| `canonicalize(path, missing_last: Bool{false})` / `absolute(path)` | `String !IOError` | Resolve an existing path with symlinks, or make an absolute lexical path without requiring existence. `missing_last: true` lets the last part be missing, as `readlink -f` does: the parent is resolved and the last name joined to it. |
| `walk(root, ignore, skip: [String]{[String]{}})` | `Walk` | Traverse recursively with an optional ignore-file name, lazily: a loop over the `Walk` yields each entry as `WalkEntry !IOError` in path order. An entry whose name is in `skip` is neither yielded nor entered (`find -name NAME -prune`). `.list()` collects them all as `[WalkEntry] !IOError`. |
| `walk_files(root, ignore, skip:)` | `Walk` | The same walk, keeping regular files only. |
| `glob(pattern)` | `[String] !IOError` | Return sorted shown paths matching the native byte glob grammar. |
| `temp_dir(prefix)` / `temp_file(prefix)` | `TempDir` / `TempFile !IOError` | Create temporary resources whose handles retain `.path`. |
| `lock(path, timeout: Duration?{None})` / `try_lock(path)` | `FileLock !IOError` / `FileLock? !IOError` | Acquire an exclusive system lock; `try_lock` returns None only for contention. |

`exists` and `is_dir` follow a final symbolic link; a broken link returns
false. Their false-on-host-error policy is Jet-owned; the host boundary
supplies only the following-metadata fact or the host error.

`glob` and `copy_dir` own their DFS policy in `Core/Files/Files.jet`. Native
entries contexts retain `DirEntry` and source/destination paths, not reconstructed
UTF-8 names. Directory copy processes unsorted `ReadDir` order, copies only
regular files, and releases each entry after use. All host failures report the
top-level source path, even for destination writes. Read then write fault
preflights occur once, before traversal.
Comptime and the resident interpreter marshal those same registered native
entry facts; neither routes public glob or directory copy to a Rust traversal.

Glob first collects each directory's children, orders native basenames, then
visits depth-first without following directory symlinks. `*` matches any bytes
(including separators), `?` consumes one byte, and brackets are literal.
The starting directory is the prefix before the first `*`/`?`, cut at its last
host separator; absent or empty directory prefixes mean `.`. Matching uses the
native shown path unchanged, including a leading `./`, and there is no special
literal-path existence shortcut. All glob errors report the original pattern.

`absolute` keeps host lexical spelling, including `.` and `..`, repeated
separators, and nonexistent paths. It leaves a host-absolute input unchanged;
otherwise Jet obtains the current directory and joins using host path grammar.
It does not apply the portable `collapse` operation.

`mktemp(prefix)` only composes a name; it does not create the file. Jet replaces
each non-ASCII-alphanumeric Unicode scalar with one underscore and combines the
result with the process ID and the unsigned Unix-nanosecond clock sample, using
zero when that sample is unavailable. The directory is the host temporary
directory (not the logical environment overlay). Its effects include environment
and clock observation as well as the filesystem surface.

`exists` and `is_dir` follow a final symbolic link; a broken link returns
false. Their false-on-host-error policy is Jet-owned; the host boundary
supplies only the following-metadata fact or the host error.

`walk` and `walk_files` accept the optional `ignore` value;
`fs.walk(root)` is the convenience form. A walk reads nothing until it is
looped over. It keeps at most one open directory and one directory's entries
per level in hand. A directory that cannot be read yields its error in place
of its entries, and the walk goes on. Leaving the loop early closes the walk.
`.list()` stops at the first error. `DirEntry` has `name`, `path`, and
`is_dir`. `Stat` has `size`, `kind`, `mode`, `readonly`, `is_file`, `is_dir`,
`is_symlink`, `id: FileID{device, inode}`, `links`, `owner: Int?` (None on
Windows), `size_on_disk`, `modified` and `changed` (`DateTime`, nanosecond
precision), and `created: DateTime?` (None where the file system keeps no
birth time). `mode` is the permission bits (0o0..0o7777, the domain
`set_mode` takes); the file type is `kind`. `WalkEntry` exposes `path`,
`relative`, `is_dir`, and `depth` through the checked type projection.
The compiler-owned `IOOperation` enum describes the operation within this
failure context. Its annotation is available to vetted Core source, not a new
application-visible type export; user annotations retain E0119.

Store-grade contracts (D-CORE-FILEERROR1, D-CORE-FILEOPEN1, D-CORE-FILESTAT1,
D-CORE-FILEDIR1, D-CORE-HANDLEFD1, D-CORE-FILERENAME1, D-CORE-FILEXATTR1,
D-CORE-FILELOCK1):

- `IOError.AlreadyExists` reports an existing target a call was asked not to
  replace (`EEXIST`, `ERROR_FILE_EXISTS`, `ERROR_ALREADY_EXISTS`).
  `IOError.Unsupported` reports a request this system or file system cannot
  perform (`ENOTSUP`, `EOPNOTSUPP`, `ENOSYS`, `ERROR_NOT_SUPPORTED`). Both
  carry the shared `IOContext`; match the kind, never `os_code`.
- `files.lock(path)` creates a missing regular lock file with mode `0o600`
  before umask, opens it close-on-exec without following a final link, and
  waits for an exclusive advisory system lock. `timeout:` bounds the wait
  and reports `IOError.TimedOut`; `files.try_lock(path)` returns None only
  for contention. After acquisition, Core revalidates the opened identity
  against the path and retries a replaced binding. `FileLock.path` remains
  read-only. Consuming `close(lock)`, scope exit, or process exit releases
  the kernel lock; release never removes the lock file. Unix uses `flock`;
  Windows uses `LockFileEx` and denies delete sharing. A directory is not a
  lock file; holding a directory and locking a regular file are distinct.
- A `Directory` keeps working after its path is renamed or removed. Its
  methods (`open`, `create`, `open_directory`, `create_directory`, `list`,
  `stat`, `entry_stat`, `read_link`, `rename(from, to, into:, if_exists:)`,
  `remove`, `remove_directory`, `fsync`) take a relative name of plain parts
  separated by `/`. Each part is opened without following a link; `..`, `.`,
  empty parts, and absolute names are `IOError.InvalidInput`. `create_directory`
  on an existing name is `AlreadyExists`. The handle is close-on-exec and
  closes at scope exit.
  `list()` returns `DirectoryEntry`: `raw_name` preserves the native basename
  exactly, and `name_is_unicode` says whether `name` is lossless UTF-8. For an
  invalid UTF-8 name, `name` and `path` are display text, not an identity.
- `directory.lock()` acquires a nonblocking exclusive advisory flock on the
  held directory itself. Its `AdvisoryLock` owns an independent close-on-exec
  descriptor, so renaming the directory or closing the original `Directory`
  does not release the lock. A contended lock is an `IOError`; explicit
  `close(lock)` or scope exit releases it, without deleting a name. Systems
  without directory flock report `Unsupported`. This differs from locking
  the regular file named by `files.lock(path)`.
- `directory.duplicate()` returns a fresh `Directory` with the same `path`
  that holds the same directory through its own close-on-exec descriptor,
  opened through the held one, never by path. Closing either handle leaves
  the other open, and both keep naming the directory after its path is
  renamed or replaced; a closed original is `IOError.Closed`. It is how a
  parent passes a directory to a child and keeps using its own:
  `spec.keep_open(directory.duplicate() ?? return, at: 9)`.
- Publish through a held directory by exclusively creating a temporary sibling,
  writing and setting mode through its handle, calling `writer.fsync()`, then
  `directory.rename(temp, destination)` and `directory.fsync()`. Rename replaces
  the destination entry atomically: it does not write through a destination
  symlink or alter another hard link's contents. Metadata from `reader.stat()`
  describes the opened identity even after the destination is replaced.
  Read-only kernel calls borrow their argument place; inspecting a held reader
  or writer does not copy or duplicate the native owner.
- `rename(..., if_exists: .Refuse)` is `AlreadyExists` when the target exists;
  a file system without no-replace renames falls back to link-then-unlink for
  files and reports `Unsupported` for folders. `.Swap` exchanges two existing
  names in one step; a missing name is `NotFound` and a system without
  exchange is `Unsupported`.
- Extended attributes on Windows, or on a file system without them, report
  no names and no values; writes are `Unsupported`.
  Native optional replies are converted to the owned absence carrier at the
  kernel boundary, including inside `IOError!`; an absent value is not an I/O
  failure, and an actual I/O failure retains its original report.

Store-grade contracts (D-CORE-FILEERROR1, D-CORE-FILEOPEN1, D-CORE-FILESTAT1,
D-CORE-FILEDIR1, D-CORE-HANDLEFD1, D-CORE-FILERENAME1, D-CORE-FILEXATTR1):

- `IOError.AlreadyExists` reports an existing target a call was asked not to
  replace (`EEXIST`, `ERROR_FILE_EXISTS`, `ERROR_ALREADY_EXISTS`).
  `IOError.Unsupported` reports a request this system or file system cannot
  perform (`ENOTSUP`, `EOPNOTSUPP`, `ENOSYS`, `ERROR_NOT_SUPPORTED`). Both
  carry the shared `IOContext`; match the kind, never `os_code`.
- A `Directory` keeps working after its path is renamed or removed. Its
  methods (`open`, `create`, `open_directory`, `create_directory`, `list`,
  `stat`, `entry_stat`, `read_link`, `rename(from, to, into:, if_exists:)`,
  `remove`, `remove_directory`, `fsync`) take a relative name of plain parts
  separated by `/`. Each part is opened without following a link; `..`, `.`,
  empty parts, and absolute names are `IOError.InvalidInput`. `create_directory`
  on an existing name is `AlreadyExists`. The handle is close-on-exec and
  closes at scope exit.
- `rename(..., if_exists: .Refuse)` is `AlreadyExists` when the target exists;
  a file system without no-replace renames falls back to link-then-unlink for
  files and reports `Unsupported` for folders. `.Swap` exchanges two existing
  names in one step; a missing name is `NotFound` and a system without
  exchange is `Unsupported`.
- Extended attributes on Windows, or on a file system without them, report
  no names and no values; writes are `Unsupported`.

The retired raw-String `read_dir` helper is not a reserved method name:
user struct and trait methods named `read_dir` participate in normal resolution.

Mapped windows have byte element type `View<U8>`: `window` and `window_len`
return `View<U8> !IOError`, and `lines()` yields `ViewIter<View<U8>>`.
Copying a line with `line.copy()` produces `[U8]`, not `[[U8]]`. A `FileScope`
parameter imported as `files.FileScope` retains its checked `read` method;
source qualification does not change its capability dispatch.
Scoped reads select only captured authority roots, reject parent/root/prefix
components in the relative name, and keep descriptor-relative no-follow opening
as the security boundary. If every eligible root fails, the last OS error is
returned; invalid UTF-8 retains its byte offset and invalid-sequence length.


Streaming constructors keep memory bounded:

```jet
use files: core.files

count_lines: fn(path: String) -> Int IOError! {
    reader: files.open(path)
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
| `reader.read_line()` | `?String !IOError` | Read from the shared buffered position, remove all trailing CR/LF bytes, and return `None` only at EOF. A final unterminated line is returned; invalid UTF-8 is `IOError.InvalidInput`. |
| `reader.read(max)` | `[U8] !IOError` | Read up to `max` bytes from the shared position; an empty list is EOF. |
| `reader.read_into(buffer: &[U8], limit)` | `Int !IOError` | Append up to `limit` bytes from the shared position to the caller's `buffer` and return the count; 0 is EOF, and a limit below one is `IOError.InvalidInput` before any read. A cleared, reused buffer reads without allocating. |
| `reader.stat()` / `writer.stat()` | `Stat !IOError` | Describe the open file itself (fstat); a writer's buffered bytes count only after a flush. |
| `reader.lines()` in a loop source | stream of `String` | Pull lines without loading the complete file. |
| `writer.write_line(text)` / `writer.write_all(bytes)` | `!IOError` | Write text and a newline, or every byte of `bytes` (buffered). |
| `writer.flush()` / `writer.fsync()` | `!IOError` | Flush pending writer bytes; `fsync` also makes the file's data durable. |
| `writer.set_mode(mode)` | `!IOError` | Set the open file's permission bits; Windows keeps only read-only. |
| `writer.allocate(size)` | `!IOError` | Reserve disk space for the first `size` bytes (posix_fallocate); a shorter file grows to `size` with zero bytes. A file system that cannot reserve is `IOError.Unsupported`. |
| `reader.advise(advice)` | `!IOError` | Tell the system how the file will be read (posix_fadvise): `Advice.Normal`, `Sequential`, `Random`, `WillNeed`, `DontNeed` or `NoReuse`. It plans caching only; a pipe accepts any advice. |
| `mapped.advise(advice)` | `!IOError` | Tell the system how a `MappedFile`'s bytes will be read (posix_madvise over the whole map), with the same `Advice` values; `NoReuse` marks the pages cold. It plans paging only; the map holds the same contents whatever it says. |

Readers, writers, temporary resources, and locks own their provider resource
until lexical scope exit or an explicit `close(handle)` consumes the handle.
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
codec behavior; the parser follows RFC 3986-style rules. Non-ASCII host labels
are lowercased byte-wise and Punycode-encoded (`xn--`, RFC 3492); no WHATWG
IDNA mapping is applied.

```jet
use url: core.net.url

run: fn() {
    base: url.parse("https://example.test/a/./b/../c?x=1") ?? return
    next: base.join("../notify?user=ada%20lovelace") ?? return
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
| `u.scheme()` / `u.path()` / `u.query()` | `String` | Read parsed URL components. |
| `u.host()` / `u.port()` / `u.fragment()` | `String?` / `Int?` / `String?` | Read optional components; `port()` is present only for an explicitly written authority port. |
| `u.path_segments()` / `u.query_pairs()` | `[String]` / `[[String]]` | Read decoded path and repeated query pairs. |
| `u.normalize()` / `u.join(relative)` | `URL` / `URL !URLError` | Return a normalized URL or resolve a relative reference. |
| `u.to_string()` / `u.default_port()` | `String` / `?Int` | Render the URL, keeping an explicitly written port, or read the scheme's registered default port. |
| `u.username()` / `u.password()` / `u.userinfo()` / `u.authority()` | `String` | Read decoded credentials, `user:password`, or `userinfo@host:port`. |
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
`v7(clock)` reads the caller's clock in milliseconds. A manual `Clock.new`
therefore fixes the 48-bit timestamp prefix while the tail still comes from
the same random provider; the version and variant bits are set in Jet.

```jet
use uuid: core.crypto.uuid

run: fn() {
    id: uuid.v4() ?? return
    normalized: uuid.parse("6BA7B810-9DAD-11D1-80B4-00C04FD430C8") ?? return
    same_every_time: uuid.v5("6ba7b810-9dad-11d1-80b4-00c04fd430c8", "python.org") ?? return
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
The source parsers check the remaining body length before projecting its bytes.
Accepted bodies use one owned byte-slice projection; oversized bodies are
rejected without a body allocation.

| API | Result | Description |
| --- | --- | --- |
| `method_parse(token)` / `method_text(method)` | `Method` / `String` | Parse or render standard and custom methods. |
| `version_parse(token)` / `version_text(version)` | `Version` / `String` | Parse or render HTTP/1.0 through HTTP/3 labels. |
| `headers()` / `headers_get(h, name)` | `Headers` / `?String` | Create headers or read a case-insensitive first value. |
| `headers_all(h, name)` / `headers_set` / `headers_append` / `headers_remove` | header values | Preserve ordered fields while replacing, appending, or removing names. |
| `body_empty()` / `body_text(text)` / `body_bytes(bytes)` | `Body` | Construct the checked empty, text, or byte body variants. |
| `Body.reader(reader)` / `Body.reader(reader, length)` | `Body !HTTPError` | Take a `FileReader` and read its bytes when the body is made. With no length, read to end of file, at most 1 GiB. With a length, read exactly that many bytes: more than 1 GiB is `BodyTooLarge`, and a negative length or a source that ends short is `InvalidFraming`. The upload is not streamed. |
| `Body.multipart(fields)` | `Body` | `multipart/form-data` text fields in map order. The boundary is the first `jet-http-boundary-%016x` that no field contains, and the body's first line is `--` plus that boundary, which the message's `Content-Type` names. |
| `request.body()` / `response.body()` | lazy `Body` | Keep the transport body lazy; `body.text(limit)` and `body.bytes(limit)` are fallible, bounded consumption operations, not construction of the owned source variants. |
| `get(url)` / `post(url, body)` | `HTTPResponse !HTTPError` | Perform one-shot GET or POST calls with URL and body checks. |
| `put(url, body)` / `patch(url, body)` | `HTTPResponse !HTTPError` | Perform one-shot PUT or PATCH calls. |
| `delete(url)` / `head(url)` | `HTTPResponse !HTTPError` | Perform one-shot DELETE or HEAD calls. |
| `exchange(method, url, body, header_lines)` | `HTTPResponse !HTTPError` | Validate and perform a request described by a method and header lines. |
| `parse_request(raw)` / `parse_response(url, raw)` | request/response `!HTTPError` | Parse bounded CRLF-framed messages and reject invalid framing or encoding. A parsed request keeps its body as the wire bytes (`Body.Bytes`), sliced once after every length check; a text read decodes it then. |
| `with_body(request, body)` / `request_with(method, url, headers, body)` / `response_ok(body)` | request/response | Carry the body as given: a `Body.Bytes` body stays bytes. |
| `query()` / `query_get` / `query_all` / `query_set` | query values | Build and inspect repeated query pairs. |
| `query_decode(text)` / `form_decode(text)` | `Query` | Split a query or an `application/x-www-form-urlencoded` form the WHATWG way: one leading `?` is stripped, empty segments are skipped, a field without `=` has an empty value, and duplicate names stay in order (`query_get` returns the first). `+` is a space and `%HH` one byte, decoded once; a broken escape such as `%QZ` stays as written, and bytes that are not UTF-8 become U+FFFD, one per maximal invalid subpart. Input above 1 MiB gives an empty `Query`. |
| `query_decode_strict(text)` / `form_decode_strict(text)` | `Query !FormDecodeError` | The same split, refusing instead of replacing: `Escape` for a broken escape, `UTF8` for bytes that are not UTF-8, `TooLarge` above 1 MiB. Code whose decoded value picks a file, path, account or other identity uses these, since `%FF` and `%FE` both decode to U+FFFD when tolerant. |
| `form_component_decode(text)` / `form_component_decode_strict(text)` | `String` / `String !FormDecodeError` | Decode one field name or value by the same rules. |
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
| `Client.new().proxy(policy)` | `HTTPClient` | Select `.FromEnvironment` (default), `.None`, or `.URL(proxy)`. |
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
Cloning a request or response, or obtaining its body, does not create a fresh
read: those body handles share the same single-use consumption state.

Taking a chunk iterator consumes the body once, even if only part of it is
read. Dropping that iterator or closing the body closes its backing resource
exactly once; closing followed by dropping does not close it again. End of
input marks a body drained. A read error or delivered task stop does not mark
it drained, and a task stop keeps its cancellation, deadline, budget, or panic
meaning rather than becoming an I/O error. The public `Body.Empty`,
`Body.Text`, and `Body.Bytes` constructor variants remain unchanged.

### `core.http.server` — HTTP serving

`core.http.server` builds a typed multiplexer and serves HTTP/1.1 over
`core.net` TCP, one request per connection with `Connection: close`. `bind`
and `serve` accept optional `HTTPServerTLS` and optional deadlines; `serve_once`
and `serve_once_listener` provide testable one-request entry points. A server
given TLS material refuses to bind, because no listener TLS adapter exists yet.
The Jet `tls(cert, key)` constructor preserves both PEM strings verbatim,
including empty or malformed material. Loading and validating those credentials
belongs to the listener's TLS setup, not to construction.

`core.http.serve(mux, deadline)` binds `mux` on 127.0.0.1:8080 and returns the
`HTTPServer`, panicking when the address cannot be bound.

An `HTTPServer` answers connections from `serve()` (alias `wait()`) until
`shutdown(grace)`, where `grace` defaults to zero. Each request is answered
before the next is accepted, so a
shutdown has no in-flight request to cancel. Every caller of `serve` and
`shutdown` receives the same `HTTPShutdownReport{accepted, overloaded,
completed, cancelled}`; a second `serve` is an `HTTPError.IO`. The connection
deadline bounds each connection's reads and writes (30 seconds by default).

Static files are read through `core.files` and the `Date` field comes from the
`core.time` wall clock, so serving carries `[Net, FS.Read, Time]`:
`serve(addr, …)`, `HTTPServer.serve`/`wait`, `serve_once` and
`serve_once_listener` have that row. `static_file`, `static_file_range`,
`HTTPHandler.handle`, and the mux installers `static_files`, `cors` and
`request_id` are `[Net, FS.Read]`, and a handler closure may use `Net` and
`FS.Read`.

| API | Result | Description |
| --- | --- | --- |
| `mux()` | `HTTPMux` | Create a router carrier. |
| `bind(addr, mux, tls, deadline)` | `HTTPServer !HTTPError` | Bind a plaintext listener. |
| `serve(addr, mux, tls, deadline)` | `!HTTPError` | Bind, then serve requests until the listener fails. |
| `serve_once(addr, mux)` / `serve_once_listener(listener, mux)` | `!HTTPError` | Serve one request for tests and small adapters. |
| `response(status, body)` | `HTTPResponse` | Construct a text response. |
| `json(status, body)` | `HTTPResponse` | Encode an `Encode` value as JSON response data. |
| `static_file(path, content_type)` / `static_file_range(request, path, content_type)` | `HTTPResponse !HTTPError` | Serve a file, with a range-aware form for a request. |
| `static_files(mux, prefix, root, index:, dotfiles:, follow_links:)` | unit | Mount a directory below a route prefix; each optional `Bool` label overrides one default policy. |
| `tls(cert, key)` | `HTTPServerTLS` | Build explicit server TLS material. |
| `sse(body)` | `HTTPResponse` | Build a server-sent-events response. |
| `cors_policy(origins)` / `cors(mux, policy)` | policy/unit | Validate a CORS origin policy and install it. |
| `access_log(request, status)` | `String` | Render a stable access-log line. |
| `request_id(mux)` | unit | Install request-id middleware. |

Routes are `/`, static segments, `:name` parameters, and one final `*name`
catch-all that may match zero segments. An invalid pattern panics with E2805
and a second route of the same method and shape panics with E2804. Request
paths are percent-decoded per segment without `+` rewriting; an encoded `/`, a
`.` or `..` segment, or invalid UTF-8 is `400`. A static segment outranks a
parameter, which outranks a catch-all, segment by segment; then the shorter
pattern wins, then the earlier registration. `HEAD` falls back to `GET`. A path
served only for other methods is `405` with `Allow`, an unknown path `404`, and
`OPTIONS *` is `204` with every registered method in `Allow`. Middleware
registered first runs outermost. Every request carries `x-request-id`: a valid
incoming value (1–128 visible ASCII bytes) is kept, otherwise one is generated.

Request framing refuses what HTTP/1.1 forbids: a head over 32 KiB or more than
100 fields is `431`; a malformed request line, an obsolete folded field, a
missing or repeated `Host` on HTTP/1.1, `Content-Length` with
`Transfer-Encoding`, a coding other than `chunked`, and conflicting lengths are
`400`; a body over 1 MiB is `413`; any `Content-Encoding` is `415`; an
`Expect` other than `100-continue` is `417`; an unsupported `HTTP/` version is
`505`; and a request not completed within the deadline is `408`. Chunked bodies
accept only the trailers the request declared. Responses carry `Date` and the
server's own framing: user `Content-Length`, `Transfer-Encoding`, `Trailer`
and `Connection` fields are not sent. Response trailers switch the body to
chunked framing with a `Trailer` field; `1xx`, `204`, `205` and `304` carry no
body. A handler error of `BodyTooLarge`, `InvalidFraming` or
`UnsupportedEncoding` answers `413`, `400` or `415`; any other is `500`.

`static_files` serves `GET` below the prefix. It refuses dot-files unless
`dotfiles: true`, symbolic links unless `follow_links: true`, and any path that
resolves outside the root. A directory serves its `index.html` unless
`index: false`. Responses carry a content type from the file extension,
`accept-ranges`, an `etag` and `last-modified`; `If-None-Match` and
`If-Modified-Since` answer `304`, one byte range answers `206` (guarded by
`If-Range`), an unsatisfiable range `416`, and anything unservable `404`.

The Jet CORS constructor accepts named-origin lists or `.Any`/`.List` policies.
Empty method/header lists retain the published defaults, credentials default to
false, and max age defaults to 86400 seconds. Explicit max ages are preserved,
including negative values. Wildcard origins with credentials are refused before
the policy reaches transport middleware.

`access_log` uses a stored route template when one is present (including an
empty template); otherwise it strips the query from the request path.
Method, selected path, and the supplied status are rendered as three
space-separated fields without status validation.

`response` and `json` preserve statuses from 100 through 599. Outside that
range they select status 500 and replace the body with
`500 Internal Server Error`. Response metadata defaults to HTTP/1.1, empty
remote address, redirect history and timings, no raw content encoding, and a
non-reused connection. JSON adds `application/json; charset=utf-8`; `sse`
wraps its text as `data: {body}\n\n`, adds `text/event-stream` and disables
caching with `cache-control: no-cache`. The selected body is transferred into
the transport's one-shot body resource without eager consumption.
Typed response JSON decoding enforces the supplied body-byte cap before
decoding, returning `HTTPError.BodyTooLarge` when the body exceeds that cap.

### `core.net.ws` — WebSocket transport

`core.net.ws` is the WebSocket home and accepts an HTTP request for server-side
upgrade. The client entry point accepts `ws://` only; it rejects credentials,
fragments, missing hosts, and other schemes before the provider handshake.

| API | Result | Description |
| --- | --- | --- |
| `connect(url) -[Net, Time.Wait]>` | `WsConn !WsError` | Validate and dial a cleartext WebSocket URL. |
| `upgrade(request) -[Net, Time.Wait]>` | `WsConn !WsError` | Validate GET, Upgrade, Connection, version 13, and key headers before upgrading. |
| `conn.send_text(text)` / `conn.send_bytes(bytes)` | `!WsError` | Send a text or binary frame. |
| `conn.recv()` | `WsMessage !WsError` | Receive a text, binary, or close message. |
| `conn.close(code, reason)` | `!WsError` | Check the `Int` code at the host-integer boundary, send a close frame, and shut down the connection. |

`WsError` distinguishes invalid URLs, invalid handshakes, protocol/size
failures, timeout, cancellation, closed connections, and unsupported targets.

### `core.web.browser` — WebDriver BiDi automation

`core.web.browser` is the portable browser automation home. It speaks
versioned WebDriver BiDi over `core.net.ws`, with a capability-checked CDP
expert path. Profiles are isolated by default, and receipts and traces keep
only redacted audit facts.
Profile construction is Jet-owned and accepts exactly `bidi-2025.5` (version
`2025.5`) or `bidi-2024.11` (version `2024.11`). It does not trim, case-fold,
or substitute an unknown profile name.
`connect_profile(endpoint, profile, timeout)` sequences the handshake in Jet:
raw WebSocket connection and timeout setup, `session.status` ready/message
validation, then `session.new` with ordered empty `alwaysMatch` capabilities.
The session-started fact is recorded before validating the session ID and
capabilities. CDP requires the first `goog:cdp` field to be exactly true.
The whole runtime is Jet source in `Core/Web/Browser.jet`: commands, event
capture, trace bounds, locators, reports, and the test server run over
`core.net.ws`, `core.process`, and `core.files`. Wire operations carry `Net`
and `Time`, because each command measures its own deadline. A `Browser` and its
contexts, pages, frames, locators, intercepts, and protocol handles stay on the
thread that opened them (**E1102**).

| API | Result | Description |
| --- | --- | --- |
| `config()` / `config_from_env()` | `BrowserTestConfig !BrowserError` | Load checked browser-test configuration. |
| `make_profile(name)` / `profile(name)` / `timeout(ms)` | `BrowserProfile` / `BrowserProfile !BrowserError` / `BrowserTimeout !BrowserError` | Construct an isolated profile, check a pinned BiDi profile (`bidi-2024.11` or `bidi-2025.5`), or check a timeout in `1..=600000` milliseconds. |
| `locked(engine)` | `BrowserLocked !BrowserError` | Read and verify the provider's locked browser record. |
| `launch(engine)` | `Browser !BrowserError` | Start the locked engine headless on a throwaway profile and connect over BiDi; closing the session stops the engine. Firefox serves BiDi itself; Chromium launches through the chromedriver its lock entry pins, with `protocol("cdp")` available. |
| `connect(endpoint)` / `connect_profile(endpoint, profile, timeout)` / `connect_browser_profile(profile, timeout)` | `Browser !BrowserError` | Connect to an endpoint with the default or a named profile. |
| `browser.context()` | `BrowserContext !BrowserError` | Open an isolated browsing context. |
| `context.page()` / `context.tab()` | `BrowserPage` | Open a page or tab in a context. |
| `page.goto(url)` | `BrowserPage !BrowserError` | Navigate a page under its timeout and protocol policy. |
| `page.main_frame()` / `page.frames()` | `BrowserFrame` / `[BrowserFrame]` | Inspect the main frame or child frames. |
| `page.close()` / `context.close()` / `browser.close()` | `!BrowserError` | Explicitly close page, context, or browser resources. |
| `page.get_by_role(role, name)` / `get_by_text(text)` | `BrowserLocator` | Locate an element through semantic role or visible text. |
| `page.get_by_label(label)` / `get_by_placeholder(text)` / `get_by_test_id(id)` | `BrowserLocator` | Locate labeled, placeholder, or test-id controls. |
| `page.get_by_css(selector)` | `BrowserLocator` | Use an explicit CSS locator when semantic selection is not suitable. |
| `locator.wait(timeout)` / `locator.wait_gone(timeout)` | `!BrowserError` | Wait for a locator to appear or disappear. |
| `locator.click()` / `locator.hover()` | `!BrowserError` | Perform checked pointer actions. |
| `locator.fill(text)` / `locator.press(key)` | `!BrowserError` | Fill a control or send a key. |
| `browser.subscribe(kind)` / `browser.next_event(timeout)` | `!BrowserError` / `BrowserEvent !BrowserError` | Subscribe to and receive bounded, redacted browser events. |
| `browser.add_intercept(pattern)` / `add_intercept_url(pattern, url)` | `BrowserIntercept` | Install a network interception rule. |
| `browser.continue_request(id)` / `fail_request(id)` / `fulfill_request(id, status, body)` | `!BrowserError` | Continue, fail, or fulfill an intercepted request. |
| `page.set_cookie(name, value, domain)` / `page.cookie(name)` | `!BrowserError` / `String? !BrowserError` | Set or read a page-partition cookie. |
| `page.clear_cookies()` | `!BrowserError` | Clear cookies in the page partition. |
| `page.storage_get(kind, key)` / `storage_set(kind, key, value)` | `String` / `!BrowserError` | Read or write local or session storage. |
| `page.storage_clear(kind)` | `!BrowserError` | Clear a local or session storage partition. |
| `locator.set_files(path)` | `!BrowserError` | Set files on a file-upload control. |
| `page.screenshot()` / `page.pdf()` | `[U8] !BrowserError` | Capture a bounded screenshot or PDF artifact. |
| `browser.protocol("bidi" \| "cdp")` / `protocol.send(method, params)` | `BrowserProtocol` / response | Access the selected protocol; CDP requires the advertised capability. |
| `browser.privacy()` | `BrowserPrivacy` | Report isolated-profile, receipt-redaction, and shared-profile policy facts. |
| `browser.receipt()` / `browser.trace()` | `BrowserReceipt` / `BrowserTrace` | Return redacted audit facts and the bounded action trace. |
| `begin_named(config, test, engine, attempt, path, line, column)` / `fixture_context(fixture)` / `fixture_page(fixture)` | `BrowserTestFixture !BrowserError` / `BrowserContext` / `BrowserPage` | Open a named test fixture (locked engine, isolated context, page, event subscriptions) and read its context and page. |
| `fixture_source(name, body)` / `generate_source(name, body, base_url)` | `String !BrowserError` | Generate bounded fixture source with escaped base-URL data. |
| `selected(config, name)` | `Bool` | Apply the configured test filter to a test name. |
| `report_new(config)` / `report_add_case(report, case)` | `BrowserTestReport` | Build an ordered browser test report. |
| `report_text(report)` / `report_json(report)` / `report_html(report)` | `String` | Render the report in text, JSON, or the self-contained HTML viewer. |
| `report_exit_code(report)` / `write_report(report, path)` | `Int` / `!BrowserError` | Produce the test exit code or write the configured report (plus the HTML viewer for `ui` runs). |
| `server_start(config)` / `server_stop(server)` / `server_url(server)` / `server_logs(server)` | `BrowserTestServer !BrowserError` / – / `String` / `String` | Start the configured test server and wait until it accepts connections; stop it and keep its bounded logs. |
| `watch_changed(path, last_modified_ms)` | `Bool !BrowserError` | Report whether the watched path changed since the given modification time. |

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
use crypto: core.crypto

run: fn() {
    recipient: crypto.X25519SecretKey.new_random() ?? return
    msg: "hello, jet".bytes()
    box: crypto.seal([recipient.public_key()], msg, []) ?? return
    plain: crypto.open(recipient, box, []) ?? return
    print(plain)
}
```

| API | Result | Description |
| --- | --- | --- |
| `sha256(input)` / `sha512(bytes)` / `blake3(bytes)` | `Digest256` / `Digest512` | Produce typed digests; use `.hex()` or `.as_bytes()`. `sha256` takes `[U8] \| String`; a `String` hashes its UTF-8 bytes. |
| `sha1(bytes)` / `sha224(bytes)` / `sha384(bytes)` | `String` | Produce legacy SHA hex digests; SHA-1 remains for UUID v5 and legacy checks. |
| `sha3_224` / `sha3_256` / `sha3_384` / `sha3_512` | `String` | Produce SHA-3 hex digests. |
| `hmac_sha256(key, data)` | `[U8]` | Produce HMAC-SHA256 bytes. |
| `hkdf_sha256(ikm, salt, info, len)` | `Secret !CryptoError` | Derive a bounded 0–8160-byte nominal secret. |
| `pbkdf2_hmac(password, salt, iterations, key_len)` | `[U8] !CryptoError` | Derive a bounded PBKDF2-HMAC-SHA256 byte sequence. |
| `new()` / `update(hasher, bytes)` / `digest(hasher)` | hasher operations | Use the streaming SHA-256 hasher: `update` absorbs each chunk as it arrives and keeps at most one partial 64-byte block, so a large input is never held whole; `digest` returns lowercase hex and leaves the hasher able to absorb more. |
| `constant_time_equal(a, b)` / `constant_time_equal_bytes(a, b)` | `Bool` | Compare nominal secrets or byte lists without early equality branching. |
| `wipe(bytes)` | unit | Zero every element of a `[U8]` and its spare capacity with stores the optimizer cannot drop; the length is unchanged. |
| `generatekey()` | `X25519SecretKey !CryptoError` | Generate an X25519 secret through the random provider. |
| `x25519(secret, public)` / `x25519_shared(secret, public)` | `SharedSecret !CryptoError` | Perform typed X25519 agreement and reject invalid keys. |
| `x25519_public(secret)` | `X25519PublicKey !CryptoError` | Derive the public key. |
| `seal(recipients, bytes, aad)` / `open(identity, box, aad)` | `Sealed !CryptoError` / `[U8] !CryptoError` | Use the recipient envelope with internal key and nonce handling. |
| `file_seal(recipients, source, destination)` / `file_open(identity, source, destination)` | `FileCryptoError!` (unit success) | Stream authenticated JETC v2 files in bounded 1 MiB chunks, with atomic no-overwrite publication; success has no Boolean payload. |
| `sign(signing_key, bytes)` / `verify(verify_key, bytes, signature)` | `Signature !CryptoError` / `Bool !CryptoError` | Use nominal Ed25519 signing and verification. |
| `wrap(secret, recipient)` / `unwrap(identity, wrapped)` | `WrappedKey !CryptoError` / `Secret !CryptoError` | Wrap a nominal Secret for an X25519 recipient. |
| `password_hash(password)` / `password_verify(password, stored)` | `PasswordHash !CryptoError` / `Bool !CryptoError` | Hash and verify a nominal Secret with the Core password policy. |

`Sealed`, `Signature`, `Digest256`, and `Digest512` expose byte projection
methods where the declared API permits it; secret-bearing types must not be
logged. `file_seal` and `file_open` take typed `Path` values at their checked
boundary and use `FS` plus `Rand` where indicated.
`Secret` owns its configuration `Decode` implementation in Core source:
only a text data-tree value is accepted, and its UTF-8 bytes become the secret.
There is no corresponding `Encode` implementation. A user-defined type named
`Secret` does not inherit this codec by name.

`wipe(bytes)` erases key bytes that must stay a plain `[U8]`, such as a seed
handed to a C library. A buffer the List alone owns is zeroed in place,
including its spare capacity. A buffer another List still shares keeps its
bytes for that List: the wiped List takes a fresh zeroed buffer of the same
capacity, and the shared buffer is marked to be zeroed when its last owner
releases it. A copy a writer makes of a marked buffer, before a write or when
it grows, is marked too. Frozen literal bytes cannot be written, so a List
over them takes fresh zeroes of its length. Buffers a List gave up while
growing before the wipe are not reached; reserve the capacity first.

The native `Secret` carrier owns the canonical List header `[len, cap, data]`
(three words, 24 bytes); `U8` elements use the canonical host U8 stride. Creating
a Secret adopts the List buffer without copying secret material. Borrowing
`bytes` reads or writes the actual parent's header, and permits only fixed-length
indexed mutation, not growth, removal, clearing, or whole-List assignment.
Close effectfully wipes cap × the host U8 stride bytes, including any slot
padding and spare capacity, then releases that same byte count at alignment
eight and clears the header.
Repeated Close is harmless; final owner drop alone releases the header.

Key wrapping authenticates the `JETW` header as AAD prefixed with the
`JETW1 wrap` domain label. `wrap` and `unwrap` use the same label and header
bytes, so the recipient can authenticate and recover the original Secret.

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
[`crypto_suite.jet`](../../../Examples/Crypto/CryptoSuite.jet) and
[`crypto_envelope.jet`](../../../Examples/Crypto/CryptoEnvelope.jet).

The expert secret-byte projections return owned copies, not transfers out of
the original zeroizing owner. The private JETC v2 engine uses those projections
only inside reasoned `#Unsafe` blocks and wipes each operation-scoped copy on
both success and refusal before propagating the outcome. The original
`Secret`, `SharedSecret`, and `X25519SecretKey` retain their ordinary `Close`
duty; no raw secret-field access is required outside the expert boundary.

Fixed-limb operations reserve their complete workspace before filling it,
avoiding geometric regrowth that could leave stale secret bytes in freed
allocations. Workspace reads are computed before lending the workspace for a
store, and AES aliasing temporaries explicitly copy scalar lanes before
mutating a bank.

AES-256-GCM uses a 32-byte key, a 12-byte nonce, and a 16-byte appended tag.
Its source substitution uses fixed bitplane operations rather than
secret-indexed lookup tables; GHASH uses 128 masked fixed-width steps.
Open authenticates all 16 tag bytes before producing plaintext and returns
`CryptoError.OpenFailed` for a bad tag. These source-level properties do not
by themselves establish a machine-code timing guarantee for a compiler target.

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
carries the `Secret` effect. A `KeyRef<T>` contains its public name/version and
the private repository identity it was issued under (store, record and record
hash), not key bytes; every operation on it re-verifies that identity, so a
same-named generation in a restored or re-imported repository never answers
for it. The provider owns persistence and key-material lifetimes.
Its `Display` is `repo:{name}@v{version}` for every key type `T`; rendering never
reads key material or private provider metadata and imposes no `Display` bound
on `T`.

```jet
use {
    crypto: core.crypto
    vault: core.crypto.vault
}

provision: fn() -[Secret, IO]> {
    plan: vault.prepare_generate<crypto.SigningKey>("release") ?? return
    write: vault.authorize_write(plan, "create release signer") ?? return
    key_ref: vault.commit_generate(write, plan) ?? return
    print("created {key_ref}")
}
```

| API | Result | Description |
| --- | --- | --- |
| `get(name)` | `?String` | Read the separate untyped repository-secret value. |
| `current<T>(name)` | `?KeyRef<T> !VaultError` | Return the active generation, if one exists. |
| `versions<T>(name)` | `[KeyRef<T>] !VaultError` | Return generations in provider order. |
| `load<T>(key)` / `status<T>(key)` | `T !VaultError` / `KeyStatus !VaultError` | Load an exact Active or Retired key, or inspect `Active`, `Retired`, or `Revoked`. |
| `prepare_generate<T>(name)` / `prepare_store<T>(name, key)` | `MutationPlan<T> !VaultError` | Prepare a typed mutation without publishing it. |
| `prepare_import_signing` / `prepare_import_x25519` | typed plan `!VaultError` | Prepare a checked 32-byte signing or X25519 secret import. |
| `prepare_rotate<T>(name)` | `MutationPlan<T> !VaultError` | Prepare a rotation for a named generation. |
| `prepare_retire<T>(key, reason)` / `prepare_revoke<T>(key, reason)` | `MutationPlan<T> !VaultError` | Bind a retirement or revocation to an exact key and reason. |
| `authorize_write<T>(plan, reason)` | `VaultWrite<T> !VaultError` | Request one provider-authorized write for the exact plan. |
| `commit_generate/store/rotate/retire/revoke<T>(write, plan)` | typed result `!VaultError` | Consume the write and plan in an atomic mutation. |
| `export_to_recipients<T>(key, recipients)` | `WrappedVaultKey !KeyWrapError` | Export a typed key for one to sixteen distinct X25519 recipients. |
| `export_to_passphrase<T>(key, passphrase)` | `WrappedVaultKey !KeyWrapError` | Export a typed key under a bounded nominal Secret. |
| `prepare_import_wrapped<T>` / `authorize_wrapped_import<T>` / `commit_import_wrapped<T>` | typed results | Bind, authorize, and commit a wrapped-key import. |

`VaultError` is closed: `InvalidName`, `NotFound`, `WrongType`, `Revoked`,
`Locked`, `AuthorityDenied`, `Conflict`, `UnsupportedProvider`,
`InvalidEncoding`, `DurabilityUnknown`, `Crypto(CryptoError)`,
`IO{operation: String, redacted_path: String}`, and `Internal{incident_id: String}`.
The `Crypto` payload is a matchable `CryptoError` case (`OpenFailed`,
`VerifyFailed`, `KeyRejected`, `Length`, or `Unavailable`), never backend text.
Invalid names or reasons return `InvalidName` before provider access; raw key
imports that are not exactly 32 bytes return `InvalidEncoding`. Missing or wrong
identity and age-open failures return `Locked`. Error payloads never contain
key or store bytes, identity material, raw OS prose, or foreign paths.

Mutation plans, writes, and wrapped-import plans are one-use typed carriers.
Provider authorization is required for every write; source, workspace settings,
environment variables, DAP, and stdin do not silently authorize it. The
persistent format authenticates type and origin, and failures redact paths,
identities, recipients, backend text, and key bytes. Revocation is local
bearer-copy state: an already exported envelope cannot be remotely erased.

Wrapped backups are parsed in Jet before a `WrappedVaultKey` is constructed:
JVKW/v1 length, version, mode, type, origin identity, sorted unique recipients,
and fixed Argon2id parameters are checked without examining plaintext key bytes.
`.bytes()` deliberately returns an owned copy of the encrypted backup.
`KeyUnlock.Recipient` and `.Passphrase` retain read views of the original secret;
the owner must remain live through wrapped import. No copied-secret carrier is
created. Name and authorization-reason limits count UTF-8 bytes, and reject
Unicode controls; reasons must also be NFC and exclude U+2028/U+2029.

`ExpiringSecret<T>` is the lifetime wrapper for `Secret`, `SigningKey`, and
`X25519SecretKey`. Construct it with an injected `Clock` and a `Duration`; use
`.with` for a non-escaping read loan. Expiry returns `Expired`, and the wrapper
destroys the owned credential through its zeroizing drop path. Expiry wipes the
resident byte storage with the same volatile kernel used by explicit secret
close, without a copied buffer or a replacement allocation. A system clock adds
the `Time` effect. See
[`vault_keys.jet`](../../../Examples/Crypto/VaultKeys.jet) and
[`expiring_secret.jet`](../../../Examples/Memory/ExpiringSecret.jet).

The wrapper stores its optional credential and original clock observer in Jet.
It samples that observer under the storage guard, and expires only when
`now > deadline`; at equality the credential remains available. Expiry takes
the payload out of its slot and ends the owned payload's scope, so its
`Close` follows the same zeroization path as dropping still-live storage.
The sole wipe host operation is the plain-byte `crypto.wipe` kernel.
No observer or loan callback crosses that host boundary.
Fields are private; `expires_at_ms()` reads the deadline and `.with(...)`
is the fallible credential accessor. This is Main's owner-approved, veto-able
private-field correction following Measurement/LogSpan, not a plaintext
`secret()` accessor that bypasses expiry. Debug/Printable are opted out.

### `core.auth` — token verification and sessions

`core.auth` verifies the closed JWT and PASETO forms and provides password,
OAuth, magic-link, and cookie-session helpers. It is Jet source; the host keeps
only the D-AUTH1=A byte cell that holds the session store.

| API | Result | Description |
| --- | --- | --- |
| `verify_jwt(token, key, audience, issuer, clock_skew)` | `Claims !AuthError` | Verify a three-part HS256 JWT with exact integer `exp`, optional `nbf`/`iat`, and audience/issuer checks. Omitting `issuer` skips the issuer check; a given issuer must equal `iss`. |
| `verify_paseto(token, key, audience, issuer, clock_skew, footer, implicit)` | `Claims !AuthError` | Verify `v4.public` with a 32-byte Ed25519 public key; issuer, clock skew, footer, and implicit assertion are optional labeled arguments. |
| `register_user(user_id, password_hash)` | `Unit !Err` | Register an identity with an already-computed password hash (`DB`). |
| `password_login(user_id, password_hash, now_ms, ttl_ms)` | `Session !Err` | Validate credentials and issue a bounded session lifetime (`DB`, `Rand`). |
| `oauth_begin(provider)` / `oauth_finish(state, assertion, now_ms, ttl_ms)` | state/session `!Err` | Start and finish the provider-backed OAuth assertion flow (`DB`, `Env`, `Rand`; `oauth_begin` also `Time`). |
| `magic_link_issue(user_id, now_ms, ttl_ms)` / `magic_link_consume(token, now_ms, ttl_ms)` | token/session `!Err` | Issue and consume a token with a bounded lifetime (`DB`, `Rand`). |
| `session_validate(session_id, now_ms)` | `Session !Err` | Validate the session identifier and return its session carrier (`DB`). |
| `session_user(session)` / `session_cookie(session)` / `session_id(session)` | `String` | Project the validated session fields. |
| `session_show(session)` | `String` | Render a redacted session summary; cookie bytes are not printed. |

JWT accepts only HS256 and requires a key of at least 32 bytes. PASETO accepts
only `v4.public`, a 32-byte key, a non-empty normalized audience, and canonical
base64url segments. Unknown algorithms, malformed JSON, wrong signatures,
expired or not-yet-valid claims, wrong audiences, and invalid inputs fail
closed. `Claims` contains `subject`, `audience`, `issuer`, `expires_at`,
`not_before`, and `issued_at`; subject, issuer, not-before, and issued-at are
optional. `AuthError` retains `MalformedToken`, `UnsupportedToken`,
`InvalidSignature`, `WeakKey`, `MissingClaim`, `WrongAudience`, `WrongIssuer`,
`TokenExpired`, `DecodeError`, and `TokenNotYetValid`, including their payloads.

Session cookies use the fixed `jet_session=` form with `HttpOnly`, `Secure`,
`SameSite=Lax`, and `Path=/` attributes. Session helpers take explicit
millisecond timestamps and lifetimes. See
[`auth_tokens.jet`](../../../Examples/Crypto/AuthSessions.jet) and
[`auth_sessions.jet`](../../../Examples/Crypto/AuthSessions.jet).

### `core.sync` — replica-aware CRDT values

`core.sync` ships the D-SYNC1 value types: a sequence-CRDT `SyncText`, a
PN-counter `SyncCounter`, a last-writer-wins `SyncMap`, and an add-wins
`SyncList`, plus a closed allow/deny action `RowPolicy`. Every write names the
replica that made it, so replicas that edit apart merge to one value in either
order and neither loses its work. Merges are commutative, associative, and
idempotent. Carrier fields are public for serialization. Database row
policies are not here; `core.db` owns `policy`, `policy_allows`, and
`policy_show` (D-DBPOLICY-CHECK1).

| API | Result | Description |
| --- | --- | --- |
| `text_new(replica, text)` | `SyncText` | Create a document whose characters `replica` wrote. |
| `text_edit(doc, replica, index, delete_count, insert)` | `SyncText` | Delete `delete_count` characters at Unicode scalar `index`, then insert `insert` there; the insert anchors to the character before the range. |
| `text_set(doc, replica, text)` | `SyncText` | Tombstone every visible character and write `text` at the start. |
| `text_merge(left, right)` | `SyncText` | Union both character sets; a deletion seen by either side stays. |
| `text_show(doc)` / `text_metadata(doc)` | `String` | Render `SyncText(<text>)`, or the `LamportClock(r1=12,r2=12)` of the highest counter each replica wrote. |
| `counter_new(replica, value)` / `counter_inc(counter, replica, delta)` | `SyncCounter` | Start or add a signed delta under one replica. |
| `counter_merge(left, right)` / `counter_value(counter)` | `SyncCounter` / `Int` | Keep each replica's larger positive and negative totals; read their sum. |
| `map_new()` / `map_set(map, key, value)` / `map_delete(map, key)` | `SyncMap` | Write a value or a tombstone at the next map clock. |
| `map_get(map, key)` / `map_merge(left, right)` / `map_show(map)` | `String?` / `SyncMap` / `String` | Read a live entry; per key keep the larger clock, then the larger writer, then a same-writer deletion, then the larger value; render `SyncMap(k=v,...)`. |
| `list_new()` / `list_push(list, replica, value)` | `SyncList` | Add a `(replica, value)` item once. |
| `list_merge(left, right)` / `list_show(list)` | `SyncList` / `String` | Union items in replica-then-value order; render `SyncList(r1:a\|r2:b)`. |
| `policy_new()` / `policy_grant` / `policy_deny` / `policy_revoke` | `RowPolicy` | Build a closed allow/deny action policy. |
| `policy_allows(policy, action)` / `policy_show(policy)` | `Bool` / `String` | Apply deny-first wildcard rules or render counts. |

Replica names, keys, and writers must be non-blank text without control
characters; text and values are bounded to one MiB, documents to 4 Mi
characters, counters to 4096 replicas, and maps and lists to 100,000 entries.
A write that breaks a bound or would overflow a clock or count leaves an
*invalid* carrier rather than truncating: it shows as
`SyncError(invalid SyncText)` (and likewise for the other types), reads as
empty, and every merge with it stays invalid. Merging one identity with two
readings (a replica name reused by two writers) is invalid too. Shown keys,
values, and replica names escape `\`, `,`, `=`, `|`, `:`, and `/`.

A mutation on an invalid carrier, or one with a malformed replica, key, index,
or size, returns the carrier unchanged. `app.sync(doc, over: session)` carries
a shown document over the live-query channel and returns the session's receipt
`SyncOver(session=…, generation=…, doc=…)`. A session keeps its merged
document in the process-wide live registry; a publication that changes it
advances the generation, and a duplicate delivery returns the same receipt.
Map keys with equal values, list members, and counter components merge; a
conflicting map value, a differing `SyncText`, or a mixed pair answers
`SyncError(document merge denied)`. A blank or over-long session is
`SyncError(invalid session)`, and a display that is not canonical (sorted,
escaped, no duplicates) is `SyncError(document is not a canonical CRDT
value)`. This module is not an
authenticated remote transport or
a general thread-lock API; use the typed transport and task modules for those
contracts. The thread-lock ledger names (`broadcast`, `clear`, `lock`, `rlock`,
`signal`, `trylock`, `unlock`, `wait`, `thread`, `timer`, and `locked`) remain
declined under D-CORESURF-SMALL1.

### `core.watcher` — file, process, and port watches

`core.watcher` creates explicit watch handles for files, process IDs, and TCP
ports (D-WATCH-SCOPE1). Polling is explicit: a handle or set does not own a
background thread or shell process. Each handle owns one synchronous
`Event<WatchEvent>`: `handle.on(scope, handler)` and `handle.once(scope,
handler)` register on it, and every `poll` emits each event it returns to it.
Handles and sets are shared: copies observe the same watch state, and
`set.add(handle)` keeps the handle in every copy of the set.

Watch constructors retain provider-owned resource state, not string-backed
handle records. `files(path)` passes the path to the provider unchanged; its
filesystem error is not replaced by a source-only empty-path error.

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

| Method | Result | Description |
| --- | --- | --- |
| `handle.poll()` / `handle.events()` | `[WatchEvent]` | Diff the target since the last poll and emit each event to the handle's event. A cancelled handle returns none. |
| `handle.on(scope, handler)` / `handle.once(scope, handler)` | `Subscription` | Register a `fn(WatchEvent)` handler for the scope's lifetime. |
| `handle.cancel()` / `handle.is_active()` | unit/bool | Stop the handle, or ask whether it still polls. |
| `handle.summary()` | `String` | `watch file <root>`, `watch process <pid>`, or `watch port <host>:<port>`. |
| `set.add(handle)` | unit | Add a handle to the shared set. |
| `set.poll()` / `set.events()` | `[WatchEvent]` | Poll every handle in insertion order. |
| `set.summary()` | `String` | `watchset handles=<n>`. |

A file poll reports `Created` and `Modified` paths in path order, then
`Removed` paths in path order, with detail `dir` or `file`; a failed snapshot
is one `Error` event for the root. A process poll reports `Ready` when the
process is first seen alive and `Exited` when it stops or was never alive. A
port poll reports `Ready` (detail `host:port`) once a connection succeeds
after the port was not ready.

`WatchDomain` is `File`, `Process`, or `Port`. `WatchEventKind` is `Created`,
`Modified`, `Removed`, `Error`, `Exited`, or `Ready`. `WatchEvent` carries
`domain`, `kind`, `path`, `detail`, `pid`, and `port`. Match all variants when
handling a closed event enum. See
[`watcher.jet`](../../../Examples/Systems/Files/Watcher.jet).

The checked `WatchEvent.kind` field has type `WatchKind`, with the same six
variant names; it is distinct from the independently exported `WatchEventKind`.
Event equality includes all six stored fields. Show prints domain, kind, path,
and detail; Debug additionally prints pid and port and quotes the text fields.

### `core.term` — terminal input and output

`core.term` owns UTF-8 stdio, prompts, raw-key input, terminal size, and ANSI
style. Terminal stream operations follow the [bounded buffering law](../spec.md#bounded-buffering-law).
`term.print` and `term.eprint` are variadic like the prelude `print`: each
argument writes on its own line to stdout or stderr (D-VERDICT-1321-1).
`eprint` requires `IO` and `Time.Wait`; `binread` and `binwrite` additionally
require `FS` because they access files rather than terminal streams.

```jet
use term: core.term

run: fn() {
    name: term.readline("your name? ") ?? return
    print("hi, {name}")
    term.eprint("(log) done")
    out := term.stdout()
    out.write("done") ?? return
    out.flush() ?? return
}
```

| API | Result | Description |
| --- | --- | --- |
| `readline(prompt)` | `String !IOError` | Write the prompt (default empty) and flush stdout, then read one line without its newline; a closed stream reads as `""`. |
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
| `progress(label)` | `!IOError` | Write a progress update through the terminal stream: a carriage-return frame on a terminal stderr, otherwise the label as a plain stdout line. |

`style` is a no-op when output is not a TTY unless `style_force` is used.
`Key` carries `Char(Char)`, `F(Int)`, and `Ctrl(Char)` payloads. Its unit variants
are `Enter`, `Escape`, `Backspace`, `Tab`, `Delete`, `Insert`, the four arrow
keys, `Home`, `End`, `PageUp`, `PageDown`, the modifier keys `Shift`, `Control`,
`Alt`, `Super` and `CapsLock`, and `Unknown`. `read_key` reports a control chord
as `Ctrl(c)` and never reports `Insert`, `Home`, `End`, `PageUp`, `PageDown` or
a modifier key; those come from a window's `UIEvent.KeyDown`/`KeyUp` (core.ui).
`confirm` treats a bare Enter as no; `choose` returns an input error
after its finite retry budget or when stdin closes.

Read a buffered handle with `reader.read_line()`: each call mutably lends
the reader and returns `String? !IOError`. Ordinary failure propagation
preserves the optional line, so EOF remains `None` rather than an I/O error.

The ambient `input` is every line of the program's input as a lazy
`Iter<String>`: the lines of each file the program's arguments
name, in order, or of standard input when it has no arguments, like awk's
file operands and Perl's `<>`. Lines lose their line ending, one file is open
at a time, and nothing is read until the walk pulls a line. A file that can't
be opened or read stops the program with its `IOError`, as a read error of any
line source does. Reading `input` needs `IO` and `Exec.Args`; the files the
arguments name are granted by being typed on the command line, so `input`
needs no `FS.Read` (D-SCRIPT-DESIGNATE1=A). Use `readline(prompt)` to ask for
one line.

```jet
run: fn() {
    loop line in input {
        if line.contains("error") -> print(line)
    }
}
```

Three methods read the same input whole. `input.csv<Row>()` reads it as one
CSV table, header first, and decodes each record into `Row`, so a field `Row`
lacks is a compile error before any input is read; `input.json<Row>()` decodes
each JSON value into `Row`, one per line (JSON Lines) or spread across lines.
Both return `[Row]` and fail with one `Err` that lists every record that did
not decode, under its CSV row or the line its JSON value starts on. They are
`core.encoding.csv` and `core.encoding.json` decoding, so they need nothing
more than `input` does. `input.edit(change)` rewrites each file the arguments
name in place, every line through `change`, keeping each line's ending (LF or
CRLF) and a missing final line break; a file `change` leaves as it was is not
written. With no arguments it prints each standard input line changed. It
needs `FS.Read` and `FS.Write`, which the run's effect notice names before the
program starts. `jet help input` lists every name a program reads input
through.

```jet
Sale: struct {
    item: String
    amount: Float
}

run: fn() {
    print(input.csv<Sale>().map((s) -> s.amount).sum())
}
```

The executable terminal examples cover binary-file composition, UTF-8
stderr, CRLF line reads and EOF, plus confirmation with an empty-answer default:
[`TerminalSource.jet`](../../../tests/run/CoreIO/TerminalSource.jet) and
[`TerminalParity.jet`](../../../Examples/Systems/Terminal/TerminalParity.jet).
Their programs and transcripts are shared by the golden and native harnesses;
they do not require a bespoke Rust test or direct `rustc` invocation.

### `core.args` — declarative command-line parsing (D-ARGS1)

`core.args` provides an `ArgsSpec` builder, explicit `decode_argv` tree parsing,
and typed `decode<T>()` / `T.merge` using the target's checked `#CLI` shape.
`ArgsSpec` is the checked builder handle, not a record containing only flag
names. The builder consumes a spec and returns a new value at each method call;
parsing is separate from process execution so tests can pass an explicit
argument vector.

```jet
use {
    args: core.args
    process: core.process
}

run: fn() {
    spec: args.spec()
        .flag("verbose", "print extra detail")
        .option("output", "write result to FILE", "FILE")
        .positional("input", "file to read")
    parsed: spec.parse(process.args()) ?? return
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
| `spec.parse(args)` | `ParsedArgs !Err` | Parse without exiting, suitable for tests and custom errors. |
| `spec.parse_or_exit(args)` | `ParsedArgs` | Handle `--help` and usage errors through process exit. |
| `parsed.flag(name)` / `parsed.option(name)` | `Bool` / `?String` | Read a boolean or optional string value. |
| `parsed.option_int(name)` / `parsed.option_float(name)` | `?Int` / `?Float` | Read a typed numeric option. |
| `parsed.options(name)` / `parsed.positional(index)` | `[String]` / `?String` | Read repeated values or a zero-based positional. |
| `parsed.subcommand()` | `?String` | Read the matched subcommand name. |

`--help` and `--version` are recognized automatically. `parse` returns an `Err`
whose message is the refusal text rather than exiting; `parse_or_exit` prints help and exits zero for
`--help`, or prints usage and exits two for invalid arguments. The decoder also
handles `--key=value`, `--key value`, `--flag`, `--no-flag`, clustered short
flags, negative numeric values, and `--` as the end-of-options marker.

`args.decode<T>()` parses the process arguments into the `#CLI` struct `T`
through the same spec as `run: fn(args: T)`, so help and refusal text are
identical. `--help` and `--version` print and exit zero; a refused argument
returns `[FieldError]`. Every `#CLI` struct also gets
`T.merge(args, settings) -> T ![FieldError]`, unless it declares its own
`merge`. A field given explicitly on the command line takes its `args` value.
Every other field takes its `settings` value, which is the environment value
or the field default when `settings` comes from `env.decode<T>()`.

The direct functions are useful when a typed builder is unnecessary:

| API | Result | Description |
| --- | --- | --- |
| `decode()` | `Dyn` | Decode the current process arguments, including `help` and `h` flags. |
| `decode_argv(argv)` | `Dyn` | Decode an explicit argument vector. |
| `merge(base, overlay)` | `Dyn` | Recursively merge objects; overlay arrays and scalars replace. |
| `get_text` / `get_bool` / `get_int` | scalar values | Read typed scalar fields with empty/false/zero fallbacks. |
| `positionals(tree)` / `remainder(tree)` | `[String]` | Read positional values. |
| `program(tree)` / `wants_help(tree)` | `String` / `Bool` | Read `argv[0]` or help intent. |

See [`args_spec.jet`](../../../Examples/Systems/CLI/ArgsSpec.jet) and
[`args_audit.jet`](../../../Examples/Systems/CLI/ArgsAudit.jet).

### `core.reflect` — runtime structural reflection (D-ANY-JAI1)

`reflect.of(x)` is the compiler-owned intrinsic for values that can be rendered
by `"{x}"`; the `core.reflect` source module itself owns the explicit text
classifier `inspect`. Runtime reflection is read-only and retains typed field
values rather than pre-rendering them.

```jet
use reflect: core.reflect

Point: struct {
    x: Int
    y: Int

    impl Display {
        display: fn(self) -> String { "({self.x}, {self.y})" }
    }
}

run: fn() {
    p: Point{x: 3, y: 4}
    value: reflect.of(p)
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

Compiler fact reads such as `@function.effects` are separate from runtime
reflection. Projections of prepared fact records, including fields reached
through an optional fact, remain compile-time values. Tests against their
closed fact-enum variants fold before the IR is built; ordinary user enums retain
their nominal owner and are not folded as compiler facts.

### `core.sys` — environment and system facts

`core.sys` exposes the process environment, working directory, platform facts,
process identifiers, and explicitly unsafe POSIX controls. Jet source validates
environment names and values; `get` returns `None` for an unset name or a
non-Unicode value, while `unset` rejects invalid names and `vars` rejects
non-Unicode names or values. Names use the platform's environment-key order.
Mutation changes only Jet's process-lifetime logical overlay, never the host
environment block. Child overlays snapshot once, apply sets in order, then
apply removals; an explicitly empty value is not an absent value.

```jet
use sys: core.sys

run: fn() {
    home: sys.home_dir()
    mode: sys.get("MODE") ?? "dev"
    sys.set("MODE", "prod")
    removed: sys.unset("CI") ?? false
    names: sys.vars() ?? []
    here: sys.current_dir() ?? return
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
| `executable()` / `username()` | `String -[Env]>` | Return process facts. `executable()` is the running program's path: the built binary, or the script's absolute path under `jet run`; launching it runs the program again on either tier, and every later command word reaches the program as an argument (under `jet run` the relaunch places them after `--`). |
| `hostname()` | `String -[Env, FS]>` | Return the host name. |
| `release()` / `version()` | `String -[Env, FS, Exec, Time.Wait]>` | Read operating-system facts; macOS queries run `uname` or `sw_vers`. |
| `pid()` / `getpid()` / `getppid()` / `cpu_count()` | `Int -[Env]>` | Return process IDs or a logical CPU count of at least one. |
| `getuid()` / `geteuid()` / `getgid()` / `getegid()` / `getpgrp()` | `Int -[Env]>` | Return POSIX identity and process-group values where supported. |
| `getgroups()` | `[Int] -[Env]>` | Return supplementary group IDs. |
| `user_ids()` / `group_ids()` | `Ids !IOError -[Env]>` | `getresuid` / `getresgid`: the `real`, `effective` and `saved` IDs. |
| `users()` / `groups()` | `[User] !IOError -[FS.Read]>` / `[Group] !IOError -[FS.Read]>` | Every account of `/etc/passwd` or group of `/etc/group`, in file order. Core reads the file as musl does and asks no other directory service (NSS, LDAP); a line that is not UTF-8, lacks a field, or has an ID outside 0..4294967294 is skipped. A `User` has `name`, `uid`, `gid`, `gecos`, `home` and `shell`; a `Group` has `name`, `gid` and its supplementary `members`. |
| `user(name)` / `user_by_id(uid)` | `User? !IOError -[FS.Read]>` | The first account with that name or user ID, `None` when there is none. |
| `group(name)` / `group_by_id(gid)` | `Group? !IOError -[FS.Read]>` | The first group with that name or group ID, `None` when there is none. |
| `expand(value)` | `String -[Env]>` | Expand `$VAR`, `${VAR}`, and the supported percent form from the environment. |
| `uptime()` | `Float -[Time, Env, FS]>` | Read system uptime from `/proc/uptime` on Linux. |
| `loadavg()` / `times()` | `[Float] -[Env]>` | Return load averages or process CPU times. |
| `exitcode(status)` / `success(status)` | `Int` / `Bool` | Interpret a wait status without an effect. |
| `sync()` | `Unit -[FS, Env]>` | Flush filesystem buffers. |
| `getpgid(pid)` / `getsid(pid)` | `Int !IOError -[Env]>` | Inspect POSIX process-group or session state. |

Descriptors and shared memory are owned values: no descriptor number or
address reaches Jet code, every descriptor is close-on-exec, and each closes
exactly once when its owner is dropped. Every offset and length is a range
type, so a literal outside its range is a compile error (E0135) and a
computed one is converted first, with `Int(0..1073741824).from_int(n)`
handling its failure; only the end of an access is checked when it runs.

| API | Result | Description |
| --- | --- | --- |
| `shared_memory(size: Int(1..1073741824))` | `SharedMemory !IOError -[Env]>` | `size` zeroed bytes (1 B to 1 GiB) of anonymous shared memory (memfd), sealed against shrinking and growing so a peer that maps it can never fault. |
| `SharedMemory.write(offset: Int(0..1073741824), bytes)` / `.read(offset: Int(0..1073741824), count: Int(0..1073741824))` / `.len()` | `!IOError -[Env]>` / `[U8] !IOError -[Env]>` / `Int` | Copy bytes in or out within bounds. |
| `SharedMemory.write_pixels(offset: Int(0..1073741824), rgba, stride: Int(4..65536), x, y, width, height: Int(0..16384), format)` | `!IOError -[Env]>` | Copy the `width`×`height` rectangle at `x`, `y` of an RGBA image whose rows are `stride` bytes (a multiple of 4) into the same rectangle of the image at `offset`, converting each pixel to `PixelFormat.XRGB8888` (B, G, R, X in memory) or `.XBGR8888` (R, G, B, X). |
| `SharedMemory.fill_u32(offset: Int(0..1073741824), count: Int(0..268435456), value: Int(0..4294967295))` | `!IOError -[Env]>` | Store `count` little-endian copies of the 32-bit `value` from `offset`: a solid run of pixels. |
| `SharedMemory.descriptor()` | `Fd !IOError -[Env]>` | A new descriptor for the memory, to pass to another process. |
| `Fd.read_at(offset: Int(0..9223372036854775807), max: Int(1..16777216))` / `.size()` / `.duplicate()` | `[U8] !IOError -[Env]>` / `Int !IOError -[Env]>` / `Fd !IOError -[Env]>` | Read at most `max` bytes of a received descriptor's file from `offset` without mapping it (`pread`; fewer at its end), size it, or own a second descriptor for it. |
| `open_pipe()` | `[Fd] !IOError -[Env]>` | A new pipe: its read end, then its write end. Both block, so either can go to another process (a Wayland paste hands the write end to the copying client). |
| `Fd.read(max: Int(1..16777216), timeout_ns)` | `[U8] !IOError -[Env]>` | At most `max` bytes from a pipe or socket once it has some, waiting at most `timeout_ns` nanoseconds (negative: no limit). Empty means the writer closed its end; a wait that runs out is `.TimedOut`. |
| `Fd.write(bytes, timeout_ns)` | `Int !IOError -[Env]>` | Writes the first of `bytes`, at most 4096 (`PIPE_BUF`, which a pipe that polls writable takes whole), once there is room, waiting at most `timeout_ns`; gives the count written. A wait that runs out is `.TimedOut`, a closed reader `.Closed`. |

Resource limits and usage (D-CORE-RLIMIT1, D-CORE-RLIMIT-NAME1, #5072) apply
to the running program, and the children it starts afterwards inherit them.
Each getter returns `(soft:, hard:)`; `None` is unlimited. Sizes are bytes. The
kernel counts CPU time in whole seconds, so a fractional `Duration` rounds up
when set. Lowering a limit needs no privilege; raising a hard limit without
privilege is `IOError.PermissionDenied`. A soft limit above the hard one, or a
negative value, is `IOError.InvalidInput`. A target without these limits
refuses with an `IOError`. Setting a limit changes the process tree, so setters
need `Env, Exec`, as `fork()` does.

| API | Result | Description |
| --- | --- | --- |
| `cpu_time_limit()` / `set_cpu_time_limit(soft, hard)` | `(soft: Duration?, hard: Duration?) !IOError -[Env]>` / `!IOError -[Env, Exec]>` | CPU time (`RLIMIT_CPU`). |
| `memory_limit()` / `set_memory_limit(soft, hard)` | `(soft: Int?, hard: Int?) !IOError -[Env]>` / `!IOError -[Env, Exec]>` | Address space (`RLIMIT_AS`), the limit `ProcessSpec.memory_limit` sets for one child. |
| `open_file_limit()` / `set_open_file_limit(soft, hard)` | as above | Open descriptors (`RLIMIT_NOFILE`). |
| `core_file_limit()` / `set_core_file_limit(soft, hard)` | as above | Core-file size (`RLIMIT_CORE`); `set_core_file_limit(0, hard)` turns off core dumps. |
| `stack_limit()` / `set_stack_limit(soft, hard)` | as above | Stack size (`RLIMIT_STACK`). |
| `usage(of: UsageOf{.Program})` | `ResourceUsage !IOError -[Env]>` | `getrusage`: `user` and `system` CPU time as `Duration`, `peak_memory` in bytes. `.Children` sums every child the program has waited for. |
| `lock_memory(future: Bool{false})` | `!IOError -[Env]>` | `mlockall(MCL_CURRENT \| MCL_ONFAULT)`, plus `MCL_FUTURE` when `future`: each page stays in RAM from its first touch until the program ends. Past the locked-memory limit without privilege it is an `IOError`. There is no unlock: `munlockall` would also unlock a `Secret`'s pages. |

CPU scheduling (`sched(7)`) is a `Scheduling`: `.Normal`, `.Batch`, `.Idle`,
`.Fifo{priority: p}` or `.RoundRobin{priority: p}`, the last two real time at
priority 1 to 99. `pid` 0 is this program. Children started afterwards
inherit the policy, so `set_scheduling` needs `Env, Exec` like the limit
setters, and it is POSIX process control under `#Unsafe` like `setpriority`.

| API | Result | Description |
| --- | --- | --- |
| `scheduling(pid: Int{0})` | `Scheduling !IOError -[Env]>` | `sched_getscheduler` and `sched_getparam`. A `SCHED_DEADLINE` process is `IOError.Unsupported`; no such process is `IOError.NotFound`. |
| `set_scheduling(policy, pid: Int{0})` | `!IOError -[Env, Exec]>` | `sched_setscheduler`. A real-time priority outside 1 to 99 is `IOError.InvalidInput`; real time without `CAP_SYS_NICE` or an `RLIMIT_RTPRIO` allowance is `IOError.PermissionDenied`. |
| `round_robin_interval(pid: Int{0})` | `Duration !IOError -[Env]>` | `sched_rr_get_interval`: the slice a `.RoundRobin` process runs before the next of its priority; zero under `.Fifo`. |

`cpu_time` reads the CPU time of any process; `set_wall_clock` sets the
machine's wall clock, so it is POSIX control under `#Unsafe`. Monotonic
readings (`core.time.instant`, stopwatches) do not jump when it is set.

| API | Result | Description |
| --- | --- | --- |
| `cpu_time(pid: Int{0})` | `Duration !IOError -[Env]>` | `clock_getcpuclockid` and `clock_gettime`: the user and system time of every thread of process `pid`, this program when 0. No such process, or a pid naming a thread that is not a process, is `IOError.NotFound`; a negative pid is `IOError.InvalidInput`. |
| `set_wall_clock(time: DateTime)` | `!IOError -[Env]>` | `clock_settime(CLOCK_REALTIME)`. It needs `CAP_SYS_TIME`, otherwise it is `IOError.PermissionDenied`; a time before 1970 is `IOError.InvalidInput`. |

`usize_max()` is a pure `U64` constant derived at compile time from the
selected target's pointer layout (#4483), not from the
compiler process's environment or the inspected executable. Missing layouts
and pointer widths outside the one-to-eight-byte carrier fail at compile time
rather than substituting the host width.

The source-compatible `set` form reports an invalid environment call as E3001;
its future typed error form is a language-edition change, not a second Core
namespace. Names must be non-empty and contain neither NUL nor `=`; values
cannot contain NUL. `vars` does not silently skip an unrepresentable entry:
`EnvError.NonUnicode` reports it.

OS-fact fallback selection, release-file parsing, supplementary-group sizing,
load-average fallback, process-time conversion, and POSIX syscall error contexts
are implemented in `Core/Sys/Sys.jet`. Temporary native leaves provide only host
availability, syscall return values, errno, or host bytes. `release`/`version`
keep the first matching release-file field, including an empty field; they do
not interpret it as shell syntax.

POSIX process/session controls are `#Unsafe` functions: every call needs an OS
gate and an enclosing audited `#Unsafe("reason")` region (E3103 otherwise),
whether the call goes through the module alias or a direct import:

| API | Result | Description |
| --- | --- | --- |
| `umask(mask)` | `Int -[FS, Env]>` | Set and return the previous creation mask. |
| `getpriority(who)` | `Int !IOError -[Env]>` | Inspect scheduling priority. |
| `setpriority(who, priority)` | `!IOError -[Env]>` | Change scheduling priority. |
| `utime(path, atime, mtime)` | `!IOError -[FS, Env]>` | Change file timestamps. |
| `stop(code)` | never returns `-[Env]>` | Request process termination through the process boundary. |
| `fork()` / `setsid()` | `Int !IOError -[Env, Exec]>` | Fork or create a process session. Both change the process tree, so callers need `Exec`, like `process.cmd`. |
| `wait()` / `waitpid(pid, options)` | `Int !IOError -[Env, Time.Wait]>` | Reap a child or selected child. |
| `kill(pid, signal)` | `!IOError -[Env]>` | Send a signal. |
| `#Unsafe reset_signal(number)` | `!IOError -[Env]>` | Restore a signal's default action (SIG_DFL), the fault signals included, so the signal raised next takes it; a supervisor re-raising its child's signal uses it before `kill`. |
| `setuid` / `setgid` / `setpgid` / `setpgrp` / `initgroups` | `!IOError -[Env]>` | Change credentials, groups, or process-group membership. |
| `set_user_ids(real:, effective:, saved:)` / `set_group_ids(real:, effective:, saved:)` | `!IOError -[Env]>` | `setresuid` / `setresgid` on every thread at once; an omitted ID (`None`) stays. Without `CAP_SETUID` / `CAP_SETGID` each new ID must be one of the current real, effective or saved IDs, else `IOError.PermissionDenied`; an ID outside 0..4294967294 is `IOError.InvalidInput`. `seteuid(id)` is `set_user_ids(effective: id)`. Supplementary groups stay as they are. |
| `pipe()` / `close_fd(fd)` | `[Int] !IOError -[FS, Env]>` / `Unit -[FS, Env]>` | Create or close raw process descriptors. |
| `mkfifo(path, mode)` | `!IOError -[FS, Env]>` | Create a named pipe. |
| `mknod(path, mode, kind, major, minor)` | `!IOError -[FS, Env]>` | Create a `DeviceKind.Character` or `.Block` device file for `major`:`minor` (0..4095 and 0..1048575); only a privileged process may. |
| `atexit(handler)` | `Unit` | Register a `fn()` handler that runs at the explicit process boundary after scope cleanup (D-FAIL-EXIT1). |

`on_interrupt(handler)` (`Unit -[Env]>`) registers a `fn()` handler for SIGINT.
Registration is additive and lasts for the process: it is
live when the call returns, and nothing unregisters it. The first call installs
the SIGINT handler, which only counts the signal (async-signal-safe), and starts
the runtime's signal dispatcher thread. For every counted SIGINT the dispatcher
runs every registered handler in registration order, each under its own stop
boundary. A handler that stops (a panic, a deadline) has its report written to
stderr, and the later handlers still run. After the last handler, the first
stopped handler's exit code ends the process. `process.exit(code)` in a handler
ends the process at once. A handler runs on the dispatcher thread, concurrently
with the program. A program that never calls `on_interrupt` installs no SIGINT
handler and links no handler runner. Registration fails with a panic when the
host refuses the SIGINT handler or the dispatcher thread.

Typed `sys.decode<T>()` keeps the existing `Dotenv` annotation and `Decode`
protocol. Its source policy requires a project-relative file with no parent
components, treats a missing dotenv file as empty, folds environment names in
ASCII, and overlays logical process values over dotenv values. Empty values
remain values; `__` separates lowercase field segments. Decoder failures retain
their field paths and `E2416` provenance; secret/password/token/key path segments
redact the rejected value.
---

### `core.process` — process execution

`core.process` is the process-boundary reference for application and tool
authors who launch or supervise child processes. Its executable contract is
defined by [`Core/Process/Process.jet`](../../../Core/Process/Process.jet) and
tests such as
[`tests/cases/corelib/process_wait_observes_inherited_deadline.case`](../../../tests/cases/corelib/process_wait_observes_inherited_deadline.case).
The export registry is
[`Prelude/Core.jet`](../../../Compiler/Prelude/Core.jet); the
data example used later is
[`data_analysis.jet`](../../../Examples/Data/Analysis/DataAnalysis.jet).

`core.process` represents a child process as an explicit argument vector and a
policy-bearing `ProcessSpec`. `process.cmd` never invokes a shell: each
argument remains a separate string, including spaces and shell metacharacters.
Use `process.shell` only when the command line is intentionally delegated to
the platform shell (D-PROCESS1).

Command defaults are initialized by Jet before transfer into the temporary
native carrier: stdin is closed, stdout and stderr are captured, the output
limit is 64 MiB, and working directory, timeout, resource limits, terminal,
and authority binding are absent. An empty argument vector is accepted by
`cmd`; launching it reports the existing process error.

Compiler store subprocesses retain directory authority rather than reopening
its pathname: the private bridge duplicates the held directory into the process
carrier, then installs the chosen child descriptor and working directory before
exec. Closing or renaming the original directory cannot redirect that child to
a replacement at the old name. This is not a public `ProcessSpec` builder;
targets without this descriptor transfer report `Unsupported` rather than
falling back to a lexical working directory.

```jet
use process: core.process

run: fn() -[Exec, Time.Wait]> {
    timeout: Duration.seconds(30) ?? return
    spec: process.cmd(["cp", "--", "directory with spaces;*.tmp", "backup"])
        .stdout(.Capture)
        .stderr(.Capture)
        .timeout(timeout)
    receipt: spec.run_checked() ?? return
    print(receipt.output)
}
```

| Function or type | Returns | Description |
| --- | --- | --- |
| `process.args()` | `[String] -[Exec]>` | Return the words after the program name, as the host supplied them. |
| `process.cmd(argv)` | `ProcessSpec -[Exec]>` | Build a specification from an explicit executable-and-arguments vector. |
| `process.shell(line)` | `ProcessSpec -[Exec, Env]>` | Explicit shell escape hatch: `sh -c` on Unix-like targets and `cmd /C` on Windows. |
| `process.run_spec(spec)` | `ProcessReceipt !IOError -[Exec, Time.Wait]>` | Run a specification and return its receipt without treating a nonzero exit as a thrown error. |
| `process.pipeline(steps)` | `ProcessReceipt !IOError -[Exec, Time.Wait]>` | Run a sequence of specifications and return the pipeline receipt. |
| `process.run_all(specs, jobs)` | `[Result<ProcessReceipt, IOError>] !IOError -[Exec, Time.Wait]>` | Run every specification through a bounded worker pool, at most `jobs` at once, and return one result per spec in input order. |
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
| `process.which(name)` | `?String -[FS, Env]>` | Find an executable using filesystem checks and the host search path. A candidate that is a link counts when its target is a file; `files.which` and `Path.which` search the same way. |
| `process.exit(code)` | `Never -[Exec.Exit]>` | Terminate the current process at the explicit process boundary. Status 101 is reserved for Jet defects, so `exit(101)` ends the process with the user-error status 1 (S36). Ending its own process is the narrow `Exec.Exit` leaf, so a caller bound to `-[Exec.Exit]>` checks clean without the whole `Exec` root. |
| `#Unsafe process.exit_unmapped(code)` | `Never -[Exec.Exit]>` | Terminate at the same boundary with `code` unchanged, 101 included; for a tool that reports a Jet defect, such as a compiler's internal error. |
| `process.on_signal(signal)` | `Unit -[Exec]>` | Register the process signal policy for one `ProcessSignal` value. |
| `process.signal_number(signal)` | `Int` | Convert a `ProcessSignal` value to its platform signal number where one exists. |
| `process.list2cmdline(argv)` | `String` | Format an argument vector for display using Windows command-line quoting rules. |
| `ProcessSignal` | type | Closed variants: `Term`, `Hup`, and `Int`, matching the shared signal dispatcher. |
| `ProcessReceipt` | type | The immutable result of a run, including status, output, policy digests, backend, authority, descendants, limits, redaction facts, and the child's resource usage (`peak_memory_bytes`, `user_cpu_ns`, `system_cpu_ns`). |
| `ProcessSpec` | type | The executable, arguments, stream policy, authority, terminal policy, limits, and detachment policy for a child. |

A child that cannot start is an `IOError`, never a receipt: its kind
follows the launch's OS error the way a file operation's does (`NotFound`
for a missing program, `PermissionDenied` for one that may not be run,
`Other` for the rest), with operation `.Resolve`, the program as its
resource and the OS error code. An empty command (`process.cmd([])`) or an
empty `process.pipeline([])` is `InvalidInput` naming that empty call. A
program that starts and then exits nonzero, 127 included, is a receipt.

`process.run_all(specs, jobs)` is the bounded worker pool. It runs the specs
in one task group with at most `jobs` children alive at once and returns the
results in input order, whatever order the children finish in. Each slot is
independent: a nonzero exit is an `Ok` receipt, a spec that cannot start is
an `Err` in its own slot, and a spec's own `.timeout(...)` is its per-job
timeout (the receipt reports `timed_out`). Every child is marked to die with
the calling process (Linux `PR_SET_PDEATHSIG`, `SIGKILL`), except a
`.detached()` spec, which is meant to outlive it. `jobs` below 1 is
`InvalidInput`.

A receipt reports what the child used, read from the kernel when the child is
reaped (`wait4`'s `rusage`). `peak_memory_bytes` is the peak resident memory of
the largest single process: the child or one descendant it waited for
(`ru_maxrss`), never the sum of the tree, except under `.memory_scope`, where it
is the scope's peak: all its processes together. `user_cpu_ns` and
`system_cpu_ns` are the CPU time of the child and every descendant it waited
for. A pipeline receipt takes the largest step's peak and sums the steps' CPU
time. All three are `None` when no usage was taken, such as a host without
`wait4` usage.

#### Building a process policy

`ProcessSpec` builder methods return a modified specification. `.arg(value)`
and `.args_extend(values)` append arguments; `.cwd(path)` selects the working
directory; `.env(name, value)`, `.env_remove(name)`, and `.env_clear()` control
the child environment. `.stdin(mode)`, `.stdout(mode)`, and `.stderr(mode)`
select one of `.Stream`, `.Inherit`, or `.Capture`; `.stderr(.Stdout)` merges
the child's error output into its standard output (see merged output below).
`.terminal()` requests a
terminal session, while `.terminal(policy)` supplies an explicit
`TerminalPolicy`. `.detached()` allows the child to outlive the caller.
`.abilities()` returns the open `Set<String>` report of what a terminal-backed
launch can use. Its stable keys are `TerminalFact.terminal`,
`TerminalFact.resize` and `TerminalFact.raw`; any other string key is a preview
fact. A string key one or two edits from a stable key is E0302 naming that
`TerminalFact` key, and an unknown `TerminalFact.name` is E0302 too.

A specification can also carry `.timeout(duration)`, `.cpu_time_limit(value)`,
`.memory_limit(value)`, `.open_file_limit(value)`, and `.output_limit(value)`.
The limits are part of the process plan and receipt. `.under(authority)`
places the child under an `Authority`; `.plan()` returns the planned executable
identity, redacted argument vector, input digest, policy digest, backend,
authority, descendants, and limits without starting the child. Inside
`#FX(grant: …) { … }`, `.under(grant)` delegates the scoped grant, so the
specification it returns is scoped like the grant: build on it and run it
(`.run()`, `.run_checked()`, `.plan()`) inside the block; storing, returning,
or passing it is E0711.

Resource-limit support is typed and fail-closed. Wall time and output limits
are represented in the process policy. Native CPU-time, memory, and open-file
limits are refused with an `IOError` when a target cannot enforce them; the
implementation does not silently claim a limit that the target cannot provide.
The macOS backend refuses memory and open-file limits, Windows refuses
open-file limits, and unsupported targets refuse the corresponding native
CPU, memory, or open-file requests.

`.cpu_affinity(cpus: [Int])` runs the child only on the listed CPUs: the
child sets its own affinity before it executes the program, so no
instruction of the program runs elsewhere, and its descendants inherit the
set. The receipt and plan limits carry `cpu-affinity=0-3,8` (the CPUs
ascending, each run as `first-last`) only when the spec sets them. An empty
list is `IOError.InvalidInput` (`cpu_affinity needs at least one CPU`), as
is a negative CPU (`cpu -1 is negative; CPUs are numbered from 0`) and a CPU
past the 1024-CPU affinity set (`cpu 2048 is past the 1024-CPU affinity
set`); nothing starts. A set the kernel refuses, such as one with no CPU
online or allowed to the caller, fails the launch with the kernel's error.
`Examples/Systems/Process/ProcessAffinity.jet` is the golden.

`.io_priority(level: Int)` runs the child in the best-effort I/O scheduling
class at `level` 0 (first) through 7 (last), and `.io_idle()` in the idle
class, which gets the disk only when no other process wants it; the later of
the two calls wins. The child sets the class on itself before it executes the
program, and its descendants inherit it. The receipt and plan limits carry
`io-priority=idle` or `io-priority=best-effort:4` only when the spec sets a
class. A level outside 0..7 is `IOError.InvalidInput` (`io_priority level 8
is outside 0..7`); nothing starts.
`Examples/Systems/Process/ProcessIOPriority.jet` is the golden.

`.memory_scope(bytes: Int)` runs the child and every process it starts in a
memory scope of their own (a cgroup v2 cgroup on Linux), capped at `bytes`
together, with no swap: going over the cap kills every process in the scope,
and never the parent. The scope is made next to the parent's own cgroup, so
that cgroup's parent must be writable and offer the memory controller, as a
systemd user session's `app.slice` does; a host without one fails the launch
with the kernel's error on the path it could not make or write
(`PermissionDenied`, `NotFound`). A Jet program inside a scope that asks for a
memory scope of its own gets one inside it, under the outer cap. When the
child is reaped, the receipt's `peak_memory_bytes` is the scope's peak, every
process still in the scope is killed, and the scope is removed; a child never
reaped ends its scope with its last handle. The receipt and plan limits carry
`memory-scope=<bytes>` only when the spec sets a scope. A cap below 1 byte is
`IOError.InvalidInput` (`memory_scope needs a cap of at least 1 byte`), as is
a detached spec (`a detached process outlives its handle, and a memory_scope
ends with it`); nothing starts.
`Examples/Systems/Process/ProcessMemoryScope.jet` is the golden.

`.keep_open(handle, at: Int?{None})` keeps a `FileReader`, `FileWriter`,
`FileLock` or `Directory` open in the child at descriptor `at`, or at the
handle's own descriptor number when `at` is `None`. The spec takes the handle:
the parent's descriptor closes at the call, and the child's duplicate is the
spec's, so renaming or replacing the file or directory afterwards does not
change what the child sees through the slot. The call never fails; a refusal
is kept on the spec, and `.spawn()`, `.run()`, `.run_checked()` and `.plan()`
return it before any child starts. A slot outside 3 to 2147483646 is
`IOError.InvalidInput` (`keep_open needs a descriptor slot from 3 to
2147483646; 0 to 2 are the standard streams`), as is a slot the spec already
keeps; a descriptor the host cannot duplicate is `IOError.Other` with the
host's error. A kept `FileLock`'s duplicate shares its lock: the lock stays
held while the child, or any process it starts, keeps the descriptor open.
`Examples/Systems/Process/ProcessKeepOpen.jet` is the golden.
#### Authority and output boundaries

An authority is explicit at the process boundary. `Authority.from_rights` can
construct a restricted authority from rights supplied by the caller, and
`Authority.workspace()` supplies the workspace authority used by examples.
The child inherits only the authority selected by the specification; an
unavailable or disallowed operation is an error rather than an implicit host
escape. This explicit authority boundary is the process execution contract (D-AGENT-EXEC1=A).

```jet
run_with_authority: fn() -[Exec, Time.Wait]> {
    policy: Authority.from_rights([
        "FS.Read:repo",
        "FS.Write:.jet/build",
        "Exec:/usr/bin/cargo",
    ])
    spec: process.cmd(["cargo", "test"]).cwd("/workspace").under(policy)
    receipt: process.run_spec(spec) ?? return
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
separate APIs. An authority-bound specification that keeps a parent handle
open (`.keep_open`) is refused the same way, at `.plan()` and at launch.

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

Merged output: `.stderr(.Stdout)` gives the child one descriptor for both
streams, a copy of whatever `.stdout(mode)` selected, so the two arrive
interleaved in the child's write order. With `.stdout(.Capture)` both land in
`receipt.output`; with `.stdout(.Stream)` both arrive on `child.stdout`; with
`.stdout(.Inherit)` both go to the parent's standard output. `receipt.errors`
is `""` and `child.stderr` reaches end of input at once. `.Stdout` names the child's
standard output, so `.stdin(.Stdout)` and `.stdout(.Stdout)` fail the launch
with `IOError.InvalidInput`. A terminal session ignores the stream modes, and a
detached child discards both streams. An authority-bound specification accepts
`.stdout(.Capture).stderr(.Stdout)`: the merged output is still the captured,
redacted receipt output.

| ProcessChild operation | Returns | Description |
| --- | --- | --- |
| `child.id()` | `Int` | Return the child identifier. |
| `child.wait()` | `ProcessReceipt !IOError` | Wait for completion and return the receipt. |
| `child.exited()` | `Bool !IOError` | Poll without waiting. |
| `child.kill()` / `.terminate()` / `.interrupt()` | `Unit !IOError` | Send the corresponding termination request. |
| `child.stdin.write(bytes)` | `Int !IOError` | Write once to a streamed child input and return how many bytes the pipe took. |
| `child.stdin.write_all(bytes)` / `.write_text(text)` | `Unit !IOError` | Write every byte, or the UTF-8 bytes of the text. |
| `child.stdin.close()` | `Unit` | Close a streamed child input; repeated close is harmless. |
| `child.stdout.read(limit)` / `child.stderr.read(limit)` | `[U8] !IOError` | Read up to `limit` bytes from the shared buffered pipe position; an empty list is end of input, and a limit below one is `IOError.InvalidInput` before any read. |
| `child.stdout.chunks(size)` / `child.stderr.chunks(size)` | loop source of `[U8]` | Yield each non-empty piece of at most `size` bytes (default 65536) from the shared buffered pipe position. The loop ends at end of input or at the first failed read; a size below one ends it before any read. |
| `child.stdout.lines()` / `child.stderr.lines()` | line stream | Read from the shared buffered pipe position, removing one LF and its preceding CR only. A final bare CR remains; invalid UTF-8 is `IOError.Other` on `process output`. The parent remains responsible for draining it. |

A terminal session is an opt-in process mode. It supplies terminal-sized input
and output through the terminal policy and fails with a typed error when the
selected backend cannot provide one; it is not an implicit replacement for
captured or inherited streams (D-PROCESS-SESSION1=A, D-PROCESS-SESSION2=D).

### `core.math` — numeric functions

`core.math` supplies integer operations, IEEE floating-point wrappers, numeric
predicates, checked and saturating arithmetic, and elementary functions. The
floating-point constants `math.PI`, `math.E`, `math.TAU`, `math.INFINITY` and
`math.NAN` are comptime values read as fields, not calls, so they also work in
`comptime` contexts.
The numeric `/%` operator rounds division toward negative infinity for both
exact `Int` and floating-point operands (`-7 /% 2` is `-4`). Its in-place
form is `/%=`, and whole-number floor division pairs with floored `%`.
`value.abs()` on the primitive `Int` loads and calls the canonical source
`core.math.abs(value)` route even without an explicit math import. It keeps
exact arbitrary-precision `Int -> Int Never!` behavior, without narrowing through
`Float` or a machine-sized integer. User nominal receivers and `Complex` retain
their own method namespaces; the source-backed primitive route grants neither
an `abs` method nor a Core identity to a same-spelled user type.

```jet
use math: core.math

run: fn() {
    angle := math.PI / 4.0
    print(math.sin_cos(angle))
    print(math.checked_add(2, 3) ?? 0)
    print(math.is_finite(math.sqrt(2.0)))
}
```

Numeric arithmetic methods need no import (D-SIGIL-ALGEBRA1=B):

| Method | Receiver | Description |
| --- | --- | --- |
| `value.bit_and(other)` | Int and each fixed-width integer | Keep shared bits; the usual integer join determines the width. |
| `value.bit_or(other)` | Int and each fixed-width integer | Keep bits set in either operand. |
| `value.bit_xor(other)` | Int and each fixed-width integer | Keep bits set in exactly one operand. |
| `value.bit_not()` | Int and each fixed-width integer | Flip every bit, keeping the width; exact Int returns `-value - 1`. |
| `value.pow(exponent)` | Int and Float | Numeric power with the existing exactness and negative-exponent rules. A written negative Int exponent produces Fraction; a dynamic negative Int exponent stops with the existing arithmetic fault. On Float and F32 it is C23 `pow`, correctly rounded, so every tier returns the same bits: 1 for `x^0` and `1^y` (even at a quiet NaN), the IEEE 754 zeros and infinities for a zero or infinite operand, a negative `x` to a whole `y` signed by `y`'s parity, and NaN for a negative `x` to a fractional `y`. |

These intrinsic methods lower to the ordinary typed numeric operations, not
user function calls, so every tier computes the same bits; Float and F32
`pow` call Core's correctly rounded power.
An update writes `value = value.bit_xor(other)`. `!value` is Boolean
negation only. Shifts retain `<<` and `>>`.

Floating classifiers such as `math.is_nan` require Float or F32, not Int.
Multi-operand floating calls such as `math.hypot`, and floating `pow`
operands, must use matching Float/F32 widths; convert explicitly before
calling them. Source-owned Core wrappers preserve these same constraints.
An all-F32 call of a correctly rounded function below (`sin`, `cos`, `tan`,
`asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh`,
`atanh`, `exp`, `exp2`, `exp10`, `exp_m1`, `ln`, `ln_1p`, `log2`, `log10`,
`cbrt`, `hypot`, `rsqrt`, `erf`, `erfc`, `gamma`, `lgamma`, `sin_pi`,
`cos_pi`, `tan_pi`, `asin_pi`, `acos_pi`, `atan_pi`, `atan2_pi`, `exp2_m1`,
`exp10_m1`, `log2_1p`, `log10_1p`, `pow_r`, `pow_n`, `root_n`,
`compound_n`), and F32 `pow`,
returns the correctly rounded `F32` result on every tier, never
a double rounding of the Float one (C23's `f` forms such as `rsqrtf` and
`pownf`); the whole exponent of `pow_n`, `root_n` and `compound_n` stays an
`Int`.

| Function | Returns | Description |
| --- | --- | --- |
| `math.acos(x)`, `asin(x)`, `atan(x)`, `atan2(y, x)` | `Float` | Inverse trigonometric functions, correctly rounded, so every tier returns the same bits: `asin` in [-pi/2, pi/2] and `acos` in [0, pi] with NaN for `abs(x) > 1`, `atan` in [-pi/2, pi/2] with ±pi/2 at the infinities, and `atan2` the angle of the point `(x, y)` in [-pi, pi] with the sign of `y` and the C23 values at zeros and infinities; `asin`, `atan` and `atan2` keep the sign of a zero result (C23 `asin`, `acos`, `atan`, `atan2`). |
| `math.acosh(x)`, `asinh(x)`, `atanh(x)` | `Float` | Inverse hyperbolic functions, correctly rounded, so every tier returns the same bits: `acosh` is 0 at 1 and NaN below 1, `asinh` and `atanh` are odd, and `atanh` is ±inf at ±1 and NaN for `abs(x) > 1` (C23 `acosh`, `asinh`, `atanh`). The extended float family is part of the shared math surface (D-CORESURFACE1). |
| `math.abs(value)` / `math.abs_float(value: Float)` | the argument type (`Float` for `Complex`) | Absolute value of an `Int`, `Float`, `F32`, `I64` or `Complex`. On `Float` and `F32` it clears the sign bit, so `-0.0` gives `+0.0` and a negative NaN loses its sign; on `I64` it is fixed-width negation, which stops with E3010 at `I64.MIN`. |
| `math.copy(value: Float)` | `Float` | Return the floating-point value unchanged, including its IEEE representation. |
| `math.is_even(value)` / `math.is_odd(value)` | `Bool` | Test integer parity. |
| `math.sign(value: Int)` | `Int` | Return `-1`, `0`, or `1` for a negative, zero, or positive integer. |
| `math.isqrt(value)` | `?Int` | Return the integer square root when the input is nonnegative. |
| `math.gcd(a, b)` / `math.lcm(a, b)` | `Int` | Greatest common divisor or least common multiple. |
| `math.gcd_many(values)` / `math.lcm_many(values)` | `Int` | Fold integer gcd or lcm over a list. |
| `math.sin(x)`, `cos(x)`, `tan(x)` | `Float` | Sine, cosine and tangent of `x` radians, correctly rounded for every finite `x` however large, so every tier returns the same bits: NaN for an infinity, and `sin` and `tan` keep the sign of a zero (C23 `sin`, `cos`, `tan`). |
| `math.cosh(x)`, `sinh(x)`, `tanh(x)`, `math.cot(x)` | `Float` | Hyperbolic functions, correctly rounded, so every tier returns the same bits: `cosh` is even and at least 1, `sinh` and `tanh` are odd, `cosh` and `sinh` overflow to ±inf from about ±710.48 (C23 `cosh`, `sinh`, `tanh`); and the cotangent `1 / tan(x)`. |
| `math.sin_pi(x)`, `cos_pi(x)`, `tan_pi(x)` | `Float` | `sin`, `cos` and `tan` of `pi * x`, correctly rounded, so every tier returns the same bits and every exact 0 or ±1 comes out exact: a zero with the sign of `x` for `sin_pi` at whole `x`, `+0` for `cos_pi` at `x = 1/2 mod 1`, an infinity for `tan_pi` there, and NaN for an infinity (C23 `sinpi`, `cospi`, `tanpi`). |
| `math.asin_pi(x)`, `acos_pi(x)`, `atan_pi(x)`, `atan2_pi(y, x)` | `Float` | `asin`, `acos`, `atan` and `atan2` divided by `pi`, correctly rounded, so every tier returns the same bits: `asin_pi` in [-1/2, 1/2] and `acos_pi` in [0, 1] with NaN for `abs(x) > 1`, `atan_pi` in [-1/2, 1/2] with ±1/2 at the infinities, and `atan2_pi` the angle of the point `(x, y)` in half-turns, in [-1, 1] with the sign of `y` and the C23 values at zeros and infinities (C23 `asinpi`, `acospi`, `atanpi`, `atan2pi`). |
| `math.exp(x)`, `exp2(x)`, `exp10(x)`, `exp_m1(x)`, `expm1(x)` | `Float` | `e^x`, `2^x`, `10^x` and `e^x - 1`, correctly rounded, so every tier returns the same bits: `+inf` on overflow, 0 (-1 for `exp_m1`) at `-inf`, `exp2` and `exp10` exact wherever the power is a Float, and `exp_m1` keeps the sign of a zero (C23 `exp`, `exp2`, `exp10`, `expm1`). |
| `math.erf(x)`, `math.erfc(x)` | `Float` | The error function and `1 - erf(x)`, correctly rounded, so every tier returns the same bits and `erfc` keeps full precision where `erf` is near 1: `erf` is odd, keeps the sign of a zero and is ±1 beyond about ±5.92, and `erfc` is 2 below about -5.86 and `+0` from about 27.23 (C23 `erf`, `erfc`). |
| `math.exp2_m1(x)`, `exp10_m1(x)` | `Float` | `2^x - 1` and `10^x - 1`, correctly rounded, so every tier returns the same bits and small `x` keeps full precision: -1 at `-inf`, `+inf` on overflow, and a zero keeps its sign (C23 `exp2m1`, `exp10m1`). |
| `math.ln(x)`, `ln_1p(x)`, `log1p(x)`, `log10(x)`, `log2(x)` | `Float` | `log(x)`, `log(1 + x)`, `log10(x)` and `log2(x)`, correctly rounded, so every tier returns the same bits: NaN below 0 (below -1 for `ln_1p`), `-inf` at ±0 (at -1), `+inf` at `+inf`, `log2` and `log10` exact at every power of 2 or 10 that is a Float, and `ln_1p` keeps the sign of a zero (C23 `log`, `log1p`, `log10`, `log2`). |
| `math.logb(x)`, `log(x, base)` | `Float` | The binary exponent as a Float, and the logarithm in any base, `ln(x) / ln(base)`. |
| `math.log2_1p(x)`, `log10_1p(x)` | `Float` | `log2(1 + x)` and `log10(1 + x)`, correctly rounded, so every tier returns the same bits and small `x` keeps full precision: NaN below -1, `-inf` at -1, `+inf` at `+inf`, exact at every `x = 2^k - 1` and `x = 10^k - 1` (`k` in [1, 15]), and a zero keeps its sign (C23 `log2p1`, `log10p1`). |
| `math.sqrt(x)`, `cbrt(x)`, `hypot(x, y)` | `Float` | Square root, cube root and `sqrt(x^2 + y^2)`, correctly rounded, so every tier returns the same bits: `cbrt` is odd and exact at every exact cube, and `hypot` never overflows or underflows early and is `+inf` when either side is infinite, even beside a quiet NaN (C23 `sqrt`, `cbrt`, `hypot`). |
| `math.rsqrt(x)` | `Float` | `1 / sqrt(x)` correctly rounded, so every tier returns the same bits: `+inf` for `+0`, `-inf` for `-0`, `+0` for `+inf` and NaN below zero (C23 `rsqrt`). |
| `math.pow_n(x, n)`, `pow_r(x, y)` | `Float` | `x^n` for every whole `n: Int`, taken exactly however large, and `e^(y ln x)`, both correctly rounded, so every tier returns the same bits (C23 `pown`, `powr`). `pow_n` gives 1 for `n = 0` even at a NaN, and a negative result for a negative `x` and odd `n`; `pow_r` gives NaN for `x < 0`, `0^0`, `inf^0` and `1^inf`, and `+inf` for `0^y` with `y < 0`. Float `value.pow(y)` is C23 `pow`. |
| `math.root_n(x, n)`, `compound_n(x, n)` | `Float` | The `n`th root of `x` and `(1 + x)^n` for every whole `n: Int`, both correctly rounded, so every tier returns the same bits (C23 `rootn`, `compoundn`). `root_n` gives NaN for `n = 0` and for `x < 0` with `n` even, and a negative root for `x < 0` with `n` odd; `compound_n` gives 1 for `n = 0` (even at a NaN), NaN for `x < -1`, and never rounds `1 + x` first. |
| `math.floor(x)` / `math.ceil(x)` / `math.trunc(x)` | `Float` | Directed or toward-zero rounding in floating-point form. |
| `math.fract(x)` | `Float` | Return the fractional part. |
| `math.gamma(x)` / `math.lgamma(x)` | `Float` | The gamma function and `ln(abs(gamma(x)))`, correctly rounded, so every tier returns the same bits: `gamma` is ±inf at ±0, NaN at every negative whole `x` and at `-inf`, and `+inf` from about 171.62; `lgamma` is `+inf` at zero, at every negative whole `x` and at ±inf (C23 `tgamma`, `lgamma`). |
| `math.degrees(x)` / `math.radians(x)` | `Float` | Convert between degrees and radians. |
| `math.lerp(a, b, t)` | `Float` | Linear interpolation. |
| `math.copysign(value, sign)` / `math.fma(a, b, c)` | `Float` | Copy a sign, or `a * b + c` rounded once (IEEE 754 `fusedMultiplyAdd`), the same bits on every tier. A nonzero exact result that rounds to zero keeps its sign, so `fma(2^-1074, -0.5, 0.0)` is `-0.0`, where musl's C fma gives `+0.0`. Native x86-64 code runs one `vfmadd213sd` when the CPU has FMA3 (probed at a module's first fma) and the soft fma otherwise, with the same bits either way. A plain `a * b + c` is never fused: it rounds the product and then the sum on every tier. |
| `math.muladd(a, b, c)` | `Float` | Multiply and add through the math wrapper. |
| `math.inv(x)` / `math.signum(x)` | `Float` | Reciprocal or floating-point signum. |
| `math.fabs(x)` / `math.real(x)` / `math.imag(x)` / `math.conj(x)` | `Float` | Absolute floating-point value and scalar component helpers. |
| `math.is_nan(x)`, `is_inf(x)`, `is_finite(x)` | `Bool` | Test NaN, infinity, or finite status. `is_finite` accepts both `Float` and `F32`. `isnan`, `isinf`, and `isfinite` are aliases. |
| `math.cmp(a, b)` | `Int` | Accept matching `Float` or `F32` operands and return `-1`, `0`, or `1`; NaN sorts after non-NaN values, two NaNs compare equal, and signed zeros compare equal. |
| `math.is_normal(x)`, `is_subnormal(x)`, `is_canonical(x)`, `is_signed(x)`, `is_zero(x)`, `is_integer(x)`, `sign_bit(x)` | `Bool` | Inspect floating-point representation properties. |
| `math.next_up(x)` / `next_down(x)` / `next_after(x, toward)` / `nextafter(x, toward)` | `Float` | Move to an adjacent representable value. `next_after` is C `nextafter` (and `nexttoward`, since `Float` is the widest type): a NaN operand gives a NaN, equal operands give `toward`. |
| `math.significand(x)` / `math.ulp(x)` | `Float` | Return the significand or unit in the last place. |
| `math.ilogb(x)` / `math.ldexp(x, exponent)` / `math.scaleb(x, exponent)` / `math.radix(x)` | `?Int` or `Float` | Inspect or scale the floating-point exponent and radix. `ilogb` counts a subnormal's exponent from its leading bit and is `None` for a zero, an infinity or a NaN; `ldexp` and `scaleb` round once, as C `scalbn`. |
| `math.logb(x)` | `Float` | The exponent as a `Float`: `-inf` for a zero, `+inf` for an infinity, NaN for a NaN. |
| `math.round_even(x)` | `Float` | Round to the nearest integral value, ties to even (C23 `roundeven`, and `rint`/`nearbyint` in the default rounding mode). |
| `math.round_to_signed(x, mode, bits)` / `math.round_to_unsigned(x, mode, bits)` | `?Int` | Round in `mode` (a `math.Rounding`: `TowardNegative`, `TowardPositive`, `TowardZero`, `TiesToAway` or `TiesToEven`) to an integer that fits a two's-complement or unsigned integer of `bits` bits (C23 `fromfp`/`ufromfp`; widths above 64 read as 64). `None` for an infinity, a NaN, `bits` below 1 or a result outside the width. |
| `math.remainder(a, b)` / `math.rem_quo(a, b)` | `Float` / `(rem: Float, quo: Int)` | IEEE remainder `a - n * b` for the integer `n` nearest `a / b`, ties to even; `rem_quo` also returns the low 31 bits of `n` with the sign of `a / b` (C `remquo`). |
| `math.fdim(a, b)` | `Float` | `a - b` when `a` is greater, else `+0`; a NaN operand is the result. |
| `math.min_float(a, b)` / `math.max_float(a, b)` | `Float` | C `fmin`/`fmax`: a NaN operand gives the other, and `-0` orders below `+0`. |
| `math.minimum(a, b)` / `maximum(a, b)`, `minimum_num(a, b)` / `maximum_num(a, b)`, `minimum_mag(a, b)` / `maximum_mag(a, b)`, `minimum_mag_num(a, b)` / `maximum_mag_num(a, b)` | `Float` | IEEE 754-2019 minimum and maximum (C23 `fminimum` family): `-0` orders below `+0`; the plain and `_mag` forms give a NaN when either operand is one, the `_num` forms give a NaN only when both are; the `_mag` forms compare magnitudes first. |
| `math.total_order(a, b)` / `math.total_order_mag(a, b)` | `Bool` | IEEE 754 `totalOrder`: whether `a` sorts at or before `b` when `-0` precedes `+0` and NaNs sort beyond the infinities by sign and payload; `total_order_mag` compares magnitudes. |
| `math.is_signaling(x)` / `math.canonicalize(x)` | `Bool` / `Float` | Test for a signaling NaN, or quiet one while keeping its sign and payload; any other value is itself. |
| `math.get_payload(x)` / `math.set_payload(p)` / `math.set_payload_signaling(p)` | `?Float` | Read a NaN's payload as an integral `Float`, or build the positive quiet or signaling NaN with payload `p`; `None` for a value that is not a NaN, or a payload that is negative, fractional, at least 2^51, or (signaling) zero. |
| `math.PI` / `math.E` / `math.TAU` | `Float` | Mathematical constants, declared as comptime values (`PI: Float{…}`); usable in `comptime` contexts. |
| `math.INFINITY` / `math.NAN` | `Float` | The IEEE-754 special values `1.0 / 0.0` and `0.0 / 0.0`, as comptime constants. `NAN` compares unequal to every value, itself included; test it with `math.is_nan`. |
| `math.round(x)` | `Int` | Round to the nearest integer, with ties away from zero. |
| `math.to_bits(x)` / `math.from_bits(bits)` | integer bits / `Float` | Convert between a floating value and its bit representation. |
| `math.sin_cos(x)` | `(cos: Float, sin: Float)` | Compute cosine and sine together, each correctly rounded as `cos` and `sin` give it. |
| `math.modf(x)` | `(fract: Float, whole: Float)` | Split a value into fractional and whole parts, each with the sign of `x`; an infinity has fraction `±0`. |
| `math.frexp(x)` | `(exp: Int, frac: Float)` | Split a value into `frac * 2^exp` with `abs(frac)` in [0.5, 1) (C `frexp`); a zero, an infinity or a NaN is its own `frac` with `exp` 0. |
| `math.checked_abs(x)`, `checked_neg(x)`, `checked_add(a,b)`, `checked_sub(a,b)`, `checked_mul(a,b)`, `checked_div(a,b)`, `checked_rem(a,b)`, `checked_pow(a,b)` | `?Int` | Return `None` on integer overflow or invalid division/remainder; checked power also rejects a negative exponent. |
| `math.int_pow(base, exponent)` | `Int` | Exact integer exponentiation through `base ^ exponent`; a negative runtime exponent stops with E3010 instead of returning a sentinel. |
| `math.saturating_add(a,b)`, `saturating_sub(a,b)`, `saturating_mul(a,b)` | `Int` | Perform integer arithmetic while clamping overflow to the representable endpoint. |
| `math.factorial(n)` / `math.binomial(n, k)` / `math.perm(n, k)` | `?Int` | Return optional combinatorial results when the input or result is outside the supported integer domain; `comb` aliases `binomial`. |

#### Exact numbers

`Fraction` stores an arbitrary-precision numerator and a positive denominator,
reduced by their greatest common divisor. Zero denominators are rejected;
division by a zero fraction stops. A finite `Float` converts using its exact
binary significand and exponent, without a decimal-text round trip.
`Decimal` stores a sign, arbitrary-precision magnitude, and decimal scale.
Parsing retains written scale, addition and subtraction use the finer operand
scale, and multiplication removes trailing zeros only down to that finer scale.
Division returns a `Fraction`; conversion to a `Decimal` rejects repeating
denominators. Decimal rounding uses ties away from zero.
Fraction arithmetic uses the ordinary `+`, `-`, `*`, and `/` operators.
The source declarations own their arithmetic and comparison hooks; their
results use normalized exact policies, not fieldwise record derivation.
Decimal's named hooks, rounding, and exact rendering are also source methods,
so loaded nominal identities and bare exact values expose the same behavior.

The policies and rendering live in `Core/Math/Math.jet`. Checked precise
operations resolve their reserved helpers by the loaded math module identity;
they do not resolve a user function with the same helper spelling.
The bare and `core.math`-qualified `Fraction`, `Decimal`, and `Complex`
spellings name the same declarations; local user types retain their own
declaration identity. These are reserved source-owned declarations, not duplicate
compiler builtin records: only the canonical loaded Core module may declare them.
Their `Display` implementations use the canonical `display: fn(self) -> String`
contract: rendering borrows the value and keeps the exact text policy above.

Float math wrappers widen `F32` with `Float.from_f32`, not a typed literal.
Typed transforms retain phantom scalar/space parameters with Float storage;
their source affine constructor accepts the widened Float coefficients, like
`Point2.new` accepts Float coordinates.

#### Measurement propagation

A measured value stores a numeric value and a nonnegative standard uncertainty.
The measurement operations use **first-order linear propagation with
uncorrelated inputs**: addition and subtraction use root-sum-square
uncertainty, and multiplication, division, and square root use the
corresponding first-order derivatives. Exact inputs have zero uncertainty.
**Correlated errors are out of scope** (D-TYPE2-UNCERT1).

`Measurement<T: Debug>` is a source-owned generic record; its `Display`
implementation requires the observations to support Debug formatting.
Construct Float measurements with `measurement(value, uncertainty: sigma)`;
read them through `value()` and `uncertainty()`, not record fields or literals.
Arithmetic uses `+`, `-`, `*`, `/`, and `math.sqrt`; named arithmetic and
`sqrt()` methods are not alternate spellings. Exact scalar operands enter
these operations with zero uncertainty. Measured unit conversion first keeps
the scalar exact/rounded conversion's failure domain `E`, then constructs
uncertainty from the converted value and the relative scale uncertainty.
The source propagation helper is generic over `E`; it does not turn failures
into an invalid `String!` error contract. Ordinary
measurement Display uses Float Debug text (`1e-8`), while `units.show` retains
Float Display text.

#### Linear algebra

Fixed-size vectors and matrices use `Float` components. `Vec2`, `Vec3`, and
`Vec4` provide constructors, `splat`, `from_array`, `to_array`, checked lane
indexing, vector addition and subtraction, element-wise multiplication,
multiplication and division by a `Float` on either side, and the `dot`,
`length`, `length_sq`, `distance`, `distance_sq`, and `normalize` operations;
`Vec3` also has `cross`. `Mat3` and `Mat4` constructors
use column-major storage; they provide `from_array`, `to_array`, addition,
subtraction, matrix multiplication, `matmul`, `transform`, and `transpose`.
Reductions use a scalar left-to-right fold, so the reduction order is part of
the reproducible fixed-array contract (D-LINALG1; `to_array` is the fixed-array bridge, D-FIXARR1).
Arithmetic results are ordinary math values: lane reads, swizzles, and methods
operate directly on them, including temporary receivers such as `(a + a).dot(a)`.
See [`vector_result_values.jet`](../../../tests/run/UnsafeAndRuntime/VectorResultValues.jet).

Swizzles (D-SWIZZLE1) name lanes with `x`, `y`, `z`, and `w` on `Vec2`,
`Vec3`, `Vec4`, `F32x4`, and `F64x2`. A read of one lane is `lane`; a read of
N lanes is the declaration's `__swizzleN` and yields `Vec2`/`Vec3`/`Vec4`
(`F64x2` for an `F64x2` pair, `F32x4` for four `F32x4` lanes). A write
`v.zx = e` — on a local or any field path — is the declaration's
`__swizzle_setN` and takes the type the read yields; `v.zx += e` writes
`v.zx + e`. An `F32x4` write narrows each `Float` with `F32.from_float` and
stops when a value is outside the F32 range. A lane the type does not have is
E3110, and a write that names a lane twice (`v.xx = …`) is E3111. See
[`swizzle.jet`](../../../Examples/LowLevel/Swizzle.jet).

```jet
rotate: fn(v: Vec3, transform: Mat3) -> Vec3 {
    unit: v.normalize()
    return transform.transform(unit)
}
```

#### SIMD lanes

The SIMD-like lane types have fixed-array semantics independent of whether a
backend selects vector instructions. Floating types are `F32x4`, `F64x2`,
`F32x8`, and `F64x4`. Signed integer types are `I8x16`, `I16x8`, `I32x4`,
`I64x2`, `I8x32`, `I16x16`, `I32x8`, and `I64x4`; unsigned types use the
corresponding `U` prefixes. Each is a `core.math` struct over its `lanes`
fixed array, declared once in Core source, so every tier runs the same
implementation. Every lane type supports a positional constructor, `splat`,
`from_array`, `to_array`, checked indexing (`v[i]` or `v.lane(i)`; a literal
index outside the lanes is a compile error), and `+`, `-`, `*`, and `/` with
matching lane types (D-SIMD1/D-SIMD2/D-SIMD3).

Each lane operator is the scalar operator applied lane by lane, so integer
lanes overflow and divide (truncating toward zero) as their scalar type does. The reductions `sum`, `product`,
`min`, `max`, and `reduce(op)` with a `ReduceOp` (`.Add`, `.Mul`, `.Min`,
`.Max`, `.Avg`) are the scalar left-to-right fold over the lanes, so a float
reduction is bit-exact with writing `((l0 + l1) + l2) + l3`. Float `min` and
`max` skip NaN lanes as `min_float`/`max_float` do; `.Avg` is `sum` divided
by the lane count, an integer division for integer lanes. Operands of
different lane types are error E2511, and a `reduce` argument that is not a
`ReduceOp` value is error E2510.

### `core.units` — measured quantities

`core.units` supplies named SI and binary-unit scales over
`Measurement<Float>`. It is a scalar conversion layer rather than a
user-defined dimension system. The module-global scale names are uppercase
(`NANO`, `MICRO`, `MILLI`, `CENTI`, `KILO`, `MEGA`, `GIGA`, `METER`, `METRE`,
`GRAM`, `SECOND`, `BYTE_UNIT`, `KIBIBYTE`, `MEBIBYTE`, and `GIBIBYTE`).

```jet
use units: core.units

run: fn() {
    distance := units.meters(12.0)
    elapsed := units.seconds(3.0)
    print(units.show(distance))
    print(units.to_si(elapsed))
}
```

| Function | Returns | Description |
| --- | --- | --- |
| `units.from(magnitude, unit)` | `Measurement<Float>` | Jet-owned construction of SI value `magnitude * unit` with zero uncertainty. |
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
`RNG` values make a stream visible and reproducible. Use `core.crypto.random`
for security-sensitive bytes or secrets (D-DET1).

The ambient stream uses a Jet xorshift transition over a 64-bit state word;
an atomic compare/exchange preserves a single draw order inside a deterministic
world. Seeding and replay retain the same words and history as the native
stream. `core.math.random()` also consumes that stream and carries `Rand`;
it retains its 64-bit floating-point projection, while
`core.math.random.float()` uses an exactly representable 53-bit draw.

```jet
use random: core.math.random

roll_at: fn(rng: &RNG) -> String {
    value := rng.int(1, 6)
    return "roll=" + value.to_string()
}

run: fn() {
    rng := random.rng(42)
    print(roll_at(rng))
}
```

| Function or method | Returns | Description |
| --- | --- | --- |
| `random.seed(value)` | `Unit -[Rand]>` | Seed the ambient deterministic stream. |
| `random.rng(seed)` | `RNG` | Create an explicit deterministic stream. |
| `random.split(seed)` | `RNG -[Rand]>` | Derive a stream from the ambient stream and a seed. |
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
| `rng.split()` | `RNG` | Derive a child stream without sharing mutable stream state. |
| `random.randint`, `uniform`, `normalvariate`, `gauss`, `expovariate`, `randbytes`, `getrandbits`, `randrange`, `choice`, `choices`, `triangular`, `gammavariate`, `betavariate`, `lognormvariate`, `paretovariate`, `weibullvariate`, `vonmisesvariate`, `binomialvariate` | varies | Compatibility aliases and distributions exposed by the random module. |
An explicit `RNG` is a seeded capability: its draw methods advance that
capability without requiring the ambient `Rand` effect (D-DET-CAPAPI).
The seeded integer kernel takes and returns native signed words. Exact Jet
integers are range-checked once when entering that boundary and widened once
on return; a native word is never reinterpreted as a numeric handle.

### `core.math.combinatorics` — generation and counts

`core.math.combinatorics` builds full collections over any element type. Each
result is a fresh list that holds copies of the input items. The counts here
are the ones `core.math` does not already own. Factorials, choose counts and
arrangement counts live only in `core.math` (`math.factorial`,
`math.binomial`, `math.perm`). Sequence work that a list already does stays
a list method (`take_while`, `skip_while`, `filter`, `windows`, `chunks`,
`chunk_while`, `scan`, `map`, `skip`, `take`, `step_by`, `flatten`, `cycle`,
`repeat`, `reverse`).

| Function | Returns | Description |
| --- | --- | --- |
| `permutations(items: [T], k)` | `[[T]]` | Every ordered pick of `k` distinct positions; `k` outside `0..=len` gives an empty list. |
| `combinations(items: [T], k)` | `[[T]]` | Every unordered pick of `k` distinct positions, in input order. |
| `combinations_with_replacement(items: [T], k)` | `[[T]]` | Unordered picks that may reuse a position; a negative `k` gives an empty list. |
| `product(items: [T], repeat)` | `[[T]]` | The cartesian power; `repeat <= 0` gives one empty row. |
| `cartesian(left: [T], right: [T])` | `[[T]]` | Every `[a, b]` pair, left-major. |
| `powerset(items: [T])` | `[[T]]` | Every subset, starting with the empty one. |
| `chain(left: [T], right: [T])` | `[T]` | `left` followed by `right`. |
| `compress(items: [T], mask: [Bool])` | `[T]` | The items whose mask entry is `true`; a short mask drops the tail. |
| `unique(items: [T])` | `[T]` | The first occurrence of each equal item, in order; `T` is `Equatable`. |
| `tee(items: [T], copies)` | `[[T]]` | `copies` independent copies; a nonpositive count gives an empty list. |
| `zip_longest(left: [T], right: [T], fill: T)` | `[[T]]` | Pairs to the longer length, padding the shorter side with `fill`. |
| `count_from(start, step, n)` | `[Int]` | `n` values from `start` by `step`. |
| `multinomial(parts: [Int])` | `?Int` | The multinomial coefficient; a negative part returns `None`. |
| `rising_factorial(n, k)` | `?Int` | `n * (n + 1) * … * (n + k - 1)`; a negative `k` returns `None`. |

```jet
use combo: core.math.combinatorics

run: fn() {
    print(combo.permutations([String]{"a", "b", "c"}, 2))
    print(combo.cartesian([String]{"a", "b"}, [String]{"x", "y"}))
    print(combo.zip_longest([String]{"a", "b", "c"}, [String]{"x"}, "-"))
}
```

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

The independent finite `Solver` counter is not an LU factorization:
`Solver.new(seed)` starts with zero checks and failures, `solver.require(ok)`
counts every requirement and records failed ones, and `failure_count()` reads
the failure count. `status()` returns `"ok"` until a failure, then `"failed"`.
Copying this value copies its counters independently.

The supported strict reproducible profiles are `F64Strict+Reproducible` and
`F32Strict+Reproducible`. The solver is CPU-only and rejects tensors above the
configured element limit rather than silently moving them to another device.

### `core.game` — scene execution and replays

`core.game` provides a small scene runner with explicit scene, replay, and
backend values. A scene owns its name, frame limit, and tick callback; a replay
records a path; and the headless backend produces a deterministic frame
transcript without requiring a renderer or audio device.
`Scene`, `Replay`, and `Backend` are opaque compiler-owned handles. Their
registered constructors preserve runtime identity and state; they do not expose
source-record construction as an alternate scene, replay, or backend model.
Game-control callbacks and command relays use the shared native devtools
boundary; the checked command DTOs and queues remain a separate control layer.
Both Rust emitters project frame and input-snapshot records to the native Game
carriers. Frame registration and execution borrow the scene mutably; absent
replay/backend/frame arguments become native optional references, not references
to the language's optional-result carrier.

```jet
use game: core.game

run: fn() {
    scene := game.Scene.new("counter")
    scene.max_frames = 3
    scene.on_frame((frame) -> {
        print("frame=" + frame.to_string())
    })
    backend: game.Backend.headless()
    replay: game.Replay.record("build/counter.replay")
    receipt: game.run(scene, replay: replay, backend: backend, frames: 3)
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

### `core.perf` — fidelity scaling

`core.perf` maps a fidelity value to bounded work scaling. `fidelity()` reads
the environment-selected value; `default_fidelity()` is the pure value `1.0`.
`override_fidelity` and `reset_fidelity` modify the environment-scoped value.

| Function or type | Returns | Description |
| --- | --- | --- |
| `perf.fidelity()` | `Float -[Env]>` | Read the selected fidelity. |
| `perf.default_fidelity()` | `Float` | Return `1.0`. |
| `perf.override_fidelity(value)` | `PerfError! -[Env]>` | Set fidelity in the inclusive range `[0, 1]`; NaN and out-of-range values fail with `PerfError.OutOfRange(value)`. |
| `perf.reset_fidelity()` | `Never! -[Env]>` | Restore the default selection. |
| `perf.of(value)` | `Perf` | Construct a bounded fidelity value; NaN becomes `1`. |
| `perf.is_full(value)` / `perf.is_low(value)` | `Bool` | Test full fidelity or fidelity below `0.25`. |
| `perf.scale(value, work)` | `Int` | Scale nonnegative work by fidelity. |
| `Perf` | type | A bounded fidelity value consumed by scaling operations. |
| `PerfError` | `#Error` enum | D-PERF-ERR1=A. `OutOfRange(Float)` holds the rejected value. |

### `core.text` — UTF-8 text operations

`core.text` operates on UTF-8 strings. Scalar counts, trimming, case
conversion, and predicates use decoded Unicode scalars; named byte operations
use bytes. Jet uses its pinned Unicode 17.0.0 tables, not the host's Rust, OS,
locale, or terminal Unicode version. Empty strings are false for the
empty-sensitive predicates. `splitn` and `rsplitn` limit the number of
resulting parts (D-TEXTUNICODE1=A).

The checksum-verifying `Tools/agent/UnicodeTables.jet` owns the pinned
mapping and property assets, including the native runtime's fixed-word
lookup functions. Regenerate them from `tests/data/unicode/ucd`; never copy
host Unicode tables or hand-maintain a native whitespace/casing list.
Run it with `--check` to check every generated asset for exact byte identity;
the lander runs that check as a ratchet.

`String.equal` borrows both operands and uses the same native UTF-8 byte
comparison as `==`; it does not normalize or clone either string.

| Function | Returns | Description |
| --- | --- | --- |
| `text.byte_count(value)` | `Int` | Count UTF-8 bytes. |
| `text.scalar_count(value)` | `Int` | Count decoded Unicode scalars. |
| `text.is_ascii(value)` | `Bool` | Test whether every byte is ASCII. |
| `text.scalars(value)` | `[String]` | Return decoded Unicode scalars. |
| `text.lower(value)` / `upper(value)` / `casefold(value)` | `String` | Apply Unicode-aware scalar case mappings supported by the runtime. |
| `text.caseless_eq(a, b)` | `Bool` | Compare after case folding. |
| `text.collate(a, b)` | `Ordering Never!` | Compare in the Unicode default order (D-TEXT-COLLATE1=A): case and accents only break ties, so `apple` < `Banana` < `éclair` < `zebra` < `Zulu`. `Equal` only for canonically equivalent text. |
| `text.collation_key(value)` | `[U8] Never!` | A sort key whose byte order is `text.collate` order; build one key per item to sort a long list. |
| `text.nfc(value)` / `nfd(value)` / `nfkc(value)` / `nfkd(value)` | `String` | Apply the named normalization form. |
| `text.graphemes(value)` / `words(value)` / `sentences(value)` | `[String]` | Return grapheme segments or Unicode 17.0.0 UAX #29 words/sentences. Words keep only segments containing an Alphabetic or Numeric scalar; sentences trim Unicode White_Space and drop empty segments. |
| `text.char_indices(value)` | `[String]` | Return scalar strings with their UTF-8 byte indexes. |
| `text.display_width(value)` | `Int` | Compute terminal display width under the portable text-width policy (D-TEXTWIDTH1=B); ambiguous scalars are narrow. |
| `text.grapheme_views(value)` / `word_views(value)` / `line_views(value)` / `byte_views(value)` | `[String]` | Produce views or segments for the requested boundaries. |
| `text.is_alphabetic(value)` / `is_numeric(value)` / `is_whitespace(value)` | `Bool` | Test the corresponding scalar property. |
| `text.is_blank(value)` / `is_graphic(value)` / `is_punctuation(value)` | `Bool` | Unicode forms of C `iswblank`, `iswgraph` and `iswpunct`: every scalar is a tab or a space separator (Zs); every scalar is visible (not whitespace, a control or a separator); every scalar is graphic and neither a letter, a digit, a combining mark nor a format character, so symbols such as `$` and `€` count. Empty text is false. |
| `text.has_control(value)` | `Bool Never!` | True iff any scalar is Unicode 17 Cc (`U+0000..U+001F` or `U+007F..U+009F`); empty text is false. One UTF-8 byte scan stops at the first match without creating per-character strings or a decoded-scalar list. |
| `text.trim(value)` / `trim_start(value)` / `trim_end(value)` | `String` | Remove supported whitespace at both or one end. |
| `text.pad_start(value, width, fill)` / `pad_end(value, width, fill)` | `String` | Pad to a requested display width. |
| `text.center(value, width, fill)` | `String` | Center a value in a requested width. |
| `text.starts_any(value, prefixes)` / `ends_any(value, suffixes)` | `Bool` | Test any of several prefixes or suffixes. |
| `text.splitn(value, separator, limit)` / `rsplitn(value, separator, limit)` | `[String]` | Split from the left or right with a maximum part count. |
| `text.inspect(value)` | `[String]` | Return each scalar as `U+XXXX NAME`, using pinned Unicode names (Unicode 1.0 names for controls, `UNASSIGNED` where no name is recorded). |
| `text.Cursor` / `text.cursor(value)` / `text.cursor_advance(text, cursor)` | type / cursor | Construct and advance a text cursor. |

`text.collate` is the Unicode Collation Algorithm (UTS #10) with the CLDR root
table for Unicode 17.0.0, which is also English order. Variable weighting is
non-ignorable, the CLDR default, so punctuation and spaces sort as characters
before digits and letters. Levels 1 to 3 (base letter, accent, case) are
followed by an identical level on the NFD scalars. Implicit weights for Han,
Tangut, Nüshu, Khitan and unassigned code points are computed, not stored.
`Tools/agent/CollationTable.jet` generates `Core/Text/CollationTable.jet` from
the vendored, checksum-verified `tests/data/unicode/uca/allkeys_CLDR.txt`;
the table is linked only into programs that collate. There is no
locale-specific tailoring. String `<`, `<=>` and `sort()` keep byte order,
which is code point order and the order `LC_COLLATE=C` gives.

`String.len()` is the compiler-owned receiver-first method and counts Unicode
scalars, the items `loop c in s.chars()` visits (S41); it means the same with
or without `use core.text`. `count_bytes()` and `text.byte_count(value)` report
the UTF-8 byte length. The public convenience surface is the qualified
`core.text` module; the old catalogue of unqualified string methods is not a
separate core module contract (D-STR-DECLINE1=C).

`s.hash()` returns a `U64` hash of the UTF-8 bytes, read in place. Equal
strings hash equal within one process run (on every optimization tier and at
compile time), so the value can key a hash table. It is not cryptographic, and
the value may differ between runs and compiler versions: never persist it or
compare it across processes.

Shell-style line and field work is a set of String methods. They need no
import, and the line methods follow the `lines()` rule (LF ends a line, one
CR before it is dropped, and a final LF adds no empty line). All but `words`
are Jet bodies in `core.text.parse` (`has_line` in `core.regex`), which the
compiler loads for the spelling:

| Method | Returns | Shell | Meaning |
|--------|---------|-------|---------|
| `text.trim_newlines()` | `String` | `$(…)` | `text` without its trailing LFs; other trailing whitespace stays. |
| `text.words()` | `[String]` | `set -- $x` | The fields between runs of Unicode White_Space; none is empty. |
| `text.first_lines(n: Int)` | `[String]` | `head -n` | The first `n` lines; all of them when there are fewer, none when `n <= 0`. |
| `text.last_lines(n: Int)` | `[String]` | `tail -n` | The last `n` lines, by the same rule. |
| `text.has_line_prefix(prefix: String)` | `Bool` | `grep -q '^prefix'` | Whether some line starts with `prefix`, read literally. |
| `text.has_line(pattern: Regex)` | `Bool` | `grep -qE` | Whether some line matches `pattern`; `^` and `$` anchor at each line. |
| `text.tsv_records()` | `[[String]]` | `cut -f` | Each line split on every tab; empty fields stay, a line with no tab is one field. |

`has_line` takes a quoted pattern as a checked `Regex` literal:
`text.has_line("^(a|b)")` checks the pattern when the program compiles, so
the call has no compile, flags or failure step, and a bad pattern is E0152 at
the call. A `Regex` value built at run time is passed as is.

### `core.time` — dates, durations, and clocks

`core.time` uses the proleptic Gregorian calendar, including year `0` (1 BCE),
and integer Unix conversion formulas. Date arithmetic and numeric-offset
parsing are pure. `zone` and `parse_zoned` carry `Env` and `FS.Read` because
named-zone lookup consults environment variables and TZif files, even when a
particular call uses UTC or bundled web data. `now`, `now_utc`, `today`,
`instant`, `start`, `sleep`, and `sleep_until` carry `Time`.

The civil carriers (`Date`/`LocalDate`, `LocalTime`, `DateTime`, `Period`,
`Zone`, `ZonedDateTime`, and `Instant`) and their arithmetic, parsing,
formatting, and text codecs are implemented in `Core/Time/Time.jet` on all
tiers. `Date` is the prelude's name for `LocalDate`
(`pub use Date: core.time.LocalDate`); `core.time` declares only
`LocalDate`. Immutable zone tables are plain values,
copied with their zone, and borrowed reads need no shared-memory guard or
`Time.Wait` effect. Native seams supply raw clock samples, environment values
and TZif bytes. `sleep` resolves to its loaded source declaration before
compiler import rows and delegates to the shared scheduler,
including its deadline, shielding, cancellation, and timer policy; it is not a
separate host wait. Zero and negative durations still check the wait boundary
(their parked duration is zero), as do already-elapsed `sleep_until` deadlines.
Wall-clock selection prefers a deterministic-world sample, then a replay
override, then the raw host clock; calendar policy does not live in those
adapters. Browser hosts supply bundled TZif bytes and report environment and
filesystem samples as absent.

Core's reserved `__` seams are loaded only through the trusted embedded-Core
loader. To check this implementation, check a program importing `core.time`,
not `Core/Time/Time.jet` as a user entry: user-written reserved names still
produce E0067.

Codec compatibility is intentionally separate from strict public parsing:
the `LocalTime` wire format drops fractional seconds, and the `DateTime`
wire decoder retains its permissive fraction policy. Civil `.with` and zoned
`.with_time` return `TimeError`, as do public parsing and local-zone
constructors. Inspect `error.reason` for validation or disambiguation details;
`.with_time` preserves the error returned by `zoned_local`.
Date `.until` and `.since` round their exact day-to-nanosecond intermediate
before saturating the final `Duration` to its signed nanosecond range.

`zone` accepts UTC/GMT/Z and numeric offsets; the runtime host also resolves
named IANA zones from TZif data. On native hosts, lookup checks
`JET_TZDB_DIR`, `TZDIR`, `$JET_ROOT/Core/Time/tzdb`, the repository's
`Core/Time/tzdb`, `/usr/share/zoneinfo`, `/usr/share/lib/zoneinfo`, and
`/etc/zoneinfo`. Optional `posix/` and `right/` prefixes are stripped for
lookup. Web/WASM uses its embedded zone data. A name with `..`, a leading
`/`, or a leading `\` is rejected; an unavailable name returns `TimeError`
with an `unknown IANA time zone` message that points to `JET_TZDB_DIR` or
`TZDIR` (D-FREESTAND-TIME1). A version 2 or later TZif file's footer holds a
POSIX TZ rule such as `EST5EDT,M3.2.0,M11.1.0`; offsets, transitions, gaps and
overlaps past the table's last transition follow that rule. A malformed footer
returns `TimeError`.

```jet
use time: core.time

run: fn() {
    stamp: time.parse_rfc3339("2025-01-02T03:04:05Z") ?? return
    print(time.isoformat(stamp))
    clock: time.Clock.new(0)
    clock.tick(1000)
    print(clock.now())
}
```

| Function or type | Returns | Description |
| --- | --- | --- |
| `time.is_leap_year(year)` | `Bool` | Test leap-year status under the proleptic Gregorian calendar. |
| `time.days_in_month(year, month)` | `Int` | Return the number of days; an invalid month retains the civil calendar's `30`-day default. |
| `time.period(start, end)` / `period_days(n)` / `period_months(n)` / `period_years(n)` | `Period` | Construct calendar periods. |
| `time.utc()` / `time.zone(name)` | `Zone` / `Zone !TimeError -[Env, FS.Read]>` | Construct UTC, a numeric-offset zone, or a named IANA TZif zone. |
| `time.utcoffset(zone)` | `Int` | Return the zone offset represented by the zone. |
| `time.gmtime(seconds)` / `time.ctime(seconds)` / `time.asctime(date_time)` | date or `String` | Convert Unix time or format a date. |
| `time.time(hour, minute, second)` / `time.local_time(hour, minute, second)` | `LocalTime` | Construct a local time; seconds through `60` can represent an input leap-second shape. |
| `time.new(year, month, day)` | `LocalDate` | Construct a date, clamping month and day to the accepted calendar ranges. |
| `time.datetime(year, month, day, hour, minute, second)` | `DateTime` | Construct a date-time value. |
| `time.from_timestamp(seconds)` / `from_unix_seconds(seconds)` / `from_unix_ms(ms)` / `from_unix_microseconds(us)` / `from_unix_nanoseconds(ns)` | `DateTime` | Convert Unix-based values at the named precision. |
| `time.from_iso_week(year, week, weekday)` / `parse_iso_week_date(text)` | `LocalDate !TimeError` | Convert or parse an ISO week date. |
| `time.parse(text)` / `parse_time(text)` | `LocalDate !TimeError` / `LocalTime !TimeError` | Parse supported date or time text, including the documented leap-second shape. |
| `time.parse_rfc3339(text)` | `DateTime !TimeError` | Parse an RFC3339 timestamp with a numeric UTC offset without named-zone lookup. |
| `time.parse_zoned(text)` | `ZonedDateTime !TimeError -[Env, FS.Read]>` | Parse an RFC9557 date-time with a bracketed IANA zone and a matching UTC offset. |
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
| `clock.wait(duration)` | `Int` | Advance by whole duration milliseconds and return the new value; this does not park a task. |
| `Duration.nanoseconds(n)` / `microseconds(n)` / `milliseconds(n)` / `seconds(n)` / `minutes(n)` / `hours(n)` | `Duration !RangeError` | Construct a checked duration from the named unit. |
| `duration.in(unit)` / `total_in(unit)` | `Int !RangeError` / `Float` | Convert a duration to a named unit. |
| `duration.is_zero()` / `total_seconds()` / `difference(other)` / `round(unit)` / `abs()` / `negated()` / `sign()` | varies | Inspect or transform a duration. |

Ordinary `Clock` copies fork independent timelines, including copies inside
lists and other aggregates. A system-clock copy rebases its own monotonic
start at the original clock's current value. Internal expiry observers are
different: they retain the caller's original clock, so later ticks on that
clock affect its attached secret's lifetime. Clock Show and Debug retain the
`Clock { now: N }` rendering. Manual tick/wait arithmetic saturates at `Int`'s
millisecond bounds.

The source system-clock state uses labeled `started_ns` and `offset_ms`
fields. Its reserved monotonic sample remains the pure Clock seam; it does
not change `clock.now()` into an effectful host-clock API.
`time.clock(seed)` stays retired (E1004); use the type-owned `Clock.new(seed)` constructor.

`ExpiringValue.new(value, ttl, clock)` computes a saturating absolute
millisecond deadline in Jet. `get(clock)` returns a copy of the payload until
`now > deadline` (equality is still live), otherwise `Expired`. Zero and
negative TTLs retain this same boundary. `is_valid(clock)` observes the same
clock explicitly and returns `now <= deadline`, including a saturated
deadline. Show retains the existing
`Expiring(deadline=N)` rendering; Debug remains unsupported (E0112), not a
structural payload renderer. Its fields are private, with
`expires_at_ms()` for the deadline and fallible `get(clock)` for payload
access: this is Main's owner-approved, veto-able private-field correction,
following the Measurement/LogSpan rule. The retired module constructor and
`force` diagnostics are unchanged.

`ExpiringValue` is a reserved source-owned nominal declared in
`Core/Time/Time.jet`. The canonical Core ownership inventory admits that
declaration and its impl even though the spelling remains reserved for user
code (E0106 for redeclarations, E0301 for user impls on the builtin spelling).
Source ownership is distinct from public export metadata and also applies
to private Core declarations; it never grants ownership to a user file.

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

`ZonedDateTime` equality compares the complete instant and the zone name, not
the zone's transition-table identity or contents. The name is immutable
metadata outside the shared transition table, so this comparison does not
acquire a shared guard or widen the `Equatable` effect contract.

Duration literals use the supported `ns`, `us`, `ms`, `s`, `min`, `h`, and `d`
suffixes. A runtime `Duration` carries checked nanoseconds; a deterministic
function should receive a `Clock` explicitly rather than reading `time.now()`
internally. The manual clock then makes time an ordinary input that can be
replayed (D-TIMERES1=A; D-DET-CAPAPI).

### `core.time.calendar` — calendar formatting

`core.time.calendar` provides fixed English calendar names and Monday-based
week calculations without locale tables. Its module-global weekday constants
are `MONDAY: 0` through `SUNDAY: 6`.

All calendar behavior is implemented in the embedded `Core/Time/Calendar.jet`
source shared by the execution tiers, including epoch conversion without a
native date-time carrier.

| Function or value | Returns | Description |
| --- | --- | --- |
| `calendar.isleap(year)` | `Bool` | Test leap-year status. |
| `calendar.leapdays(y1, y2)` | `Int` | Count leap days between the smaller and larger endpoint, excluding the larger year. |
| `calendar.weekday(year, month, day)` | `Int` | Return Monday `0` through Sunday `6`. |
| `calendar.monthrange(year, month)` | `(days: Int, weekday: Int)` | Return the day count and Monday-based first weekday. Invalid months retain the `30`-day default. |
| `calendar.monthcalendar(year, month)` / `monthcalendar_start(year, month, firstweekday)` | `[[Int]]` | Build a month grid. |
| `calendar.yearcalendar(year)` | `[[[Int]]]` | Build a year calendar. |
| `calendar.day_name(index)` / `day_abbr(index)` | `String` | Return fixed English day names. |
| `calendar.month_name(index)` / `month_abbr(index)` | `String` | Return fixed English month names. |
| `calendar.weekheader(width, firstweekday)` | `[String]` | Clip abbreviated weekday names or pad them on the right; widths below `1` use `1`. |
| `calendar.formatmonth(year, month, width)` / `formatyear(year)` | `String` | Render a month or year calendar. |
| `calendar.timegm(year, month, day, hour, minute, second)` | `Int` | Convert a UTC calendar tuple to Unix seconds, constraining civil fields and preserving a `23:59:60` leap label as the next epoch second. |

### `core.encoding` — bounded value codecs

`core.encoding` converts complete values to and from `Dyn`. Objects retain
arrival order: the `Object` payload is the ordered pair list
`[(key: String, value: Dyn)]` (D-DATATREE-ORDER1), and a repeated name is a
`Syntax` error except under `json.parse_allow_duplicates`. Integers and
floating-point numbers remain distinct, and
malformed input returns a typed `EncodingError`. The whole-value codecs are
pure; readers and writers expose the same boundaries incrementally. The
shared codec shape and value tree follow D-ENC1, D-JSONVERB1, D-SERDE13, and
D-DATATREE-ORDER1.

The reason-only `HexError`, `base32.Base32Error`, `base64.Base64Error`, and
`core.crypto.uuid.UUIDError` render only their `reason` in Display and Debug
formatting. `encoding.HexError` and `encoding.hex.HexError` name the same type.
`EncodingError.Display` is registered from its Core source declaration, not a
second synthetic implementation. Its byte, line, column, path, and reason
projection is the same on every execution tier.


| Function or type | Returns | Description |
| --- | --- | --- |
| `encoding.Dyn` | type | Dynamic tree with `Null`, `Bool`, owned exact `Int`, `Float`, lexical `Number`, `Text`, `Bytes`, `DateTime`, `Array`, and ordered `Object` cases. |
| `encoding.DataEvent` | type | Event emitted by an incremental reader. |
| `encoding.EncodingCause`, `EncodingErrorKind`, `EncodingError` | types | Structured encoding failure information. |
| `encoding.EncodingFormat` | type | Format selector used by generic readers and writers. |
| `encoding.EncodingLimits` | type | Depth, item-size, and format limits. |
| `encoding.Reader` / `encoding.Writer` | types | Generic incremental reader and writer interfaces. |
| `encoding.bytes_to_hex(bytes)` / `hex_to_bytes(text)` | `String` / `[U8] !HexError` | Convert bytes to or from lowercase hexadecimal. |
| `encoding.hex_nibble(value)` / `hex_value(value)` | `U8` / `?Int` | Encode a nibble or look up a hexadecimal value. |
| `encoding.wrap32(value)` | `Int` | Wrap an integer to 32 bits. |

`DataEvent.Number(text)` carries the exact JSON numeric lexeme, including
large exact integers and numbers not representable by the scalar event
carriers. User code can construct it and match `.Number(text)` with a `String`
payload on every tier. JSON and CBOR event writers retain their `Unsupported`
rejection of `Number` events (valid or malformed lexemes), before emitting
payload bytes; applications decode the lexeme to a supported scalar first.

The standard formats are exposed as `core.encoding.base32`, `base64`, `binary`,
`cbor`, `csv`, `hex`, `ini`, `json`, `jsonl`, `toml`, `xml`, and `yaml`.

#### Typed codecs and validation

A parser returns a `Dyn` (or a format-specific document). A typed codec
maps a declared Jet type to and from that tree. `#Codable`, `#Encode`, and
`#Decode` derive the typed encode/decode routes; field attributes such as
`#Rename`, `#Skip`, `#Flatten`, `#DenyUnknownFields`, `#Discriminant`, and
`#Untagged` describe the wire shape; `#Rename(.Camel)` on a type sets the
casing of every field or variant. Unknown fields are
ignored unless a type opts into rejection. A field conversion or missing
required field is reported as a `FieldError` rather than silently defaulting to
an unrelated type.
Text never implicitly decodes as an integer, float, or Boolean, except in
text-native sources: environment variables, CSV cells, route and query
parameters. `Dyn.Int` owns its exact integer even beyond machine range.
`Dyn.Number` retains a parsed number's spelling; `.int()`, `.float()` and
typed decoding check the destination range when reading it. There is no
public typed-text carrier.
`#Codable` requests both directions; `#Encode` and `#Decode` request one-way derivation, and derivation is compiler-owned rather than macro- or reflection-driven (`D-SERDE1–8`). Typed `decode<T>` produces `T ![FieldError]` (or `[T] ![FieldError]` for CSV); each failure preserves its path and reason, and generic codec bounds are injected at the use site (`D-GENERIC-CALL1`; `D-SERDE6`; `D-SERDE9–12`). Hand-written codecs use the same `Encode`/`Decode` protocol and field-path accumulation (`D-SERDE2`; `D-SERDE13–16`).

Validation can be declared as `validate { check(...) }`: all failed checks accumulate, and conditions and messages are purity-checked (`D-VALIDATE1`; `D-FIELDPOL1`; `S60/E3401`).
The use-site escape is the bare Core builder `Validate.over(value)` (declared in `core.encoding.validate`, which the bare name loads on demand): `.check(condition, at: field, "message")` takes the same rule shape, where `at:` must name a field of the validated type (E0353/E0354). `.finish()` returns `T ![FieldError]` with every failed rule (`D-VALIDATE1`).

The concrete format sources live under
[`Core/Encoding/`](../../../Core/Encoding/), and typed CSV/JSON convenience
functions are also exported by [`Core/Data/Data.jet`](../../../Core/Data/Data.jet).
A decoder is only type-safe when its destination has an explicit shape; do not
use an untyped string round trip as a substitute for a typed codec.

### `core.encoding.json` — JSON trees

`core.encoding.json` parses strict RFC 8259-style JSON into `Dyn`. Every
number token remains a `Number` with its exact spelling, including trailing
zeroes, exponent spelling, and numbers outside machine range. NaN and infinity
syntax is rejected. Typed `decode<T>` checks numbers when projecting into `T`;
quoted numbers stay text. Compact output retains insertion order, pretty output
uses two-space indentation, and `canonical` is RFC 8785 JCS with UTF-16 key
order. `parse`, `decode`, and `reader` reject a repeated object name with its
path and position; `parse_allow_duplicates` is the explicit opt-in.
The reader enforces depth `256` and a maximum item size of `16,777,216` bytes.

| Function or type | Returns | Description |
| --- | --- | --- |
| `json.parse(text)` / `decode(text)` | `Dyn !EncodingError` | Parse one strict JSON value. |
| `json.loads(text)` / `load(text)` | `Dyn !EncodingError` | Compatibility aliases for parsing text. |
| `json.dumps(value)` / `dump(value)` / `to_string(value)` | `String !EncodingError` | Emit compact JSON; a non-finite `Float` is an `EncodingError`. |
| `json.to_string_pretty(value)` | `String !EncodingError` | Emit two-space pretty JSON; a non-finite `Float` is an `EncodingError`. |
| `json.canonical(value)` | `String !EncodingError` | Emit RFC 8785 JCS bytes: UTF-16 key order, ECMAScript numbers; reject NaN, infinities, and an `Int` that binary64 cannot hold exactly. |
| `json.parse_allow_duplicates(text)` | `Dyn !EncodingError` | Parse like `parse`, but keep the later value of a repeated object name. |
| `json.pointer(tree, path)` | `Dyn !EncodingError` | Select by RFC 6901 JSON Pointer; a missing member or bad step is a located error, JSON `null` is a value. |
| `json.patch(doc, ops)` / `patch_with_limits(doc, ops, limits)` | `Dyn !EncodingError` | Apply an RFC 6902 operation array atomically; `doc` is unchanged on failure. |
| `json.events(value)` | `[DataEvent]` | Produce the canonical event sequence for a tree. |
| `json.reader(input, limits)` / `reader_allow_duplicates(input, limits)` | `JSONReader !EncodingError` | Construct a bounded file-backed event stream. `reader` fails with a sticky `Syntax` error at a repeated object name; `reader_allow_duplicates` reports each occurrence as its own `Key` event. |
| `json.writer(output, limits, canonical)` | `JSONWriter !EncodingError` | Construct a bounded file-backed event writer. |
| `JSONReader.next()` | `?DataEvent !EncodingError` | Return the next event or stable end-of-input. |
| `JSONWriter.write(event)` / `flush()` / `finish()` | `Unit !EncodingError` | Write events, flush output, or validate and finish the stream. |

Typed `core.data.json<T>` reuses the source JSON parser while retaining exact
number lexemes until `Decode<T>` selects the destination representation. Its
parser entry is compiler-private, not an application-visible JSON function.

### `core.encoding.jsonl` — line-delimited JSON

`core.encoding.jsonl` parses one JSON value per nonblank line. Blank lines are
skipped, each nonblank line is parsed independently, and one malformed line
returns an `EncodingError`. Compact output writes one value and a trailing
newline per row.
Records end at LF; a CR immediately before LF is stripped. A final bare CR
is retained and counts as nonblank (only spaces and tabs make a blank record).

| Function or type | Returns | Description |
| --- | --- | --- |
| `jsonl.parse(text)` / `loads(text)` | `[Dyn] !EncodingError` | Parse all nonblank lines. |
| `jsonl.to_string(values)` / `dumps(values)` | `String` | Encode values as compact JSON lines with a trailing newline per row. |
| `jsonl.count_rows(text)` | `Int` | Count nonblank rows. |
| `jsonl.append_line(text, value)` | `String` | Append one encoded row. |
| `jsonl.first(text)` | `?Dyn !EncodingError` | Return the first nonblank row. |
| `jsonl.reader(input, limits)` / `jsonl.writer(output, limits)` | `JSONLReader !EncodingError` / `JSONLWriter !EncodingError` | Construct bounded file-backed line streams. |
| `JSONLReader.next()` | `?Dyn !EncodingError` | Return the next row or stable end-of-input. |
| `JSONLWriter.write(value)` / `flush()` / `finish()` | `Unit !EncodingError` | Write records, flush output, or finish the stream. |

### `core.encoding.csv` — comma-separated rows

`core.encoding.csv` exposes physical CSV rows and a tree representation. A
`CSVRow` records its physical line and fields. `csv.rows` splits on
`delimiter` (default `","`; one character other than a quote or a line
ending, otherwise an `Unsupported` error), drops the first record when
`header` is true, and keeps it when `header` is false; `skip_blank` only skips
blank physical rows (D-ROWS1).
The filesystem query helper decodes header-named records into a typed query;
it is distinct from the physical row parser.
The public file-backed `CSVReader` and `CSVWriter` are ordinary source-owned
holders, not native resource-table handles. The typed `core.data.csv_reader<T>`
is Core source over the same `CSVReader`: each `next` reads one record (the
header on the first pull) and decodes it, so rows stream one at a time.

| Function or type | Returns | Description |
| --- | --- | --- |
| `csv.rows(text, delimiter, header, skip_blank)` | `[CSVRow] !EncodingError` | Parse physical rows with explicit delimiter, header and blank-line policy. |
| `csv.parse(text, delimiter, header, skip_blank)` / `decode(text)` | `Dyn !EncodingError` | Parse rows as an array of arrays through the same engine and options as `csv.rows`. |
| `csv.to_string<T: Encode>(rows)` | `String` | Encode explicit row arrays, typed records, or the array-of-arrays `Dyn` that `csv.parse` returns, so parsed rows round-trip; records use the first record's field order as the header. Rows are separated by LF, with no trailing LF. |
| `csv.reader(source, limits, delimiter, header, skip_blank)` / `csv.writer(target, limits)` | `CSVReader !EncodingError -[FS]>` / `CSVWriter !EncodingError -[FS]>` | Construct incremental file-backed CSV objects. Reader defaults retain the header and blank records. |
| `CSVReader.next()` | `?CSVRow !EncodingError -[IO]>` | Return the next physical row or stable end-of-input; errors retain their byte offset, physical location, and record/field path. |
| `CSVWriter.write(fields)` / `flush()` / `finish()` | `Unit !EncodingError -[IO]>` | Write comma-delimited string fields; CRLF is deferred until the next record or finish. Flush does not finish a record. |
| `csv.query<T>(path, sql)` | `Query<T> ![FieldError] -[FS]>` | Decode typed header-named records and apply the bounded query plan. |
| `CSVRow` | type | Physical line number and parsed string fields. |

### `core.encoding.toml` — TOML values

`core.encoding.toml` supports the common TOML 1.0 document forms: comments,
dotted keys, tables, array tables, strings, integers, floats, booleans,
inline tables, and arrays. Numeric spellings remain `Dyn.Number`; duplicate
keys (including inline tables) are errors. Offset datetimes become `Dyn.DateTime`;
local dates and times remain text. Unsupported syntax fails closed, and output
is minimal rather than comment-preserving.

| Function | Returns | Description |
| --- | --- | --- |
| `toml.parse(text)` / `decode(text)` / `loads(text)` / `load(text)` | `Dyn !EncodingError` | Parse a TOML document or a compatibility alias. |
| `toml.to_string(value)` | `String` | Emit a minimal TOML document. |

### `core.encoding.yaml` — bounded YAML input

`core.encoding.yaml` accepts JSON-compatible YAML 1.2 plus simple block maps,
lists, comments, and `|` or `>` block scalars. Unknown or unsupported syntax
fails closed. This module is an input parser and does not expose a YAML
serializer.

| Function | Returns | Description |
| --- | --- | --- |
| `yaml.parse(text)` / `decode(text)` | `Dyn !EncodingError` | Parse supported YAML input. |

### `core.encoding.xml` — bounded XML trees

`core.encoding.xml` uses the lossless namespace-aware `Dyn` schema and
security rules in [encoding-decisions.md](../encoding-decisions.md#xml-representation-and-security).
Native authority preserves the complete parse/render options, entity policies,
lexical evidence, encodings, canonical modes, and located `XMLError` payloads.
The native CBOR and XML options/error records retain checked owner rows
even when no source module declares them. Their field shapes come from the
same checked Core field registry used by type checking; removing a source
codec must not remove its native options or located error payloads.

| Signature | Result | Description |
| --- | --- | --- |
| `xml.parse(text)` / `parse_with(text, options)` / `parse_bytes(bytes, options)` | `Dyn !XMLError` | Parse Unicode text or supported byte encodings with bounded, non-I/O entity policy. |
| `xml.to_string(tree)` / `to_bytes(tree, options)` | `String !XMLError` / `[U8] !XMLError` | Render a validated XML document with lexical and encoding options. |
| `xml.canonical(tree, options)` | `String !XMLError` | Apply Inclusive 1.1 or Exclusive 1.0 canonicalization. |
| `xml.reader(input, limits, options)` | `XMLReader !EncodingError` | Construct a bounded file-backed XML event reader. |
| `xml.attribute(element, name)` / `content(element)` / `expanded_name(node)` / `root(document)` | varies `!XMLError` | Inspect attributes, content, expanded names, and the document root. |
| `XMLWriter.write(event)` / `flush()` / `finish()` | `Unit !EncodingError` | Write XML events, flush output, or validate and finish the stream. |

XML rendering is fallible at both runtime and compile time. Invalid document
trees return the located `XMLError`; callers must handle it explicitly rather
than relying on an empty-string fallback. Compile-time code that stops on the
error is reported by the ordinary compile-time-stop diagnostic.

### `core.encoding.cbor` — bounded CBOR

`core.encoding.cbor` handles RFC 8949 integer, byte string, text, array,
map, simple, and float16/32/64 values, including indefinite strings and
containers. A byte string is `Dyn.Bytes`. Integers are exact `Dyn.Int`
values: major types 0 and 1 reach -2^64 and 2^64-1, and a wider value is a
bignum (tag 2, or tag 3 below zero, over its big-endian magnitude), which
`parse` decodes to the same `Int` case and `to_bytes` writes. The streaming
writer carries no tags, so it refuses an `Int` past major types 0 and 1.
Tags other than the date-time tags 0 and 1 (which decode to `DateTime`) and
the bignum tags, and duplicate text map keys, are unsupported. Canonical
mode requires shortest arguments and float widths, a canonical NaN, definite
containers, and bytewise map-key order. Encoding uses the shortest exact
float width.
Whole-value parsing and preferred float-width checks share a sealed source
part; its helper functions are not exports of the public CBOR module.

| Function or type | Returns | Description |
| --- | --- | --- |
| `cbor.parse(bytes, options)` | `Dyn !CBORError` | Parse a whole value under depth, item, allocation, and canonical limits. |
| `cbor.to_bytes<T: Encode>(value)` / `to_bytes_canonical(value)` | `[U8] !CBORError` | Encode ordinary or canonical CBOR. |
| `cbor.reader(input, limits)` / `writer(output, limits)` | `CBORReader !EncodingError` / `CBORWriter !EncodingError` | Construct bounded file-backed event streams. |
| `CBORReader.next()` | `?DataEvent !EncodingError` | Return the next event or stable end-of-input. |
| `CBORWriter.write(event)` / `flush()` / `finish()` | `Unit !EncodingError` | Write events, flush output, or validate and finish the stream. |

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
Edition 2027 rejects whitespace, mixed alphabets, invalid padding, and
nonzero unused bits, except that the explicitly line-wrapped decoder accepts
ASCII whitespace. Edition 2026 retains the historical decoder acceptance
rules; the caller's resolved package edition selects the policy.

| Function | Returns | Description |
| --- | --- | --- |
| `base64.encode(bytes)` / `decode(text)` | `String` / `[U8] !Base64Error` | Encode or decode standard padded base64. |
| `base64.encode_url(bytes)` / `decode_url(text)` | `String` / `[U8] !Base64Error` | Encode or decode URL-safe unpadded base64. |
| `base64.b64encode` / `b64decode` / `standard_b64encode` / `standard_b64decode` | varies | Compatibility aliases for standard operations. |
| `base64.encodebytes` / `decodebytes` / `b2a_base64` / `a2b_base64` | varies | Byte-wrapped or line-wrapped base64 forms. |
| `base64.l64a(value: Int) -> String !Base64Error` / `a64l(text: String) -> Int !Base64Error` | text / integer | POSIX radix-64 text of a 32-bit value (C `l64a` and `a64l`): at most six characters from `./0-9A-Za-z`, least significant digit first, and 0 is empty text. A value outside 0 through 4294967295, a seventh character or a character outside the alphabet is an error. |

### `core.encoding.binary` — checked binary packing

`core.encoding.binary` packs and unpacks fixed-width values from byte buffers.
Its format grammar accepts optional `<` little-endian or `>`/`!` big-endian
prefixes, repeat counts, and the `x`, `c`, `b`, `B`, `h`, `H`, `i`, `l`, `I`,
`L`, `q`, and `Q` integer codes. Native-alignment `@` is not part of this
portable grammar. The source is
[`Core/Encoding/Binary.jet`](../../../Core/Encoding/Binary.jet).

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
| `byteswap(data: [U8], width: Int) -> [U8] !BinaryError` | bytes | Reverse the byte order of each `width`-byte item; width 2 is C `swab`. A width other than 2, 4 or 8 is `Range`; a length that is not a multiple of the width is `Buffer`. |

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

The data error, summary, limit, and status declarations are source-owned Jet
values rather than native nominal substitutes. `DataError` displays its kind
and operation, then any row, column, and index in that order, followed by the
reason; its optional cause is retained but not included in that text.
`DataWatchStatus` exposes the complete retained status record in a stable field
order. `Dyn` remains the existing native nominal and ordered-object
carrier; source-owned data policy does not introduce a second tree algebra.
The source plot selector enums preserve the native variant order and spellings,
including text/detail channels, point scales, row/column facets, zoom/brush
interactions, terminal/browser/native/export backends, and supported/degraded
capabilities. The default `show` backend is Browser; Terminal selects text.

```jet
use data: core.data

#Codable
Ticket: struct {
    team: String
    minutes: Float
}

run: fn() {
    rows: data.csv<Ticket>("team,minutes\nCore,4.0\nCore,8.0\nTools,5.0") ?? return
    counts: data.query(rows)
        .group_by(t -> t.team)
        .count()
        .collect() ?? return
    chart: data.bar_text(counts) ?? return
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
| `quantile(values, q)` | `Float !DataError` | Compute a bounded order statistic by worst-case linear rank selection on a copy; the caller's list is unchanged. `quantile(values, 0.5)` is the median with this error contract; `core.math.stats.median` returns `Float?`. |
| `rolling_mean(values, window)` | `[Float] !DataError` | Compute a bounded rolling mean. |
| `query(rows)` | `Query<T>` | Start a deterministic query plan under `DataLimits.safe()`. |
| `query(stream)` | `Query<T>` | Start a one-shot query over a stream's remaining rows, under the `DataLimits` of the reader that made the stream. |
| `inner_join(left, right, left -> key, right -> key)` | `[DataJoin<T, U>] !DataError` | Preserve duplicate matches. |
| `left_join(left, right, left -> key, right -> key)` | `[DataJoin<T, ?U>] !DataError` | Retain unmatched left rows as `?U`. |
| `pivot_sum(rows, row -> key, row -> column, row -> value)` | `[DataPivotCell] !DataError` | Group and sum by a row and column key. |
| `plot<T>(rows)` | `JetDataPlot<T> !DataError` | Build a bounded plot plan from encodable rows. |
| `inspect(plot)` / `inspect_json(plot)` | inspection / `String !DataError` | Inspect a plot plan as text or JSON. |
| `text(plot)` / `svg(plot)` | `String !DataError` | Render plot text or SVG. |
| `show(plot)` / `render(plot, backend)` | `JetDataPlotRender !DataError` | Render through the selected backend. |
| `bar_text(groups)` / `bar_svg(groups)` | `String !DataError` | Render `Group<String, Int>` bars as text or SVG. |
| `line_text(groups, options)` / `line_svg(groups, options)` | `String !DataError` | Render `Group<String, Float>` lines with `DataLineOptions`. |
| `status()` | `[DataStatus] -[FS, Env]` | Report native and bridge path, ownership, trust, fallback, and replacement facts. Live R availability reads `PATH` and `JET_DATA_R_BRIDGE`, then checks files. |
| `require_bridge(provider)` | `Unit !DataError -[FS, Env]` | Fail closed when a requested `py`, `r`, or `gpu` bridge is unavailable; use the same live availability facts as `status`. |

Query methods filter, sort, map, extrema, joins, grouping, collection, and
watching while retaining typed positions for errors. `Query<T>.plan()` returns
deterministic operation names; `collect()` materializes `[T] !DataError`.
The source `query(rows)` facade constructs the shared deferred carrier through
the data provider, rather than rebuilding its private storage as a source
record. Filter, sort, and grouping selectors receive an owned row value at
the provider callback boundary. A query's limits bound its sorts, groups, and
collected rows, and every query derived from it keeps them: grouping a
`csv_reader` stream with `max_groups = 1` into two groups fails with
`Limit query.group_by.mean: max_groups 1 exceeded`.
Grouped `count`, `sum`, and `mean` return `Query<Group<K, V>>`, and `mean`
requires a floating numeric selector. `DataTracked<T, K>` supports `query`,
`insert`, `replace`, and `remove`; `DataWatch<T>` supports `get`, `status`, and
`cancel` (D-QUERY-RETAIN1).
The bounded SQL kernel accepts `SELECT *` or explicit fields, the aggregates
`COUNT`, `SUM`, `AVG`, `MIN`, and `MAX`, and optional `WHERE`, `GROUP BY`,
`ORDER BY`, and `LIMIT` clauses. It validates projected and grouped fields
against the inferred schema before materializing a result (D-SQL-SURFACE1).
Both typed list queries and CSV queries receive the complete checked `SQL`
carrier. A scalar hole in the bounded `WHERE` value is bound after parsing;
quoted text stays a value and cannot become query syntax.
`SUM` retains an integer result only when its wide accumulated value fits
`I64`; an out-of-range sum uses the accumulated floating result rather than
propagating a numeric-conversion failure from the aggregate.

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

### Event-time windows over `Stream<T>`

Three receiver stages on the builtin pull stream group events into tumbling
event-time windows. They are `core.data` source stages
([`Core/Data/Data.jet`](../../../Core/Data/Data.jet)). Each stage consumes
its receiver and stays lazy: the stream runs the callbacks once per element as
it is pulled. The callbacks take one element and carry no effects.

| Stage | Returns | Description |
| --- | --- | --- |
| `stream.with_event_time(event -> time)` | `StreamEventTime<T>` | Stamp each element with its event time: `Int` Unix seconds or a `DateTime`. |
| `stamped.key_by(event -> key)` | `KeyedStream<K, T>` | Key each stamped element. `K` must be `Equatable`. |
| `keyed.window(width, watermark: lateness, late: .Drop)` | `Stream<Window<K, T>>` | Group elements into tumbling windows of `width`. `late:` defaults to `.Drop`. |

`Window<K, T>` has `key`, `start` and `end` (`DateTime`), and `events`, a list
of `StreamEvent<T>` (`value` plus `event_time`) in arrival order. A window
starts at a multiple of `width` since the Unix epoch. The watermark is the
largest event time seen so far minus `lateness`, and it never moves back. An
element older than the watermark is late, and `LateEventDisposition.Drop`
discards it. A window is emitted once the watermark reaches its end. When the
source ends, the remaining windows are emitted. Windows that become ready at
the same time come out in the order their keys were first seen, then by start.
`width` must be positive and `lateness` must not be negative; anything else
panics when the stream is first pulled.

### `core.data.plot` — deterministic data plots

`core.data.plot` turns aligned labels and numeric values into a deterministic
plan and renders that plan as text or SVG. The source implementation is a
portable inspection renderer: it does not require a GPU, and it makes missing
values explicit as zero. The module contract is defined in
[`Core/Data/Plot.jet`](../../../Core/Data/Plot.jet) and exercised by the plot
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
values, not bytes; byte-size formatting uses SI units.

`percent` groups the scaled number before appending `%`. Scientific output
uses an unpadded exponent (`e0`, `e-3`, `e23`) and rounds the exact binary
value to the requested precision with ties to even, preserving negative
zero. Empty padding fill means a space; centered padding restarts the fill
pattern on each side. `pretty` expands a complete outer brace, bracket, or
parenthesis fragment, recursively splitting only its top-level commas;
quoted delimiters, redacted lists, incomplete fragments, and trailing text
remain unchanged.

Exact integer formatting also uses the public `Fixed`, `Grouped`, `Hex`,
`Bin`, and `Oct` interpolation selectors. The `quantity`, `fixed_int`,
`grouped_int_fixed`, and `*_decimal` source helpers support compiler lowering;
they are not exported application APIs. Their lexical-policy witnesses live
with the source helpers, while application examples use public formatting.

| Signature | Result | Description |
|---|---|---|
| `number(value: Int) -> String` | text | Format an integer with grouping separators. |
| `bin(value: Int) -> String` | text | Format an integer in base 2. |
| `oct(value: Int) -> String` | text | Format an integer in base 8. |
| `hex(value: Int, width: Int) -> String` | text | Format lower-case hexadecimal with the requested width. |
| `ordinal(value: Int) -> String` | text | Add an English ordinal suffix. |
| `plural(value: Int, singular: String, many: String) -> String` | text | Choose a singular or plural word from the value. |
| `pad_left(text: String, width: Int, fill: String) -> String` | text | Pad on the left; an empty `fill` pads with spaces. |
| `pad_right(text: String, width: Int, fill: String) -> String` | text | Pad on the right; an empty `fill` pads with spaces. |
| `pad_center(text: String, width: Int, fill: String) -> String` | text | Pad on both sides; the left side gets the smaller half and the fill pattern restarts on each side (`pad_center("x", 4, "ab")` is `axab`). |
| `pad(text: String, width: Int, fill: String) -> String` | text | Apply the module's default padding direction (right; `pad("ab", 5, "")` is `ab   `). |
| `grouped(value: Float, frac: Int) -> String` | text | Add grouping separators and keep `frac` fractional digits, rounded like `decimal`. |
| `decimal(value: Float, frac: Int) -> String` | text | Format fixed-point decimal output, rounding half away from zero with no negative zero (`decimal(0.125, 2)` is `0.13`, `decimal(-0.0001, 2)` is `0.00`). |
| `percent(value: Float, frac: Int) -> String` | text | Format a grouped percentage with `frac` fractional digits (`percent(12345.6, 0)` is `1,234,560%`). |
| `sci(value: Float, frac: Int) -> String` | text | Format normalized scientific notation with a plain exponent (`sci(1234.0, 2)` is `1.23e3`, `sci(9.999, 2)` is `1.00e1`). |
| `pretty(value: String) -> String` | text | Lay out Debug text one item per line with a space before the opener and commas between items; empty collections stay on one line, and `[:]` and `[redacted]` are unchanged. |
| `bytes(value: Int) -> String` | text | Format bytes in SI units `B`, `KB`, `MB`, `GB`, `TB`, `PB`, or `EB`, with one decimal below 10 (`1536` is `1.5 KB`, `1000` is `1 KB`). |
| `duration(milliseconds: Int) -> String` | text | Format a duration using at most three nonzero units. |

Calls run [`Core/Text/Fmt.jet`](../../../Core/Text/Fmt.jet); interpolation
selectors and compile-time calls run the shared Prelude kernel with the same
steps, so every tier prints the same text. Fixed-precision rounding rounds the
exact binary value at `frac` decimals, half away from zero (`decimal(2.675, 2)`
is `2.67`: the stored binary value is just below 2.675); non-finite values print as ordinary
`Float` text. `duration` does not emit unbounded trailing zero units.
The library-call boundary is deliberate: ordinary human formatting remains explicit (`D-HUMANFMT1`), and `pretty` follows the expanded deterministic Debug shape (`D-FMT-PRETTY1`).

### `core.log` — structured logging

`core.log` writes structured records through a configured sink. The levels are
`LogLevel.Debug`, `.Info`, `.Warn`, `.Error`, `.Critical` and `.Fatal`; filters
spell them `debug`, `info`, `warn`, `error`, `critical` and `fatal`. A record
can carry typed fields and a redaction marker, and every record carries a
`module` field: the `SourceLocation.module` of the call that logged it. The
package source is [`Core/Log/Log.jet`](../../../Core/Log/Log.jet).

| Signature | Result | Description |
|---|---|---|
| `debug(message: String) -[Log]>` / `info(message: String) -[Log]>` / `warn(message: String) -[Log]>` | — | Emit a record at the named level. |
| `error(message: String) -[Log]>` / `critical(message: String) -[Log]>` / `fatal(message: String) -[Log]>` | — | Emit a high-severity record. |
| `log(level: LogLevel, message: String) -[Log]>` | — | Emit at a caller-selected level. |
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
| `set_sink(kind: String, path: String) -[Log]>` | — | Select the output. |
| `set_level(level: LogLevel, path: String{""}) -[Log]>` | — | Set the code's level for the module at `path` and every module under it; `""` is every module. |
| `sample_every(n: Int) -[Log]>` | — | Sample every positive number of records. |
| `counter(name: String, value: Int) -> LogField` | field | Build a counter field. |
| `otlp_file(path: String) -[Log, FS]>` | — | Select a file sink for OTLP output. |
| `set_trace_id(id: String) -[Log]>` | — | Attach a trace identifier to later records. |
| `setup(spec: String) -[Log]>` | — | Apply comma-separated `key=value` settings; a bare `auto`, `json` or `text` selects the format. |
| `enabled(level: LogLevel) -[Log]> Bool` | Boolean | Test whether a record at `level` from the calling module would be written. |
| `record(level: LogLevel, message: String, fields: [LogField]) -> LogRecord` / `handler(level: LogLevel) -> Handler` | record, handler | Build a record, or a handler that writes records at or above `level`. |
| `flush() -[Log]>` / `disable() -[Log]>` | — | Flush records or disable logging. |

The emitting functions, `handle`, `set_level` and `enabled` also take a
trailing `at: SourceLocation{#caller}`; callers leave it to the compiler.

**Level filter.** A filter is comma-separated pairs: a bare `level` covers
every module, and `module=level` covers that dotted module path and every
module under it. For each record the pair with the longest matching path
wins. Five sources set pairs, each above the earlier on the same path:

1. stock: `info`;
2. the package's top-level `log:` field in `package.jet`, such as
   `log: "warn,shop.db=debug"`;
3. the code's `set_level` calls;
4. the `JET_LOG` environment variable, such as `JET_LOG=warn,shop.db=debug`;
5. the `--log=<filter>` flag of `jet run` and `jet dev`.

A `set_level` call for a path that `JET_LOG` or `--log` sets keeps the
operator's level and logs one `warn` note naming the call's file and line. A
pair that is not `level` or `module=level` is refused where it is written: the
package's `log:` field fails the manifest check, `--log` is a usage error
(E2104), and `JET_LOG` ends the program at startup with an error naming the
pair. `jet explain log [<file|dir>]` lists each source with its file and line,
and the filter the program starts with.

Sink names are normalized to `stderr`, `json`, `jsonl`, or `text`; there is
no `stdout` sink. `jsonl` and `text` sinks require a path. An unsupported sink
kind, a file sink without a path, or an unknown `setup` setting leaves the
configuration unchanged and logs an `error` record naming the rejected
setting. Keep secrets in `redact` fields so
the sink, rather than each caller, owns the redaction decision; a
`#Credential` value passed to any `core.log` call is denied at compile time
(E0722).
`fatal` emits, flushes, and terminates the process with status 1; `disable`
suppresses later emission until process end.

### `core.tasks` — timers and scheduler identity

`core.tasks` supplies the scheduler's timers and task identity. A task yields
at explicit suspension points; the scheduler does not turn a blocking
computation into preemptive parallelism. Mutable task-owned values stay with
their task; values crossing a task or channel boundary must satisfy Jet's
sendability rules. The implementation is in
[`Core/Tasks/Tasks.jet`](../../../Core/Tasks/Tasks.jet).

| Signature | Result | Description |
|---|---|---|
| `after<T>(delay: Duration, value: T) -[Time]> Receiver<T>` | receiver | Deliver one value after a duration, then close. |
| `interval(period: Duration) -[Time]> Receiver<Int>` | receiver | Deliver successive tick counts starting at one. |
| `recv<T>(rx: Receiver<T>) -[Time.Wait]> T?` | value | Receive the next value; None once the channel is closed and drained. |
| `try_recv<T>(rx: Receiver<T>) -> T?` | value | Take a queued value without waiting; None when nothing is queued. |
| `waitall<T>(receivers: [Receiver<T>]) -[Time.Wait]> [T?]` | values | One `recv` per receiver, in order. |
| `waitany<T>(receivers: [Receiver<T>]) -[Time.Wait]> (index: Int, value: T?)` | answer | The receiver that answered and its `recv` result; the list must not be empty. |
| `yield_now() -[Time]>` | — | Yield to another runnable task. |
| `current_task() -> TaskID` | identity | Identify the running task. |
| `cpu_ns() -[Time]> Int` | nanoseconds | The CPU time the running task has used. |
| `timeout(delay: Duration) -[Time]>` | — | Wait for a duration before returning. |
| `quiesce() -[Exec]> Bool` | joined | Join the scheduler's carrier and timer threads before a raw exec or fork that needs a single-threaded process; `false`, with nothing stopped, while a task or a timer is pending. The threads start again on the next use. |

`TaskID` is opaque: it compares with `==`, keys a map and prints, and nothing
else reads it. Two reads by one task are equal and differ from every other running task's;
a finished task's identity can be reused, and its printed form changes from run
to run, so compare identities rather than printing them into golden output.
Outside a scheduled task it identifies the calling thread. `cpu_ns` counts only
the time the task ran, on whichever carrier threads ran it, and stands still
while the task is parked; on a plain thread it is the thread's CPU time. Only
the difference of two reads by the same task means anything. A deterministic
world has no CPU clock to control, so it rejects the read.

`TaskState` is the pure integer convenience queue; generic task and channel
handles are compiler-owned scheduler surfaces. A capacity of zero means that
the convenience queue has no finite limit. `put` returns the unchanged state
when it is closed or at capacity, so callers that require backpressure must use
the scheduler receiver surface. Cancellation closes the receiver and leaves
its handle available for identity and ownership.
Generic channels return `(Sender<T>, Receiver<T>)`; tasks send owned values and do not use `async`/`await` (`D-CONC-CHAN1`; `D-SHAPE-COPY1`). Cancellation and pause/resume are control-plane requests on owned task handles (`D-COROUTINE1`), while shared memory uses `Shared<T>` guards and `Condition` rather than a raw lock (`D-SHAREDGUARD1`).
`Condition` and `RealtimeStream` remain native carriers. Their registered Core
source owners supply their declared methods without manufacturing a struct or
enum declaration. This ownership does not export `tasks.Condition`: use the
existing bare `Condition` spelling, and keep ordinary application impls on
these builtin carriers rejected.
Qualified annotation checks derive these owner-only names and compiler-owned
Core enum names from generated metadata, and still require a public type export
from the imported module.

The source scheduler-channel constructors preserve the declared
`(sender: Sender<T>, receiver: Receiver<T>)` pair by projecting the raw
channel's fields by name, rather than returning `TaskState` or depending on
the raw tuple's positional order. Bounded scheduler channels clamp capacities
below one to one. Interval periods truncate to whole milliseconds and clamp to at least
one millisecond; their first delivered tick is one.

The source `Condition` extension delegates notification to the existing shared
condition protocol, preserving parked waiters rather than fabricating state.
The realtime stream extension likewise observes and cancels the same provider
worker; its source callback constructor rejects nonpositive rates before
crossing the provider boundary.

The list adapters `para_map`, `para_filter`, `para_partition` and `para_fold`
run their callback on the task scheduler's pool on every tier: `jet run`
and `jet dev` (O0), `jet build --optimize=basic` (O1) and `jet build` (O2) all call the one
runtime kernel, `jet_rt_native_para_walk` in
[`Parallel.jet`](../../../Compiler/JetRuntime/Source/Parallel.jet). Each
worker runs on a task fiber, so the pool's carrier threads (by default one
per CPU of the host's available parallelism: cgroup quota, CPU affinity,
online CPUs),
bound how many items run at once; an item that waits parks and frees its
carrier. Without `limit:` there are as many workers as carriers; a
`para_map(f, limit: n)` has up to `n` workers, and so up to `n` items in
flight, however many carriers there are. `para_map`, `para_filter` and
`para_partition` spread their items over the workers in chunks of at most 64
items, so a short list of costly items still reaches every worker.
`para_map` and `para_filter` keep source order, and `para_fold` folds
each 64-item chunk from a fresh seed and then merges the chunk results up a
fixed tree, so every tier and every worker count gives the same result.
Compile-time code also runs natively at O0, but there the kernel uses one
worker: a `para_*` call in a `comptime` block, a `comptime fn` or a value
computed while building runs its chunks in order and returns the same value.

### `core.prelude` — small compiler-owned helpers

`core.prelude` is available without a separate package import. Its functions
are deliberately small barriers and predicates used by generic code and
constant evaluation; they are not alternate syntax for bindings. See
[`Core/Prelude/Prelude.jet`](../../../Core/Prelude/Prelude.jet).

| Signature | Result | Description |
|---|---|---|
| `keep<T>(value: T) -> T` | `T` | Preserve a value at an explicit use site. |
| `identity(value: &&String) -> String` | text | Return a borrowed string unchanged. |
| `always(value: Bool) -> Bool` | Boolean | Return a Boolean unchanged. |
| `identity_int(value: Int) -> Int`, `identity_float(value: Float) -> Float`, `identity_bool(value: Bool) -> Bool` | same type | Typed identity helpers for primitive values. |
| `const_int(value: Int, ignored: Int) -> Int` | integer | Select the first integer in a constant-evaluation expression. |
| `const_bool(value: Bool, ignored: Bool) -> Bool` | Boolean | Select the first Boolean in a constant-evaluation expression. |
| `not_bool(value: Bool) -> Bool` | Boolean | Negate a Boolean. |
| `min<T: Comparable>(a: T, b: T, rest: ...T) -> T` / `max<T: Comparable>(a: T, b: T, rest: ...T) -> T` | `T` | The first least or greatest of two or more ordered values, by `compare` (a Float by IEEE 754 totalOrder). Prelude names: no import. |
| `clamp<T: Comparable>(x: T, lo: T, hi: T) -> T` | `T` | Clamp `x` to the bounds by `compare` (a Float by IEEE 754 totalOrder); reversed bounds are accepted. Prelude name: no import. |
`keep` is the approved use-site identity sink when measurement must observe a value (`D-BENCH-KEEP1=A`).

`SourceLocation` is the record `#here` and a `#caller` parameter default
(`at: SourceLocation{#caller}`) give: `file`, `line`, `column`, `function`
and `module`, displayed as `file:line:column`. `module` is the dotted module
of the place: the package's name, then the `name: module` declarations that
reach the file, then the inline modules around it. In package `shop`, code in
`run.jet` is `shop`, code in the file `cache: module` declares is
`shop.cache`, and code inside `db: module { … }` there is `shop.cache.db`.
A file that no `name: module` declaration reaches sits at the package root.
The compiler fills `module` at every `#here` and `#caller` site; its `""`
default only covers a record built by hand. `core.log` filters records by it.

### `core.testing` — deterministic test helpers

`core.testing` provides deterministic values and comparison helpers for tests;
it does not replace `jet test`'s test discovery. A test can use a fake clock,
fake random source, fake data, temporary directory, fixture, corpus, snapshot,
or golden file. The public source is
[`Core/Testing/Testing.jet`](../../../Core/Testing/Testing.jet).

| Signature | Result | Description |
|---|---|---|
| `assert_equal(comparison: TestComparison) -> Bool` | Boolean | Check the comparison's recorded proof fields. |
| `compare<E>(cases: [Dyn], reference: fn(Dyn) -> Dyn E!, candidate: fn(Dyn) -> Dyn E!, relation: String) -> TestComparison E!` | comparison | Compare reference and candidate outputs over explicit cases. The callbacks' shared failure type passes up (D-CALLBACK-ERR1=A); callbacks that cannot fail make the comparison `Never!`. |
| `snap(name: String, value: String) -[FS, Env]> Bool` | Boolean | Compare or create `__snapshots__/<sanitized-name>.snap`; `JET_UPDATE_SNAPSHOTS=1` rewrites it. Snapshot names retain ASCII letters, digits, `-`, and `_`, replacing other scalars with `_`; an empty name becomes `case`. A mismatch returns `false` and records `snapshot differs: <name>` with the same bounded unified diff as `golden`, so a failing `assert` shows it. The `expect(value).snapshot()` builtin is `assert(snap("<file>_<line>", "{value}"))`. |
| `golden(name: String, value: String) -[FS, Env]> Bool` | Boolean | Compare `value` with the file `name`. A missing, unreadable, or differing file returns `false` and records the reason (with a bounded unified diff for a difference). The next failing `assert` or `assert_eq` on that thread reports the recorded reason as its stop message and the path and diff as its locals; a passing helper call clears it. Under `JET_UPDATE_SNAPSHOTS=1` (`jet test -u`) a missing or differing file is written, parent folders included, and the check passes. |
| `fixture(name: String) -[FS]> String` | text | Load a named fixture. |
| `skip(reason: String)` | — | Mark the running `#Test` skipped, like Go's `t.Skip`: `skipped: <reason>` joins its output and it reports skipped, as a first `.skip { }` member makes it. The body goes on, so return after it. Outside a running test nothing happens. |
| `temp_dir(prefix: String) -[FS]> String` | path | Allocate an isolated temporary directory. |
| `corpus(glob: String) -[FS]> [String]` | contents | Read each readable regular file directly inside the directory `glob` (the parameter keeps its historical name; it is not a pattern), ordered by full path; an absent directory gives `[]`. |
| `corpus_entries(root: String, suffix: String) -[FS]> [CorpusEntry]` | entries | List the regular files below `root`, at any depth, whose names end in `suffix`, ordered by `id`. `CorpusEntry{id, path}`: `id` is the path below `root` without `suffix` (`basics/hello` for `<root>/basics/hello.jet`). Names starting with `.` are skipped, folders included; an absent root gives `[]`. |
| `fake_clock(unix_ms: Int) -> Clock` | clock | Create a deterministic clock at a Unix-millisecond value. |
| `fake_rng(seed: Int) -> RNG` | RNG | Create deterministic random state. |
| `fake_data(seed: Int) -> Fake` | fake | Create a deterministic fake-data carrier. |
| `test_suite() -> Test` | suite | Create the default suite record. |
| `world(body: fn(DeterministicWorld))` | — | Run a body with deterministic world services. |
| `histories<Command>(seed: Int, cases: Int, strategy: HistoryStrategy<Command>?, model: fn([Command]) -> Dyn, actual: fn([Command]) -> Dyn, observe: fn(Dyn) -> Dyn) -> TestComparison Err!` | comparison | Run a typed history campaign (`D-TEST-HISTORY1`, `D-TEST-STRATEGY1`): one `HistoryRNG` seeded by `seed` feeds every `generate`; invalid or out-of-bounds cases are discarded, a mismatch shrinks to a valid reduced case and fails with `history mismatch in case `<id>`: <op> -> <op> …`, whose cause carries the replayable `history-artifact {…}` JSON. Without `strategy`, the compiler derives one from an enum whose payloads are `Int`, `Bool` or `String`; any other command type is `unsupported`. The artifact's source, tool and target identities come from the compiler. `HistoryRNG` is a Core SplitMix64 generator whose copies share one stream; `next_u64()` and `below(bound)` return `Int` (`D-TEST-COUNT1`). |
| `status(comparison: TestComparison) -> String` | text | Read the comparison status. |

`#Test` is the only test declaration syntax (`D-TESTKIT1`).
Use `#Test` for test declarations and keep assertions about behavior rather
than implementation details. The compiler and test runner own test discovery;
the helper types only control inputs and observations.

### `core.regex` — linear-time regular expressions

`core.regex` is a Thompson/Pike VM over Unicode scalars, so every tier gives
one answer. Matching is linear in the text, with no recursion and no step
budget. Matching is leftmost-first, as in RE2 and Rust: the leftmost match
wins, and among the matches that start there, alternation prefers its earlier
arm and a repetition prefers its greedy count. `Regex{"a|ab"}.find("ab")` is
`"a"`, while `full_match` asks whether any reading covers the whole text, so
`Regex{"a|ab"}.full_match("ab")` is `true`.

The syntax:

- concatenation, alternation `|`, capture groups numbered by their `(`,
  `(?:…)`, and named groups `(?<name>…)` or `(?P<name>…)`. A group name is
  ASCII letters, digits and `_`, does not start with a digit, and is unique.
- `*`, `+`, `?`, `{n}`, `{n,}` and `{n,m}` (a count may be at most 1000), each
  with a lazy form written with a trailing `?` (`*?`, `{2,5}?`), which prefers
  the smallest count. A repetition cannot directly follow another, so `a**`
  is an error.
- `.` (any scalar except `\n`), `^` (start of text), `$` and `\z` (end of
  text), `\A` (start of text), and `\b`/`\B` (an ASCII word boundary, or its
  absence).
- classes over scalars: `[]`, `[^]`, ranges, and a `]` written first as a
  member (`[]a]`). A literal `[` inside a class is written `\[`.
- the class escapes `\d` and `\w` (ASCII), `\s` (Unicode White_Space), and
  `\p{L}`/`\pL`/`\p{Letter}`/`\p{Alphabetic}`, `\p{N}`/`\p{Number}` and
  `\p{White_Space}`. `\D`, `\W`, `\S` and `\P{…}` are their negations. All
  of them work inside `[]` too, so `[\D_]` and `[^\P{L}]` are classes.
- the escapes `\n \r \t \f \v \a`, `\xHH`, `\uHHHH`, and `\x{…}`/`\u{…}` for any
  Unicode scalar value. A `\` before ASCII punctuation or a space matches that
  character; any other escape is an error.

`ignore_case` compares Unicode simple case mappings; a range matches a scalar
when either its lowercase or its uppercase form is in the range.
`multiline` lets `^` match after a newline. `dot_matches_newline` lets `.`
match `\n`. A `RegexFlags` value displays as its inline-flag letters (`"im"`).

Backreferences, lookaround and inline flags (`(?i)`) are syntax errors: the
first two would break the linear bound, and flags go through `compile_with`.
Groups nest at most 250 deep, and a compiled program holds at most 100000
instructions (nested counted repetitions multiply), so compile time and
memory stay proportional to a bounded pattern.

A malformed pattern is a `RegexError { offset: Int, reason: String }`.
`offset` is the zero-based character position in the pattern where the
problem is, and the error displays as
`invalid regex at position {offset}: {reason}`. `RegexError` converts to
`Err`, so `rx: re.compile(text)` propagates inside a fallible function. The
implementation is in [`Core/Regex/Regex.jet`](../../../Core/Regex/Regex.jet).

Checked `Regex{"…"}` literals and inferred-head `{"…"}` arguments in a
`Regex`-typed slot produce the same source-owned `Regex` as `compile`; there is
no separate native regex carrier or engine. The compiler checks the complete
literal by running the same Core `compile`, so a literal and a run-time
pattern can never disagree about what a pattern means. An invalid literal is
E0152, which carries the same position and reason as the run-time
`RegexError`. The literal then lowers through the source `compile` function,
which keeps its checked success value.

`Regex` has the public fields `pattern` and `flags`. `Match` has the public
fields `start`, `end`, `text` and `groups`; `start` and `end` are character
(Unicode scalar) positions, and `groups` is a positional list in which a group
that did not take part is `""`. `replace`, `replace_first` and `expand` share
one template dialect: `$N` and `${N}` insert group `N` (`$0` is the whole
match), `${name}` inserts a named group, `$$` is a literal `$`, and any other
`$` is literal text. Use the compiled `Regex` value when the same expression is
used repeatedly, and call `escape` for literal user input.

Methods on `Regex` take the receiver as the pattern of the free function of
the same name: `is_match`, `full_match`, `match`, `find`, `find_all`,
`matches`, `split`, `split_limit`, `replace`, `replace_first`, plus
`replace_all_with(text, replacer: fn(Match) -> String)`, which replaces every
match with what `replacer` returns for it. Methods on `Match`:

| Method | Result | Description |
|---|---|---|
| `group(n: Int) -> ?String` | text | Group `n` (0 is the whole match); none when there is no group `n` or it took no part. |
| `name(name: String) -> ?String` | text | The named group, as `group` reports it. |
| `group_start(n: Int) -> ?Int` | position | Character position where group `n` starts. |
| `group_end(n: Int) -> ?Int` | position | Character position where group `n` ends. |

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
| `expand(match: Match, template: String) -> String` | text | Expand a replacement template against a match. |
| `purge() !RegexError` | — | Report that the source engine has no mutable cache to purge. |
| `subn(pattern: Regex, text: String, replacement: String) -> {text: String, count: Int}` | replacement receipt | Replace matches and return their count. |

The provider also exposes the concise names `regex`, `join`, `search`,
`findall`, `fullmatch`, `sub`, and `subn` for code that uses the corresponding
standard-library vocabulary.

### `core.ui` — portable node trees and backends

`core.ui` represents a UI as a typed node tree plus geometry and accessibility
metadata. It provides portable node construction and backend adapters; it does
not pretend that a desktop, phone, or terminal backend has the same input or
rendering capabilities. The source is [`Core/Ui/Ui.jet`](../../../Core/Ui/Ui.jet);
nodes and backends are plain Jet values, with no `#Unsafe`.

A `UINode` shows its `label`, `width` and `height` (in cells) and the
optional `accessibility`, `ime` and `shortcut` metadata; its kind, role,
children, color and click handler are its own. `box` stores its children in
the returned tree.

The IME capability is spelled `UI.IME`, alongside `UI.Keyboard` and
`UI.Pointer`; capability checks and runtime support use the same spelling.

| Signature | Result | Description |
|---|---|---|
| `point(x: Float, y: Float) -> Point` | point | Construct a point. |
| `size(width: Float, height: Float) -> Size` | size | Construct a size. |
| `rect(x: Float, y: Float, width: Float, height: Float) -> Rect` | rectangle | Construct a rectangle. |
| `constraint(min_width: Float, min_height: Float, max_width: Float, max_height: Float) -> SizeConstraint` | constraint | Describe allowed layout size. |
| `node(label: String, width: Float, height: Float) -> UINode` | node | Construct a sized node. |
| `box(children: [UINode]) -> UINode` | node | Group child nodes. |
| `text(value: String) -> UINode` | node | Construct a text node. |
| `preview(name: String, body: fn() -> UINode, *, viewport: UIPreviewViewport?) -> UIPreview` | preview | Describe a named preview; `viewport:` defaults to `desktop()`. |
| `playground(name: String, body: fn() -> UINode, *, viewport: UIPreviewViewport?) -> UIPreview` | preview | Describe a named interactive playground; `viewport:` defaults to `desktop()`. |
| `previews(entries: [UIPreview]) -> UIPreviewRegistry UIPreviewError!` | registry | Register at most 64 previews and playgrounds, each with a non-empty unique name. |
| `UIPreviewRegistry.render(name: String) -> UINode UIPreviewError!` | node | Call the named preview's body. |
| `desktop() -> UIPreviewViewport`, `phone() -> UIPreviewViewport`, `tablet() -> UIPreviewViewport` | viewport | Select standard preview dimensions: 1280×800 at scale 1, 390×844 at scale 2, 834×1119 at scale 2. |
| `key_event(code: String) -> InputEvent` | event | Create `InputEvent.Key{code: code}`. |
| `resize_event(width: Float, height: Float) -> InputEvent` | event | Create `InputEvent.Resize{size: Size{width: width, height: height}}`. |
| `reactive_render(body: fn())` | — | Ask the provider to render a reactive body. |
| `tui_backend() -> TUIBackend`, `null_backend() -> NullBackend` | backend | Select a backend; copies of a backend share it. |
| `mount(backend, tree: UINode)`, `mount(backend, tree: UINode, constraint: SizeConstraint)` | — | Call the backend's `mount`. |
| `aria_role_button() -> UIAriaRole`, `aria_role_container() -> UIAriaRole` | role | Construct standard accessibility roles. |
| `aria_role_label() -> UIAriaRole`, `aria_role_text_input() -> UIAriaRole` | role | Construct label or text-input roles. |
| `node_accessibility(n: UINode, metadata: UIAccessibility) -> UINode` | node | Attach accessibility metadata. |
| `node_color(label: String, width: Float, height: Float, color: String) -> UINode` | node | Construct a colored node. |
| `node_role(label: String, width: Float, height: Float, role: UIAriaRole) -> UINode` | node | Construct a node with a role. |
| `node_shortcut(n: UINode, shortcut: UIShortcut) -> UINode` | node | Attach a shortcut descriptor. |

Both backends have the same methods. `measure(tree, constraint) -> Size` is a
box's widest child by its children's summed heights, any other node its own
size, clamped to the constraint. `layout(tree, frame: Rect)` places the next
paints. `paint(tree)` draws the tree and makes its buttons and text inputs the
focus ring. `mount(tree, constraint?)` measures within the constraint (80 × 24
cells when absent), lays out at the origin and paints. `on_event(event:
InputEvent) -> EventResult` answers `Handled` or `Ignored`; Tab moves the
focus to the next node of the ring. `set_focus_group(nodes: [UINode])` sets
the ring and `focused_label() -> String` names its focused node ("" for
none). `NullBackend.commands() -> [String]` lists what the paints since the
latest mount drew: `fill(rect, color)` for each leaf but a text, then
`text(rect, label)`. `TUIBackend` writes each leaf's label into a grid of
terminal cells: `frame_lines() -> [String]` gives its rows without trailing
white space and `render_count() -> Int` counts its paints. In a web build the
page shows each paint.

`UIPreviewViewport` is `{device: UIPreviewDevice, width: Int, height: Int,
scale: Int}` and `UIPreview` is `{kind: UIPreviewKind, name: String, viewport:
UIPreviewViewport, body: fn() -> UINode}`; both are plain Jet values on every
tier. Registration fails with `UIPreviewError.EmptyName`, `.TooManyPreviews`,
or `.DuplicatePreview(name)`; rendering an unregistered name fails with
`.UnknownPreview(name)`. The registry function can be imported directly as
`core.ui.[previews]`.
Host file-dialog, clipboard, shortcut, accessibility, and IME consumers declare
their corresponding `UI.*` authority needs and use the current failure carrier;
IME polling yields an optional event after host failure propagation.

#### Native windows

`open` shows a native window and `headless` gives the same `Window`, `Frame`
and `UIEvent` types with no device, for tests. The program draws its own RGBA
pixels; Core speaks the display server's wire protocol itself (Wayland now,
picked from `$WAYLAND_DISPLAY`; [`Core/Ui/Wayland.jet`](../../../Core/Ui/Wayland.jet)),
with no C library, so programs need no `#Unsafe` and no native dependency.

```jet
use ui: core.ui

run: fn() {
    window: ui.open(width: 640, height: 400, title: "demo") ?? return
    window.set_app_id("org.example.Demo") ?? return
    loop frame in window.frames() {
        loop event in frame.events() {
            if event == .KeyDown(.Escape) -> return
        }
        pixels := [U8]{}
        loop _ in 0..<(frame.width * frame.height * 4) { pixels.push(U8{255}) }
        frame.present(pixels) ?? return
    }
}
```

The display paces the frames, exactly one in flight: a frame is due when the
compositor's frame callback arrives, so a window presents on every vblank
while the program keeps presenting and sleeps, with no timer, while it does
not, until input arrives. `frame.time` is the vblank the frame is predicted to
reach the screen at (from `wp_presentation` feedback), in seconds since the
first frame; compute motion from it, never per frame, and a late frame shows
the right position instead of slowing down. A frame starts as late as its
measured draw time allows, so its input is as fresh as possible. Three pooled
buffers rotate as the compositor releases them, and `present_damage` copies
and damages only the rectangles that changed.

A window is asked for in logical units; frames, pixels and event positions are
physical pixels, `frame.scale` of them per unit, so a 640 x 400 window on a
display scaled to 150% draws 960 x 600 frames and stays sharp. The scale comes
from `wp_fractional_scale_v1` with `wp_viewporter` when the compositor has
them, else from the largest `wl_output` scale the window is on; a change of
scale or size reaches the program as `.Resize` in pixels on the next frame.
A resize is acknowledged only by the commit of a frame drawn at the new size,
so the compositor never shows a stretched or cropped frame.

Jet draws the window's frame itself: a title bar with the icon from
`set_icon` (Jet's mark until one is set), the title centred on the window
(shifted only as far as it must to clear the icon and the buttons, and cut
with an ellipsis when even that space is too narrow), and minimize, maximize
and close buttons, rounded corners and a soft shadow, on a `wl_subsurface`
below the program's pixels. Frames stay the content: `open`'s size is the
content's, while the compositor configures the whole window, so one
configured 640 x 400 units draws 640 x 354. The title bar moves the
window, a double press on it maximizes, the secondary button opens the
compositor's window menu, and its edges and corners resize it; on a touch
screen the buttons spread to 46-unit targets. From the keyboard, F6 moves
the focus between the program and the frame's buttons, which show a
focus ring; Tab and Shift+Tab step across them, Enter or Space works the
focused one, Shift+F10 opens the window menu under it and Alt+Space under
the title, and Escape gives the keys back to the program. The keys the
frame takes never reach the program. A maximized or tiled window
has square corners and no shadow, and a fullscreen one no frame. When the
compositor draws frames itself (`zxdg_decoration_manager_v1` answers
server-side) or offers no `wl_subcompositor`, its frame shows instead and
frames are the whole window.

| Signature | Result | Description |
|---|---|---|
| `open(*, width: Int, height: Int, title: String) -[Env, Net, Time]> Window UIError!` | window | Open a window of 1 to 16384 logical units a side on the environment's display server: `.NoDisplay` when none is named, `.Connect` / `.Protocol` / `.Unsupported` when it fails. |
| `headless(*, width: Int, height: Int, script: [[UIEvent]]) -> Window UIError!` | window | A window with no device at scale 1: frame `i` carries `script[i]`, one more carries `.Close`, frames are 1/60 s apart, and each present records a checksum. |
| `Window.frames() -> Frames` | frames | The display-paced frames; the stream ends after the frame that carries `.Close`. |
| `Window.width()` / `.height()` / `.scale() -> Float` / `.failure() -> UIError?` / `.checksums() -> [Int]` | facts | The latest frame's size in pixels and pixels per logical unit, why the display ended the frames, and a headless window's per-frame FNV-1a checksums. |
| `Window.cursor() -> UICursor` / `.set_cursor(cursor: UICursor)` | cursor | How the pointer looks over the window, `.Default` until set: the CSS cursor keywords (`Pointer`, `Text`, `Wait`, `Grab`, `ResizeNWSE`, … in `wp_cursor_shape_v1` order) and `Hidden`. Without `wp_cursor_shape_v1` the compositor keeps its cursor for every shape but `Hidden`. |
| `Window.set_app_id(app_id: String) -> UIError!` | — | Name the application, reverse-DNS (`"org.example.Notes"`), so the desktop matches its desktop entry for name, icon and grouping; `"dev.jet.app"` until set. `.Invalid` for an empty id or one with a NUL byte. |
| `Window.set_icon(icon: UIIcon) -[Env]> UIError!` | — | Show `UIIcon{name, images}`: a theme icon name (`""` for none) and `UIIconImage{side, pixels}` images, `side` rows of `side` straight-alpha RGBA pixels, 1 to 1024 a side (`.Size` otherwise). An icon with no name and no images unsets it. Without `xdg_toplevel_icon_v1` the desktop entry's icon shows. |
| `Window.clipboard_text() -[Env, Net, Time]> String UIError!` / `.set_clipboard_text(text: String) -[Net, Time]> UIError!` | clipboard | Paste and copy through the seat's `wl_data_device`. A paste reads the holder's text in its first type of `text/plain;charset=utf-8`, `UTF8_STRING`, `text/plain`, `TEXT`, `STRING` through a pipe, waiting up to 2 s (`.Protocol` past that or for text that is not UTF-8, `.Size` past 16 MiB), and is `""` when the clipboard is empty or holds no text; the window's own copy comes back without a round trip. A copy offers the text as all five types at the latest input's serial (the compositor takes it only from the focused window) and answers each paste until another program copies. `.Unsupported` without `wl_data_device_manager`; a headless window keeps a clipboard of its own. The web host's clipboard stays `core.ui.host`'s `read_text` / `write_text`. |
| `Window.set_text_input(area: UIRect?) -[Net, Time]> UIError!` | text input | The program's text field, for the compositor's input method (`zwp_text_input_v3`) to compose into: `area` is the caret's rectangle in pixels, where the input method shows its candidates, and `None` says there is no text field, which disables the text input and ends any composition. Without `zwp_text_input_v3`, and in a headless window, typing comes from the keyboard alone. |
| `Window.maximized() -> Bool` / `.fullscreen() -> Bool` / `.set_maximized(maximized: Bool)` / `.set_fullscreen(fullscreen: Bool)` / `.minimize()` | states | The window's states as the display server last configured them, and requests to change them (`xdg_toplevel`'s set/unset maximized and fullscreen, on the output the display server picks, and set_minimized). The display server decides; a new size arrives as `.Resize`, and a minimized window comes back only when the user brings it back. A headless window takes each state at once and keeps its size. Each window has its own connection to the display server, so a program shows several windows by driving each window's `frames()` in its own task. |
| `Window.frame_stats() -> FrameStats` | stats | `presented`, `missed` (vblanks skipped between consecutive frames), `discarded`, `refresh_ns`, present-to-present `median_ns` / `p99_ns`, `spans` (intervals by refreshes spanned: 1, 2, 3, 4+) and input-to-photon `input_samples` / `input_median_ns` / `input_p99_ns`. |
| `Frame.index` / `.width` / `.height` / `.scale` / `.time` / `.delta` | fields | The frame's number, the size in pixels to draw at and the pixels per logical unit, and its predicted presentation time and the step since the previous frame's, in seconds. |
| `Frame.events() -> [UIEvent]` | events | Input since the previous frame: `KeyDown(Key)` (core.term's `Key`; Space is `Char(' ')`), `KeyUp(Key)`, `Text(String)`, `Compose(UIIMEComposition)`, `PointerMove(x:, y:)` in pixels, `PointerDown(UIPointerButton)`, `PointerUp`, `Scroll(dx:, dy:)` in pixels, `Resize(width:, height:)` in pixels, `Focus(Bool)`, `Close`. Keys go through the compositor's XKB keymap (`core.ui.xkb`; a US layout until one arrives), and a held key other than a modifier repeats as further `KeyDown` and `Text` at the compositor's rate after its delay. `Text` is also what an input method commits. `Compose` is its composition in progress (`core.ui.host.ime`'s type: `text`, the caret as a `selection` and the `marked` span in characters), which replaces the previous one until its text is committed. A composition with empty text ends it, and it ends before its committed `Text` arrives. |
| `Frame.present(pixels: [U8]) -[Env, Net, Time]> UIError!` | — | Show `height` rows of `width` RGBA pixels. |
| `Frame.present_damage(pixels: [U8], damage: [UIRect]) -[Env, Net, Time]> UIError!` | — | Show `pixels` where only the `damage` rectangles (in pixels) changed since the previous frame. |

Backends expose their own command or event projection through the backend
handle. The provider/catalog may add interactive node and mount operations,
but those operations remain typed provider boundaries. Build a portable tree
first, then use backend-specific capabilities; do not infer that a node tree
supplies a platform-native callback API.
UI backend allocation and host capability/dialog/clipboard/IME/drag polling
enter Jet source before reserved Core-only provider leaves. Public UI
capability admission and the existing per-function Core effect rows remain
unchanged. Lexical filesystem grants and accessibility metadata construction
are Jet policy, not host facade calls.
Terminal backend allocation, shortcut registration/dispatch, and host
accessibility attach/project also enter their Jet public functions first.
`attach` returns the provider's canonical node with the metadata attached,
`UINode UIHostError!`, and propagates the typed host error on failure.
`project` returns `UIAccessibilityProjection{node: UINodeID, metadata:
UIAccessibility}` when the node carries metadata.
`UIHostError` has the variants `Cancelled(UICancellation)`,
`CapabilityUnavailable(capability: String)`, `CapabilityDenied(capability:
UICapability)`, `InvalidRequest(String)`, `HostFailure(service: String,
message: String)`, `ResourceDenied(String)`, `ShortcutConflict(shortcut:
UIShortcut, existing_action: String)`, and `QueueFull(service: String)`.
Every `core.ui.host` service answer is Core's: the host is a provider of
primitives (capability flags, clipboard text, queued raw events, dialog
tokens and results), and multi-field provider text is a record of
length-prefixed fields (`<decimal UTF-8 byte length>:<bytes>`). A native
build's provider is headless and reports every capability absent, so each
service fails with `CapabilityUnavailable` after Core validates the request.
On the web, a page that installs no `globalThis.jetUiProvider` gets the
runtime's default browser provider, which forwards raw composition and
drag/drop events (event type, data, drop effect) to `ime.poll` and
`drag_drop.poll`; Core maps each event type to its phase (a `compositionend`
with no text cancels), places an unreported IME caret after the composition
text in characters, reads URI items from `text/uri-list` lines, and reports a
host buffer overflow as `QueueFull`. A drag event's `operation` is a
`UIDragOperation?`: a `none` drop effect is no operation, and a drop with no
operation arrives as `Cancel` (D-UI-DRAG-NONE1=A). `shortcuts.register`
claims its key from the provider, and the browser provider cancels the
default action of a claimed key press only and queues it for
`shortcuts.poll`, which answers the matching binding (`None` when no key is
queued); every unclaimed key keeps its browser default (D-UI-WEB-KEYS1=A).
Modifier bits are Control 1, Alt 2, Shift 4, Meta 8 and the logical command
16; a key pressed with exactly one of Control and Meta also matches a command
binding. Accessibility is
available. `clipboard.read_text` answers the clipboard text the page last
observed (a paste, copy or cut, or its own `write_text`), because a browser
lends the live clipboard only to a user gesture; before any is observed it
fails with `HostFailure`. File dialogs and font shaping are absent there.
TUI capability selection, ASCII-only case folding, encoded-OS flag semantics,
dimension parsing, and defaults are Jet policy. The private environment-byte
leaf samples the same startup facts; the shared terminal leaf remains the
canonical stdout `IsTerminal` sample. No environment effect is added to the
existing public capability row.

#### Drawing

A `Frame` also draws: its `clear_background` and `draw_*` calls paint a canvas
of the frame's size in Jet, the same on every backend and tier, clipped to the
canvas, and the frame presents that canvas when the next frame begins unless
the program called `present` itself. The canvas, like every `Image`, holds
premultiplied RGBA8 pixels, and every draw follows core.ui's one blend rule,
source-over on the sRGB bytes as they are: a color comes in straight and
premultiplies, red, green and blue each `(c * a + 127) / 255`, and each byte
of the canvas, alpha included, becomes `s + (d * (255 - a) + 127) / 255` for
the premultiplied source byte `s` (alpha 20 white over `#0B0B0D` gives
`#1E1E20`). A clear stores its color instead. Pixels read back straight: each
color channel `(c * 255 + a / 2) / a`, at most 255, and a clear pixel all
zeros. Shapes that take
a `Point` or `Rect` fill the pixels whose centers lie inside, with coordinates
snapped to 1/256 pixel; a pixel whose center lies on a left or top edge is
inside, one on a right or bottom edge is not. The collision checks and the
`Image` functions are pure and need no effect; `image_*` functions change an
`Image` in place through `&`.

| Signature | Result | Description |
|---|---|---|
| `Frame.clear_background(color)` | — | Set every canvas pixel to the color, alpha included. |
| `color(r, g, b, a)` | `Color` | Clamp each component to `0..255` and construct a color. |
| `Frame.draw_rectangle(x, y, width, height, color)` | — | Fill a rectangle, clipped to the canvas and blended src-over. |
| `Frame.draw_line(start_x, start_y, end_x, end_y, color)` | — | Draw a one-pixel line including both endpoints; along the longer axis each step draws one pixel at the line value rounded half up. |
| `Frame.draw_circle(center_x, center_y, radius, color)` | — | Fill every pixel within `radius` (an `Int`) of the center; a radius below 1 draws nothing. |
| `Frame.draw_triangle(x0, y0, x1, y1, x2, y2, color)` | — | Fill the pixels whose centers lie inside the triangle, in either winding; triangles sharing an edge never both cover a pixel. |
| `Frame.draw_pixel(x, y, color)` | — | Blend one pixel. |
| `Frame.draw_rectangle_rec(rec, color)` | — | Fill a `Rect`; an empty or negative side draws nothing. |
| `Frame.draw_rectangle_lines(x, y, width, height, color)` | — | Draw the rectangle's one-pixel border inside its bounds, each pixel once. |
| `Frame.draw_rectangle_lines_ex(rec, line_thick, color)` | — | Draw a border `line_thick` wide inside `rec` as four bands: top and bottom full width, the sides between them. A thickness past either side becomes half the shorter side. Bands are blended one after another. |
| `Frame.draw_rectangle_gradient_v(x, y, width, height, top, bottom)` / `.draw_rectangle_gradient_h(x, y, width, height, left, right)` | — | Fill a rectangle shaded from the first color to the second, top to bottom or left to right. |
| `Frame.draw_rectangle_gradient_ex(rec, top_left, bottom_left, bottom_right, top_right)` | — | Fill a rectangle shaded between its four corner colors, given counter-clockwise from the top left. Each pixel's color is interpolated at its center across the two triangles split along the top-left to bottom-right diagonal, rounded half up per channel. |
| `Frame.draw_rectangle_rounded(rec, roundness, segments, color)` | — | Fill a rectangle whose corners are arcs of radius `roundness` (clamped to `1`) times half the shorter side, each arc a polygon of `segments` steps, or of an automatic count when `segments` is below 4. A `roundness` of 0 or below, or a side below 1, fills the plain rectangle. |
| `Frame.draw_rectangle_rounded_lines(rec, roundness, segments, color)` | — | Draw the one-pixel band just outside the rounded rectangle. A `roundness` of 0 or below draws the one-pixel band around `rec`. |
| `Frame.draw_circle_lines(center_x, center_y, radius, color)` | — | Draw the pixels of `draw_circle`'s disc that touch a pixel outside it. |
| `Frame.draw_ellipse(center_x, center_y, radius_h, radius_v, color)` | — | Fill the pixels at offset `(dx, dy)` from the center with `(dx * radius_v)^2 + (dy * radius_h)^2 <= (radius_h * radius_v)^2`; a radius of 0 or below draws nothing. |
| `Frame.draw_ellipse_lines(center_x, center_y, radius_h, radius_v, color)` | — | Draw the pixels of `draw_ellipse`'s fill that touch a pixel outside it. |
| `Frame.draw_ring(center, inner_radius, outer_radius, start_angle, end_angle, segments, color)` | — | Fill the ring between the radii from `start_angle` to `end_angle` (degrees), built of segments; with an inner radius of 0 or below it fills a circle sector. Argument fixes apply: swapped radii and angles are reordered and an outer radius of 0 or below becomes 0.1. Each pixel is blended once. |
| `Frame.draw_ring_lines(center, inner_radius, outer_radius, start_angle, end_angle, segments, color)` | — | Draw the pixels of `draw_ring`'s fill that touch a pixel outside it. |
| `Frame.draw_poly(center, sides, radius, rotation, color)` | — | Fill a regular polygon with vertex `i` at angle `rotation + 360 * i / sides` degrees; fewer than 3 sides draw 3. |
| `Frame.draw_poly_lines(center, sides, radius, rotation, color)` | — | Draw the pixels of `draw_poly`'s fill that touch a pixel outside it. |
| `Frame.draw_text(text, x, y, size, color)` | — | Draw text in anti-aliased Atkinson Hyperlegible Next at `size` pixels to the em, kerned, with glyphs at quarter-pixel positions. The first line's top is at `y` and its baseline the face's ascent below that, rounded; `\n` starts a new line one line height (ascent, descent and line gap, rounded) lower. Each pixel blends the color src-over with its alpha scaled by the glyph's coverage. A size below 1 draws nothing. |
| `measure_text(text, size)` | `Int` | The width in pixels of `draw_text`'s widest line: its kerned advance, rounded. Lines break at `\n`; empty text, empty lines and a size below 1 measure 0. Needs no effect. |
| `Frame.draw_sprite(atlas, source, x, y)` | — | Copy the atlas's first region named `source` to `(x, y)`, clipped to the texture and the canvas and blended src-over; an unknown region or an atlas without a decoded texture draws nothing. |
| `check_collision_recs(rec1, rec2)` / `check_collision_point_rec(point, rec)` | `Bool` | Whether two rectangles overlap, or a point lies inside a rectangle. A rectangle includes its left and top edges and excludes its right and bottom edges, so rectangles that only touch do not collide. |
| `check_collision_circles(center1, radius1, center2, radius2)` / `check_collision_point_circle(point, center, radius)` | `Bool` | Whether two circles overlap or touch, or a point lies within `radius` of the center. |
| `check_collision_circle_rec(center, radius, rec)` / `check_collision_circle_line(center, radius, p1, p2)` | `Bool` | Whether a circle reaches a rectangle or the segment from `p1` to `p2`. |
| `check_collision_point_triangle(point, p1, p2, p3)` / `check_collision_point_poly(point, points)` | `Bool` | Whether a point lies strictly inside a triangle, or inside a polygon by the even-odd rule; fewer than 3 points enclose nothing. |
| `check_collision_point_line(point, p1, p2, threshold)` | `Bool` | Whether a point lies between `p1` and `p2` along the line's longer axis and within `threshold` pixels of the line, using a cross-product test. |
| `check_collision_lines(start1, end1, start2, end2)` | `Point?` | The point where two segments cross, or `None` when they are parallel or cross outside either segment. |
| `get_collision_rec(rec1, rec2)` | `Rect` | The overlap of two rectangles, or a zero rectangle when they do not overlap. |
| `gen_image_color(width, height, color)` / `gen_image_checked(width, height, checks_x, checks_y, first, second)` | `Image` | A new RGBA image of one color, or of `checks_x` by `checks_y` pixel checks starting with `first` at the top left. A side below 1 or above 16384, or more than 2^24 pixels, gives an image with no pixels. |
| `get_image_color(image, x, y)` | `Color` | The pixel at `(x, y)`, read back straight, or a zero color outside the image. |
| `image_clear_background(image, color)` / `image_draw_pixel(image, x, y, color)` | `Unit` | Set every pixel, or one pixel inside the image, to `color` premultiplied, alpha included. |
| `image_draw_rectangle(image, x, y, width, height, color)` / `image_draw_rectangle_rec(image, rec, color)` | `Unit` | Set the rectangle's pixels, clipped to the image and truncated to whole pixels. Less than one column still sets the first pixel unless the rectangle ends at the left edge, and less than one row still fills the first row. |
| `image_crop(image, rec)` | `Unit` | Keep the rectangle's pixels, clipped to the image and truncated to whole pixels. A rectangle starting past the image leaves it unchanged. One with no whole column or row leaves no pixels, and the truncated sides (not below 0) become the image's size. |
| `image_resize_nn(image, width, height)` | `Unit` | Nearest-neighbour resize with a 16.16 fixed-point source index. A size outside `gen_image_color`'s limits leaves the image unchanged. |
| `image_flip_vertical(image)` / `image_flip_horizontal(image)` / `image_rotate_cw(image)` / `image_rotate_ccw(image)` | `Unit` | Mirror the rows or columns, or turn the image a quarter turn; a turn swaps width and height. |
| `image_color_tint(image, color)` / `image_color_invert(image)` | `Unit` | On each pixel's straight color, stored premultiplied again: scale each channel by `color`'s channel / 255, rounded down, alpha included; or replace red, green and blue with 255 minus themselves. |
| `image_color_brightness(image, amount)` / `image_color_replace(image, color, replace)` | `Unit` | Add `amount`, clamped to -255..255, to each pixel's straight red, green and blue, capped at 255; a sum below 0 becomes 1. Or replace every pixel that holds `color` premultiplied, in all four bytes, with `replace`; a clear pixel holds every color of alpha 0. |
| `load_texture_atlas(path)` | `TextureAtlas -[FS]>` | Load an atlas and decode its PNG texture (every color type, bit depth and Adam7; sixteen-bit samples keep their high byte), premultiplied as it loads. A missing or malformed PNG leaves the atlas without a texture. |
| `Image` | type | An RGBA8 image held in memory, premultiplied: `width()`, `height()` and `pixels()` (four `U8` per pixel, row by row, straight, as `Frame.present` takes them). A width of 0 means no image. |
| `TextureAtlas` / `.name() -> String` / `.loaded() -> Bool` | type | Named regions of one decoded PNG texture; the atlas text's `atlas:` line or its file's stem, and whether the texture decoded. |
| `Window.load_image_from_screen() -> Image` | image | A copy of the canvas the frames draw on, as last drawn. |
| `Frame.is_key_down(key)` / `.is_key_up(key)` / `.is_key_pressed(key)` / `.is_key_released(key)` | `Bool` | Whether `key` is held during the frame, or went down or up since the previous one. |
| `Frame.is_mouse_button_down(button)` / `.is_mouse_button_up(button)` / `.is_mouse_button_pressed(button)` / `.is_mouse_button_released(button)` | `Bool` | The same for a `UIPointerButton`. |
| `Frame.get_mouse_x()` / `.get_mouse_y() -> Int` / `.get_mouse_position() -> Point` | position | The pointer's last position in pixels. |

#### Keyboards: `core.ui.xkb`

`core.ui.xkb` is the one XKB keyboard engine behind native windows, in pure
Jet ([`Core/Ui/Xkb.jet`](../../../Core/Ui/Xkb.jet)). It compiles an XKB
text-v1 keymap, the include-resolved text a Wayland compositor sends with
`wl_keyboard.keymap`. It then tracks the keyboard state as libxkbcommon
1.13.2 does: groups, levels, modifier and group actions, locks and latches,
and Compose sequences from the en_US.UTF-8 table. Keys are `core.term`'s
`Key`. A hostile keymap or modifier event gives only a typed
`UIError.Protocol`.

| Signature | Result | Description |
|---|---|---|
| `keymap(bytes: [U8]) -> Keymap UIError!` | keymap | Compile an XKB text-v1 keymap. |
| `Keymap.modifiers(depressed: Int, latched: Int, locked: Int, group: Int)` | — | Apply a `wl_keyboard.modifiers` event. |
| `Keymap.press(code: Int, down: Bool) -> (key: Key, text: String)` | key, text | Press or release evdev key `code`: its key and the text it types (`""` for none, for control characters and inside a Compose sequence). |
| `Keymap.repeats(code: Int) -> Bool` | Boolean | Whether holding the key repeats it; modifier keys do not. |
| `Keymap.codes() -> [Int]` | codes | The evdev codes of the keys with symbols at level 1 of group 1. |
| `Keymap.keysym(code: Int) -> Int` / `.keysym_name(sym: Int) -> String` | keysym | The key's keysym, as `xkb_state_key_get_one_sym`, and libxkbcommon's name for a keysym. |
| `Keymap.layout() -> String` / `.active(name: String) -> Bool` | facts | The active layout's name, and whether a real or virtual modifier (`"Shift"`, `"NumLock"`, `"LevelThree"`) is active. |
| `Repeat{rate:, delay:}.due(held_ms: Int) -> Int` / `.between(from_ms: Int, to_ms: Int) -> Int` | count | Key repeats from `wl_keyboard.repeat_info` due after a hold, or while it grows. |

#### Text: `core.ui.font`

`core.ui.font` reads TrueType (glyf) fonts and draws text into a window's
pixels, in pure Jet ([`Core/Ui/Font.jet`](../../../Core/Ui/Font.jet)): no
`#Unsafe`, no files and no OS calls. Rasterizing and painting are integer
arithmetic, so the native tiers and the web build paint the same bytes. The
built-in faces are Atkinson Hyperlegible Next Regular and Bold (SIL Open Font
License, `license()`). They cover Latin; the only Greek is Δ Ω μ π, and there
is no Cyrillic. Layout applies GPOS pair kerning or the kern table. There is
no GSUB, no mark positioning and no hinting, and text runs left to right.
Every fault in a font file is a typed `FontError{reason}`.

| Signature | Result | Description |
|---|---|---|
| `face(bytes: [U8]) -> Face FontError!` / `regular() -> Face` / `bold() -> Face` | face | Read a TrueType font, or take a built-in face. |
| `Face.glyph(scalar: Int) -> Int` / `.glyph_count()` / `.units_per_em()` / `.metrics(size_px)` / `.advance(id, size_px)` / `.outline(id)` | facts | The glyph for a character (0 when missing), and the face's metrics, advances and outlines. |
| `Face.layout(text: String, size_px: Float) -> TextRun` / `.wrap(text, size_px, max_width) -> [TextLine]` | layout | Place a line of glyphs with kerning, and break text into lines after spaces and hyphens. |
| `Face.raster(id: Int, size_px: Float, x_frac: Float) -> GlyphMask` | mask | A glyph's anti-aliased coverage, unhinted, at a sub-pixel position. |
| `cache() -> GlyphCache` / `cache_within(budget: Int)` / `GlyphCache.glyph(face, id, size_px, x_frac)` | cache | Masks kept per face, glyph, size and quarter pixel, within about `budget` bytes (CACHE_BYTES_MAX, 4 MiB, by default). |
| `blend(&pixels, width, height, stride_bytes, mask, x, y, argb, clip: UIRect)` | — | Paint a mask onto premultiplied ARGB8888 pixels (bytes B, G, R, A, `stride_bytes` per row) by core.ui's one blend rule, source-over on the sRGB bytes: the source covers `sa = (A * coverage + 127) / 255` of a pixel, and each byte `d` becomes `s + (d * (255 - sa) + 127) / 255`, with `s = (C * sa + 127) / 255` for a color byte and `sa` for alpha. Only pixels inside `clip` and the canvas change. |
| `draw_text(&pixels, width, height, stride_bytes, &cache, face, text, size_px, x, baseline_y, argb, clip) -> Int` | advance | Lay out and paint a line of text at quarter-pixel positions, and return its advance in whole pixels. |

### `core.reactive` — explicit signals and dependency tracking

`core.reactive` provides opt-in, dependency-tracked state. A `Signal<T>` holds
a value and version; `computed` and `derived` values track reads; an `Effect`
can be unsubscribed. The source carrier is immutable, while the runtime
provider supplies synchronized boxes and dependency tracking. See
[`Core/Reactive/Reactive.jet`](../../../Core/Reactive/Reactive.jet) and the
reactive conformance corpus.

| Signature | Result | Description |
|---|---|---|
| `signal<T>(value: T) -> Signal<T>` | signal | Create mutable reactive state. |
| `computed<T>(compute: fn() -> T) -> Computed<T>` | computed | Compute an initial value from a body. |
| `derived<T>(compute: fn() -> T) -> Derived<T>` | derived | Compute an initial derived value. |
| `autorun(body: fn()) -> Effect` | effect | Run a body once and retain an active effect record. |
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
Setting a synchronized signal updates the provider state, not the Jet carrier
binding, so its receiver call uses `count.set(value)` without an `&` mark. UI
button handlers are ordinary code arguments, `ui.button("Click", () -> { … })`
(D-TRAILBLOCK2); a bare `{ }` after the call is E0335.

### `core.reactive.loadable` — asynchronous value state

`core.reactive.loadable` models a value that is idle, loading, loaded, or
failed. The state carries the successful value or failure reason and provides a
retry transition; callers can inspect it without using sentinel values. Its
source is [`Core/Reactive/Loadable.jet`](../../../Core/Reactive/Loadable.jet).

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
bus. The source contract is [`Core/Event/Event.jet`](../../../Core/Event/Event.jet),
and the event conformance corpus covers listener lifetime, policy, and
async-result behavior.

`new<T>()` composes `with_policy<T>(policy_sync())` in Jet source.
`async_result` rejects a nonpositive capacity with `InvalidCapacity` before
allocating dispatch state. Listener cells, scope identities, and scheduler
resources remain provider-owned.

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
The module source is [`Core/Web/Web.jet`](../../../Core/Web/Web.jet).
`App` and `WebPage` are opaque compiler-owned carriers, not constructible source
records. The registered constructors and App builder methods retain the checked
route graph and its typed handlers across both compiler implementations.
The selected Apps runtime part emits their native implementations together with
the development-server and shared web-router dependencies.
`openapi` remains infallible at its public boundary: failures while collecting
checked route facts or rendering the document stop at the existing contract
boundary. The renderer consumes those facts instead of copying the route list.

| Signature | Result | Description |
|---|---|---|
| `app() -> App` | app | Create the default application carrier. |
| `page(title: String, body: String) -> WebPage` | page | Build a server-rendered page. |
| `form(Model, action: handler) -> WebFormTyped` | form | Derive typed inputs and the named action contract at compile time (D-WEBFORM1=A). |
| `on(path: String, title: String, handler: fn(WebEvent)) -[Browser]>` | — | Register a browser-facing handler. |
| `openapi(routes: HTTPMux or HTTPRouter) -> String` | text | Render OpenAPI from the checked facts of the same route value that is served. |
| `value(key: String) -[Browser]> String` | text | Read a browser value. |
| `storage() -> String` | text | Select the local storage namespace. |
| `auth(kind: String) -> Auth` | auth | Build an authentication carrier. |
| `auth_oauth(auth: Auth, client_id: String) -> Auth` | auth | Add OAuth client identity to an auth carrier. |
| `auth_routes(auth: Auth) -> [String]` | paths | Return authentication route paths. |
| `auth_show(auth: Auth) -> String` | text | Render the source auth summary. |
| `live(query, args...) -> Live<T, E>` | binding | Bind `query: fn(args...) -[DB.Read]> T E!` to one argument list. The arguments are checked as a call to `query`, evaluated once and kept by the binding; the same source and arguments return the existing binding. |
| `subscribe(source, args..., every: Duration? = None) -> Live<T, E>` | binding | Bind any named `source: fn(args...) -> T E!` to one argument list; with `every`, a read re-runs the source once the interval has passed since its last run. |
| `state(binding: Live<T, E>) -> Loadable<T, E>` | state | Read the binding's snapshot: `Loaded(value)` or `Failed(reason)`, running the source first when the binding is new, invalidated, or due to poll. |
| `push(source, args..., value: T)` | — | Publish `value` as the result of that source and argument list without running the source. |
| `invalidate(source)` / `invalidate(source, args...)` | — | Re-run every binding of `source` on its next read, or exactly the binding for that argument list. |
| `live_stats() -> String` | text | Report the registry's binding count, its 1024-binding limit, evictions, invalidations and value changes. |
| `sync(key: String, data: String) -> String` | text | Format a synchronized key/value update. |

`app` and `core.web` live bindings share one process-wide registry. A source
must be a named function, not a closure: its module path and name identify it
on the wire. `T` and `E` must be `Encode` and `Decode`, and the encoded
arguments tell bindings of one source apart (an argument type without
`Encode`, such as a database scope, does not). A stored value that equals the
previous one is not a change, so readers wake only when the value changed.
`Live<T, E>` is an identity, not a snapshot: reads, invalidation and
statistics always use the one registry. A full registry evicts its oldest
binding. The type is `core.web.Live`; `app` returns the same type and does
not declare its own, so a signature names it after `use core.web.[Live]`.

`core.web` delegates HTTP transport to `core.http`; route handlers should use
that typed request/response contract rather than opening sockets directly.
Storage adapters are named below because they are separate built modules.
The high-level `web.form` value keeps its compiler-owned typed-model identity
and `.set`/`.render` interface. The source `core.web.forms.typed` API remains
separate for callers that explicitly supply `WebFormInput` and an action name.
The source query carrier exposes the same receiver state/mutation operations as
its module functions. Mutating source query, form, table, virtual viewport, and
store calls take explicit write access. Source table pages keep typed row values
and separate stable row keys, with `total_rows`, `page_index`, `page_size`, and
`page_count` facts. Form action failures are a declared `#Error` domain.
A table holds its columns' projection functions and its server loader, which
cannot be duplicated, so its configuring and mutating calls (`with_column`,
`with_server_page`, `sort_by`, `paginate`, selection, focus, and page moves)
edit the table in place and return no second table.
`with_server_page` retains its typed `WebTableError!` loader without fetching.
Each server-mode `page_state` call passes the current requested boundary to
that loader, validates its row keys and boundary, and preserves the remote
`total_rows` instead of filtering or paginating the returned page again.
Explicit `#Target(JS)` functions and their nested closures are excluded from
native Rust artifacts; their browser callbacks remain in the web partition.

### `core.web.store` — transaction history

The Jet source store starts at generation zero and keeps an initial state plus
bounded transaction and event ledgers. A write records its action, changed fields,
before/after states, generation, and canonical transaction diff. Evicting an old
entry advances the initial state and adjusts the history cursor; a write after
navigation discards the abandoned future branch. `facts_json` reports retained
transaction/event lengths, history limit, cursor, generation, and capture state;
`event_json` reports the last transaction identity and diff without serializing
the generic state payload.

Store callbacks take no state parameter: they capture the store's rooted
reactive handles (D-DX-STORE1). `signal` and `state_signal` return the typed
`Signal<T>`, not a textual generation token. Writes made during a transaction
remain staged, are visible to that callback's subsequent reads, and notify
reactive observers only once at the outermost commit.


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

Outside a browser (`jet run`, a native build), each of `local` and `session`
is one process-wide in-memory store, and the two stay separate: `set`
replaces a key's value, `remove` and `clear` touch only their own store,
and the empty key is an ordinary key. The golden
`Examples/Apps/Web/StorageLocalSession.jet` shows this on O0, O1 and O2.

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
[`Core/Mem/Mem.jet`](../../../Core/Mem/Mem.jet).

Raw `*T` fields in physical aggregates use the selected target's pointer size,
alignment, and pointer-sized stride, even when the pointee's layout is opaque;
these facts do not grant dereference access or change the `#Unsafe` rules.

An allocation view borrows its allocator's storage. It cannot be returned,
stored, sent, or used after the allocator resets or closes: the checker reports
**E0631** for an escaping view and **E0632** for a view used after invalidation.
Copy a value out before leaving the allocator's region. `#Region(name)` creates
an explicit region when inferred scope is too broad or two allocators must
share a lexical lifetime.

Core's reserved foreign-byte carriers are not public pointer constructors.
They require an audited `#Unsafe` block: a borrowed String's byte address is
valid only while that same String remains alive and unmodified; a packed byte
buffer must be freed once with its original byte length. Unpacking copies
into owned bytes before a foreign buffer is invalidated. Empty buffers are
never dereferenced. A `[U8]` list's element storage must not be assumed to be
the C representation of a packed byte array.

```jet
use core.mem

run: fn() {
    #Region(scratch) {
        arena: mem.Arena.new()
        bump: mem.Bump.new(capacity: 256)
        first: arena.alloc(1)
        second: bump.alloc(2)
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
| `Fixed.over(bytes)` | allocator | Borrow one mutable byte buffer for the handle's scope. |
| `allocator.alloc(value)` | view | Store a value and return a scope-bound view. |
| `allocator.reset()` | — | Drop values while retaining reusable storage. |
| `close(allocator)` | — | Release the allocator resource permanently. |
| `address_of<T>(value: T) -> Int` | address | Return an inert address value. |
| `from_addr<T>(address: Int) -> *T` | pointer | `#Unsafe` function: reconstitute a pointer; every call sits in an audited `#Unsafe` region. |
| `volatile_read<T>(pointer: *T) -> T` | value | `#Unsafe` function: perform a volatile read; every call sits in an audited `#Unsafe` region. |
| `volatile_write<T>(pointer: *T, value: T)` | — | `#Unsafe` function: perform a volatile write; every call sits in an audited `#Unsafe` region. |
| `stream_store<T>(dst: &T, value: T)` | — | `dst = value`, written without filling the cache (a non-temporal store) where the target has one for the value's width, else an ordinary store. Safe. |

`Fixed` never grows or falls back to the heap. It reserves allocation and
reverse-drop space from opposite ends of its buffer and fails before moving a
cursor when they would collide. `reset` is rejected while allocation views are
live; after a valid reset, the same bytes can be reused. Fixed handles and
views cannot be returned, captured, stored, or sent across a task boundary.

`stream_store` is for a large output written once and not read again soon:
the writes skip the cache, so they do not evict data the program still needs.
On x86-64 a value of four or eight bytes is written with `movnti`; narrower
values and aggregates use an ordinary store. A function that streams fences
those writes (`sfence`) before each call and at its return, so callers,
callees and other threads see them in program order. Pad hot fields to the
cache line with `#Layout(c, align(#build.cpu.cache_line))`; the fact is 64 on
every target until Jet selects CPUs (D-CACHE-CONTROL1).

### `core.text.parse` — text parsing and scanning

`core.text.parse` contains destination-independent string operations and
Option-returning convenience parsers. `Int.parse` and `Float.parse` are
fallible destination-owned conversions; do not confuse their `Result` return
with the module's Option helpers. The implementation is in
[`Core/Text/Parse.jet`](../../../Core/Text/Parse.jet).

The builtin method `text.count(needle)` uses the shared `jet_unicode_count`
kernel in compile-time evaluation and at runtime. It counts non-overlapping,
exact UTF-8 matches without normalization; an empty needle returns zero,
including for empty text. This is a separate operation from the module function
`core.text.parse.count(text, needle)`, whose empty-needle contract differs.

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
| `is_blank(text)`, `is_graphic(text)`, `is_punctuation(text)`, `is_hex_digit(text)` | Boolean | ASCII forms of C `isblank`, `isgraph`, `ispunct` and `isxdigit`: space or tab; `!` through `~`; visible and neither a letter nor a digit; `0-9`, `a-f`, `A-F`. Empty text is false. |
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
[`Examples/Text/TextCursor.jet`](../../../Examples/Text/TextCursor.jet).

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
scanner examples are [`binary-reader.jet`](../../../Examples/Data/Binary/BinaryReader.jet)
and [`text-cursor.jet`](../../../Examples/Text/TextCursor.jet).

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

User-defined mixed arithmetic names its right operand in the hook declaration:
`impl Money.Add(Int) { ... }`. An untyped `impl Money.Add` is the same-type
hook, not an overload inferred from the method parameter. Add and multiply
hooks may opt into reverse lookup with `#Commutative`, as in
`impl Money.Mul(Int) #Commutative { ... }`; subtract and divide retain their
declared operand order. The exact foundations example exercises both operand
orders without changing its integer or rational representation.

Plain arithmetic on a fixed-width integer traps on overflow, and a typed-literal
conversion to a fixed width that can't hold every source value (`U8{count}`,
`I32{wide}`) stops with **E3010** when the value doesn't fit. Use one of the
explicit policies when wrapping or clamping is part of the contract:

| Form | Result | Description |
|---|---|---|
| `a + b`, `a - b`, `a * b`, `a / b` | `T` | Trap if a fixed-width result is out of range. |
| `wrapping(a + b)` | `T` | Wrap in the operand's width. |
| `saturating(a + b)` | `T` | Clamp to the operand's bounds. |
| `checked(a + b)` | `Option<T>` | Return no value on overflow. |
| `T{x}` | `T` | Stop with E3010 if `x` is outside `T`. |
| `wrapping(T{x})` | `T` | Keep the low bits of `x` in `T`'s width. |
| `saturating(T{x})` | `T` | Clamp `x` to `T`'s bounds. |
| `checked(T{x})` | `Option<T>` | Return no value when `x` is outside `T`. |
| `value.wrapping_add(other)` | `T` | Receiver form of wrapping addition. |
| `value.saturating_add(other)` | `T` | Receiver form of saturating addition. |
| `value.checked_add(other)` | `Option<T>` | Receiver form of checked addition. |

The wrappers accept exactly one integer arithmetic operation or one narrowing
conversion; another expression, including a conversion that can't fail,
reports **E1005**. A wrapper covers only the operation or conversion it is
written around. A written literal must fit its destination even inside a
wrapper (**E1003**, whose fix names the value the wrapper would give).
Integer types expose `MIN`, `MAX`, `count_ones`,
`count_zeros`, `leading_zeros`, and `trailing_zeros`. Float types expose
`INFINITY`, `NEG_INFINITY`, `NAN`, and `EPSILON`, plus `is_nan`, `is_infinite`,
and `is_finite`. Named bit methods preserve operand width. A shift keeps its
left operand's width, and its count may be any integer width (`U64{byte} << n`
with `n: U8`); a count past the value's width traps rather than leaking a
host-language panic.

`wrapping(T{x})` is the truncating conversion from `Int` for every
fixed-width integer type: it keeps the low bits of `x` in `T`'s width, two's
complement for a signed `T`.

```jet
low_bits: fn(wide: Int, deep: Int) {
    print(wrapping(U8{wide}))   // 112
    print(wrapping(U16{wide}))  // 4464
    print(wrapping(I32{deep}))  // -705032704
}

run: fn() {
    low_bits(70000, -5000000000)
}
```

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
[`Core/Net/Net.jet`](../../../Core/Net/Net.jet).

DNS failures carry the built-in `NetDNSError` inside `NetError.DNS`: `NotFound`
retains the queried name, while `Failure` retains the resolver's message.

`SocketAddr` and `UDPPacket` retain the native provider carrier so resolver and
socket results keep the nominal types accepted by transport operations. Jet owns
IP parsing/canonical rendering, socket-text parsing/rendering, and lossy UDP
UTF-8 decoding. Compiler-private, audited leaves transfer only native address
representation; they do not choose validation, defaults, or error policy.
`SRVRecord` is explicitly assembled from resolver fields.

`socket_addrs` checks the port range `0..=65535` and parses numeric IPv4/IPv6
addresses in Jet; a numeric host is its one address, without a lookup. Ordinary
host names use the platform resolver, including its configured name-service
sources; every answer is returned in platform order, without sorting,
deduplication or filtering. An empty successful answer reports the existing
“no address” network error rather than an empty list, so the list is never
empty. `socket_addr` is the first element of `socket_addrs`. Explicit
`DnsResolver.at` lookups remain separate from ordinary platform resolution.

| Signature | Result | Description |
|---|---|---|
| `gethostname() -[Env, FS.Read]> String` | text | Read the local host name: `$HOSTNAME`, else `/etc/hostname`. |
| `ip_addr(text: String) -> IPAddr !NetError` | address | Parse an IP address. |
| `ip_to_string(addr: IPAddr) -> String` / `ip_is_ipv4(addr: IPAddr) -> Bool` | text/Boolean | Render an address or test its family. |
| `socket_addrs(host: String, port: Int) -[Net, Time.Wait]> [SocketAddr] !NetError` | addresses | Every address the host names, in resolver order. |
| `socket_addr(host: String, port: Int) -[Net, Time.Wait]> SocketAddr !NetError` | address | The first of `socket_addrs`. |
| `socket_addr_parse(text: String) -> SocketAddr !NetError` | address | Parse a rendered socket address. |
| `socket_host(addr: SocketAddr) -> String` / `socket_port(addr) -> Int` | scalar | Read address components. |
| `socket_to_string(addr: SocketAddr) -> String` / `socket_type(stream: TCPStream) -> String` | text | Render an address or identify a stream. |
| `tcp_listen(addr: String) -[Net]> TCPListener !NetError` / `tcp_listen_addr(addr: SocketAddr) -[Net]> TCPListener !NetError` | listener | Bind a TCP listener. |
| `tcp_accept(listener: TCPListener) -[Net, Time.Wait]> TCPStream !NetError` | stream | Accept one connection. |
| `tcp_connect(addr: String) -[Net, Time.Wait]> TCPStream !NetError` / `tcp_connect_addr(addr: SocketAddr) -[Net, Time.Wait]> TCPStream !NetError` | stream | Connect to a TCP endpoint. |
| `tcp_connect_timeout(addr: SocketAddr, timeout_ms: Int) -[Net, Time.Wait]> TCPStream !NetError` | stream | Connect with a nonnegative timeout. |
| `tcp_connect_happy(host: String, port: Int, timeout_ms: Int) -[Net, Time.Wait]> TCPStream !NetError` | stream | Connect through the address-family race. |
| `create_connection(host: String, port: Int) -[Net, Time.Wait]> TCPStream !NetError` / `create_server(host, port) -[Net]> TCPListener !NetError` | endpoint | Build host/port TCP endpoints. |
| `send(stream: &TCPStream, text: String) -[Net, Time.Wait]> Int !NetError` | count | Write text through the caller's exclusive stream window. |
| `tcp_read(stream: &TCPStream) -[Net, Time.Wait]> String !NetError` / `tcp_read_bytes(stream, n) -[Net, Time.Wait]> [U8] !NetError` | text/bytes | Read from a TCP stream. |
| `tcp_read_text(stream, n) -[Net, Time.Wait]> String !NetError` | text | Read and decode up to `n` bytes. No byte is lost: a character the read cuts stays for the next read, and one that starts the read is read whole, up to 3 bytes past `n`. Bytes that are not UTF-8, and a character the end of the stream cuts, fail each text read and stay for `tcp_read_bytes`. |
| `stream.read_into(buffer: &[U8], limit)` / `stream.read_into(buffer, limit, deadline:)` on `TCPStream` | `Int !NetError` | Append one read of up to `limit` bytes (at most 16 MiB) to the caller's `buffer` and return the count; 0 is the end of the stream, and a limit below one is `.InvalidInput`. The bytes go straight into the buffer, so a loop that clears and reuses one buffer allocates nothing per read. A carry an earlier text read left is appended first. |
| `tcp_write(stream, text: String) -[Net, Time.Wait]> Unit !NetError` / `tcp_write_bytes(stream, bytes) -[Net, Time.Wait]> Int !NetError` | unit/count | Write text or bytes. |
| `tcp_write_all_bytes(stream, bytes) -[Net, Time.Wait]> Unit !NetError` / `tcp_write_text(stream, text) -[Net, Time.Wait]> Unit !NetError` | unit | Write all bytes or text. |
| `tcp_shutdown(stream, how: NetShutdown) -[Net]> Unit !NetError` / `tcp_close(stream) -[Net]> Unit !NetError` | — | Shut down or close a stream. |
| `tcp_reply(stream, status: String, body: String) -[Net, Time.Wait]> Unit !NetError` | — | Consume a stream, write one checked reply with a three-digit status, and close it. |
| `tcp_ready(stream, interest: NetReadyInterest, deadline_ms: Int) -[Net, Time.Wait]> NetReady !NetError` | readiness | Wait for a readiness interest until a deadline. |
| `ready_readable(ready: NetReady) -> Bool` / `ready_writable(ready) -> Bool` | Boolean | Inspect readiness. |
| `tcp_local_addr(stream) -[Net]> String !NetError`, `tcp_peer_addr(stream) -[Net]> String !NetError` | text | Read rendered stream endpoints. |
| `tcp_local_socket_addr(stream) -[Net]> SocketAddr !NetError`, `tcp_peer_socket_addr(stream) -[Net]> SocketAddr !NetError`, `listener_local_socket_addr(listener) -[Net]> SocketAddr !NetError` | address | Read typed stream/listener endpoints. |
| `set_timeout(stream, timeout_ms: Int) -[Net]> Unit !NetError` | — | Set a nonnegative timeout on the caller's stream, without copying the handle. |
| `set_read_timeout(stream, timeout_ms: Int) -[Net]> Unit !NetError` / `set_write_timeout(stream, timeout_ms: Int) -[Net]> Unit !NetError` | — | Set read or write timeouts in milliseconds. |
| `nodelay(stream) -[Net]> Bool !NetError` / `set_nodelay(stream, on: Bool) -[Net]> Unit !NetError` | option | Read or set TCP no-delay. |
| `ttl(stream) -[Net]> Int !NetError` / `set_ttl(stream, hops: Int) -[Net]> Unit !NetError` | option | Read or set the TTL. |
| `sendfile(stream, path: String) -[Net, Time.Wait]> Int !NetError` | count | Send file bytes through a TCP stream. |
| `udp_bind(addr: String) -[Net]> UDPSocket !NetError` / `udp_bind_addr(addr: SocketAddr) -[Net]> UDPSocket !NetError` | socket | Bind a UDP socket. |
| `udp_local_addr(socket: UDPSocket) -[Net]> SocketAddr !NetError` / `udp_set_timeout(socket, timeout_ms: Int) -[Net]> Unit !NetError` | address/— | Inspect a UDP endpoint or set its timeout. |
| `udp_send_to(socket, text, addr) -[Net, Time.Wait]> Int !NetError` / `udp_send_bytes_to(socket, bytes, addr) -[Net, Time.Wait]> Int !NetError` | count | Send text or bytes to a datagram address. |
| `udp_recv_from(socket, limit) -[Net, Time.Wait]> UDPPacket !NetError` / `udp_receive(socket, limit) -[Net, Time.Wait]> UDPPacket !NetError` | packet | Receive a bounded datagram. |
| `udp_packet_data(packet)`, `udp_packet_bytes(packet)`, `udp_packet_addr(packet)` | scalar | Read packet text, bytes, or address. |
| `udp_packet_original_len(packet) -> Int` / `udp_packet_truncated(packet) -> Bool` | scalar | Inspect datagram length and truncation. |
| `unix_listen(path: String) -[Net]> UnixListener !NetError` / `unix_connect(path) -[Net, Time.Wait]> UnixStream !NetError` | endpoint | Use Unix-domain sockets. |
| `unix_accept(listener) -[Net, Time.Wait]> UnixStream !NetError` / `unix_read(stream) -[Net, Time.Wait]> String !NetError` | stream/text | Accept or read a Unix stream. |
| `unix_write(stream, text) -[Net, Time.Wait]> Unit !NetError` / `unix_read_bytes(stream, n) -[Net, Time.Wait]> [U8] !NetError` | unit/bytes | Write text or read bytes through the caller's exclusive stream window. |
| `stream.read_into(buffer: &[U8], limit, deadline:)` on `UnixStream` | `Int !NetError` | The TCP `read_into` with an explicit deadline, retrying an interrupted read. |
| `unix_write_all_bytes(stream, bytes) -[Net, Time.Wait]> Unit !NetError` / `unix_shutdown(stream, how) -[Net]> Unit !NetError` / `unix_close(stream) -[Net]> Unit !NetError` | — | Write all bytes or close the same Unix stream. |
| `unix_send(&stream, bytes: [U8], fds: [core.sys.Fd]) -[Net, Time.Wait]> Unit !NetError` | — | Send every byte with `fds` attached (SCM_RIGHTS) to the first byte; at least one byte, at most 253 descriptors. The peer gets its own copies. Partial sends and `EINTR` resume. |
| `unix_receive(&stream, max: Int) -[Net, Time.Wait]> UnixMessage !NetError` | message | At most `max` bytes (1 B to 16 MiB) of one message and the descriptors that rode with it, each an owned close-on-exec `core.sys.Fd`. A message whose descriptors were cut short (`MSG_CTRUNC`) closes the ones that arrived and fails with `.Protocol`. Empty bytes and no descriptors mean the peer closed. |
| `unix_wait(&stream, timeout_ns: Int) -[Net, Time.Wait]> Bool !NetError` | ready | Wait on the calling thread until the stream has something to read (true) or `timeout_ns` passes (false); a negative timeout waits without limit. Interrupted waits resume with the time left. |
| `dns_a(name: String, ms: Int) -[Net, Time.Wait]> [IPAddr] !NetError` / `dns_aaaa(name, ms) -[Net, Time.Wait]> [IPAddr] !NetError` | addresses | Resolve IPv4 or IPv6 records. |
| `dns_a_at(server: String, name: String, ms: Int) -[Net, Time.Wait]> [IPAddr] !NetError` / `dns_aaaa_at(server, name, ms) -[Net, Time.Wait]> [IPAddr] !NetError` | addresses | Resolve A or AAAA records through an explicit server. |
| `dns_txt(name: String, ms: Int) -[Net, Time.Wait]> [String] !NetError` / `dns_ptr(addr, ms) -[Net, Time.Wait]> [String] !NetError` | records | Resolve TXT or PTR records. |
| `dns_txt_at(server: String, name: String, ms: Int) -[Net, Time.Wait]> [String] !NetError` / `dns_srv_at(server, name, ms) -[Net, Time.Wait]> [DNSSRV] !NetError` | records | Resolve TXT or SRV records through an explicit server. |
| `dns_srv(name: String, ms: Int) -[Net, Time.Wait]> [DNSSRV] !NetError` | records | Resolve SRV records. |
| `dns_srv_target(record: DNSSRV) -> String` / `dns_srv_port(record) -> Int` / `dns_srv_priority(record) -> Int` / `dns_srv_weight(record) -> Int` | fields | Inspect an SRV record. |
| `gethostbyname(name: String) -[Net, Time.Wait]> String !NetError` / `gethostbyaddr(addr) -[Net, Time.Wait]> String !NetError` | hosts | Read one host name/address result. |
| `getservbyname(name: String) -[Net, Time.Wait]> Int !NetError` / `getservbyport(port: Int) -[Net, Time.Wait]> String !NetError` | service | Resolve a service name or port. |
| `error_operation(err: NetError) -> String` / `error_message(err) -> String` / `error_address(err) -> ?String` / `error_name(err) -> ?String` / `error_os_code(err) -> ?Int` | diagnostics | Inspect structured network-error details. |
| `addressfamily(addr: SocketAddr) -> String` | family | Report `IPv4` or `IPv6`. |

The `*_at` DNS forms take an explicit server, and `error_operation`,
`error_message`, `error_address`, `error_name`, and `error_os_code` inspect a
`NetError`. Ports are checked in `0..=65535`; timeout and deadline values are
nonnegative milliseconds. A blocking socket call declares `Net` and
`Time.Wait`, so the suspension boundary remains visible.

Effects follow the existing per-function Core rows, not a module-wide waiting
annotation. TCP listen and address getters, socket options, UDP bind/options,
and Unix listen/shutdown/close require `Net` without `Time.Wait`. Accept,
connect, byte transfer, DNS, and the other listed waiting operations retain
their existing `Time.Wait` leaf. Reserved raw aliases mirror those same rows;
moving public validation into Jet does not widen the authority contract.
TCP reply is Jet end to end: it frames the plain-text HTTP/1.1 reply and
writes it with the stream's own `write_all`, then shuts down the write half.
TCP option access and UDP bind enter their Jet public functions before the
corresponding reserved `__set_*`, `__nodelay`, `__ttl`, and `__udp_bind*`
leaves. Raw leaves retain existing native error mapping, borrow/move
conventions, and interpreter-route availability.
UDP address, timeout, send, and receive operations and Unix listen, accept,
read, byte-write, shutdown, and close also enter Jet source first. Their
reserved raw leaves preserve exactly the public operation's existing effect
row and supported interpreter/native tiers.
The HTTP client is Jet end to end: `get`, `post`, `request`, and `send` build
an `HTTPRequest` record, frame HTTP/1.1 over `core.net` TCP or `core.net.tls`,
and decode the status line, headers, chunked/length framing, and gzip into an
`HTTPResponse` record. `HTTPRequest`, `HTTPResponse`, and `HTTPBodyChunks` are
Jet records in `Core/Http/Http.jet`; their accessors and builders are Jet
methods, and `core.http.parse` reuses the Jet request parser. The HTTP server
is Jet end to end as well: `HTTPMux`, `HTTPHandler`, `HTTPServer`, and
`HTTPShutdownReport` are records in `Core/Http/Server.jet`, which accepts over
`core.net` TCP, frames requests and responses, routes, and serves static files.
Its only reserved leaves read the wall clock for `Date` and stat, canonicalize,
and read files for static serving, all under the Net effect.
HTTP router creation and dispatch, TLS byte I/O, and WebSocket connect/upgrade are also
source-owned; the reserved transport/carrier leaves keep their old effects
(including no new WebSocket effect) and tier support.
Reserved Unix connect and TLS client handshake leaves use full positional
signatures inside Core. Their public overload selection, implicit option
defaults, and escaping callback admission still precede source forwarding;
registration of a raw alias alone does not replace those public admission
checks.

DNS error classification is Jet source policy. Reserved Core-only
`__dns_*_lookup` leaves call the existing resolver and return its raw string
cause; Jet preserves not-found, cancellation, protocol, timeout, and generic
failure precedence and caller-name context. Public signatures and unsupported
tier behavior are unchanged; raw leaves keep the same `Net` and `Time.Wait`
requirements as their public operations.

`unix_connect` retains the native entry so its ratified `deadline:` overload is
not narrowed to a path-only source wrapper. Unix and TLS byte/text wrappers use
the native access contract: reads and writes borrow the named stream with `&`,
not a copied local handle.

### `core.net.ip` — IPv4/IPv6 values and address classes

`core.net.ip` parses, renders and classifies `IPv4` and `IPv6` values and IPv4
networks. The implementation is [`Core/Net/Ip.jet`](../../../Core/Net/Ip.jet).
Two address classes are distinct:

- **Reachable** (`is_global`, `ipv6_is_global`) follows the IANA IPv4 and IPv6
  Special-Purpose Address Registries (IPv6 registry updated 2025-10-09): an
  address is global unless a registry row marks it not globally reachable,
  with the more-specific reachable rows (for example `2001:3::/32` and
  `2001:20::/28`) as exceptions. 6to4 `2002::/16` and NAT64 `64:ff9b::/96`
  are reachable; IPv4-mapped `::ffff:0:0/96`, `64:ff9b:1::/48`, Teredo inside
  `2001::/23`, documentation `2001:db8::/32` and `3fff::/20`, and `5f00::/16`
  are not.
- **Safe to fetch** (`is_public_destination`, `ipv6_is_public_destination`)
  is stricter and requires the reachable class as well. IPv4 also refuses
  `0/8`, `10/8`, carrier-grade NAT `100.64/10`, `127/8`, `169.254/16`,
  `172.16/12`, all of `192.0.0/24`, `192.0.2/24`, `192.168/16`,
  benchmarking `198.18/15`, `198.51.100/24`, `203.0.113/24` and everything
  from `224/4` up. An IPv4-mapped IPv6 address takes the IPv4 rule; any other
  IPv6 address must be in `2000::/3` (refusing every NAT64 prefix) and must
  not be 6to4 or Teredo.

| Signature | Result | Description |
|---|---|---|
| `is_global(addr: IPv4) -> Bool` / `ipv6_is_global(addr: IPv6) -> Bool` | Boolean | Reachable per the IANA registries. |
| `is_public_destination(addr: IPv4) -> Bool` / `ipv6_is_public_destination(addr: IPv6) -> Bool` | Boolean | Safe to fetch: the strict outbound class. |

### `core.net.tls` — TLS stream wrapper

`core.net.tls` upgrades a connected TCP stream to a verified TLS 1.2/1.3
stream. It is Jet source over the Jet TLS engine (Core/Net/SSL, translated from
Mbed TLS 3.6.7); configuration carriers are validated before network use. See
[`Core/Net/Tls.jet`](../../../Core/Net/Tls.jet).

| Signature | Result | Description |
|---|---|---|
| `ClientConfig.default()` | `ClientConfig` | Start with system trust and inclusive TLS 1.2–1.3 version bounds. |
| `ClientConfig.default().with_alpn(protocols)` | `ClientConfig !IOError` | Validate and offer the configured ALPN protocol list. |
| `RootCertificates.from_pem(bytes)` | `RootCertificates !IOError` | Validate a custom PEM root bundle before network use. |
| `ClientIdentity.from_pem(cert_chain: bytes, private_key: bytes)` | `ClientIdentity !IOError` | Validate a PEM certificate chain and matching private key. |
| `ClientConfig.default().with_trust(policy)` | `ClientConfig !IOError` | Select `.System`, `.SystemPlus(roots)`, or `.CustomOnly(roots)` trust. |
| `ClientConfig.default().with_client_identity(identity)` | `ClientConfig !IOError` | Add a validated mTLS client identity to an immutable configuration. |
| `ClientConfig.default().with_version_bounds(min: version, max: version)` | `ClientConfig !IOError` | Select inclusive `.TLS12` / `.TLS13` bounds; reversed bounds fail before network use. |
| `tls.client(tcp, server_name:, config:, deadline:)` | `TLSStream !NetError` | Consume the connected TCP stream, verify the server name, apply configuration, and use the earliest handshake deadline. |
| `client(host: String, port: Int)` | `TLSStream !IOError` | Convenience form that connects and verifies a host and port. |
| `read(stream: &TLSStream, n: Int)` / `read_text(stream, n)` | `[U8] !IOError` / `String !IOError` | Read bounded TLS bytes or checked UTF-8 text. Like `tcp_read_text`, `read_text` loses no byte: a cut character stays for the next read, and bytes that are not UTF-8 fail the read and stay for `read`. |
| `write(stream: &TLSStream, bytes: [U8])` / `write_all(stream, bytes)` | `Int !IOError` / `!IOError` | Write application bytes, partially or completely. |
| `write_text(stream: &TLSStream, text)` / `close(stream)` | `!IOError` | Write text or send close-notify on the same handle; repeated close is harmless. |
| `stream.peer_identity()` | `TLSPeerIdentity` | Read the verified server name, certificate chain, cipher suite, and negotiated TLS version. |
| `stream.read(limit, deadline:)` / `stream.write_all(bytes, deadline:)` | matching results | Apply explicit per-call deadlines on the same TLS handle. |
| `stream.read_into(buffer: &[U8], limit, deadline:)` | `Int !IOError` | Append up to `limit` decrypted bytes to the caller's `buffer` and return the count; 0 is close-notify, and a limit below one is `IOError.InvalidInput`. |
| `stream.close_write(deadline:)` | `!IOError` | Flush close-notify and close only writes while reads continue. |

TLS handshake, read, write, and close-notify drive the engine over the consumed
socket's nonblocking descriptor and wait through `TCPStream.ready`. Handshake
failures use `NetError`; stream failures use the `IOError` tree, including
cancellation, timeout, closed, and protocol errors. A peer that closes the
socket without close_notify is a protocol (truncation) error, not end of stream.

`TLSCertificate` and `TLSPeerIdentity` are plain Jet records parsed from the
verified chain: DER bytes, SHA-256 of the certificate and its SubjectPublicKeyInfo,
DNS subjectAltNames, validity in Unix milliseconds, and RFC 4514-style subject
and issuer names.

The consumed-stream overload is distinct from `client(host, port)`: using
`tls.client(tcp, server_name:, config:, deadline:)` keeps the immutable
configuration and returns `NetError`. System trust reads `SSL_CERT_FILE` /
`SSL_CERT_DIR` when set and otherwise the first distribution bundle that loads.
`RootCertificates.from_pem` and `ClientIdentity.from_pem` validate through the
same engine used by the handshake before any TCP stream is consumed. A
custom-only trust policy never loads system roots. The `core_net_tls` testkit
cases exercise that path over loopback against the checked-in local CA.

### `core.db` — checked database boundaries

`core.db` exposes connections, pools, scoped SQL, policy checks, transactions,
migrations, and typed row decoding. SQL interpolation binds values; raw SQL
requires the explicit `SQL.raw` escape. Database operations carry `DB` (and,
when they wait, `Time.Wait`) effects. The module source is
[`Core/Db/Db.jet`](../../../Core/Db/Db.jet).
The backend-neutral driver/policy boundary keeps query operations and the user policy attached to the typed scope (`D-DBDRIVER1=A`; `D-DBPOLICY-BIND1=A`).
A row policy constrains only the rows of its own table: inside a policy scope,
SQL that targets no table (`SELECT 1`) or another table runs unchanged, SQL on
the policy's table must name it as its one simple target and gets the policy
predicate, and SQL that reads the policy's table any other way (a join or
subquery from another table) is refused.

| Signature | Result | Description |
|---|---|---|
| `open(url: String) -[DB]> DBConnection` | connection | Open a database URL. |
| `open_memory() -[DB]> DBConnection` | connection | Open an in-memory database. |
| `pool(url: String, max: Int) -[DB]> DBPool DBError!` | pool | Create a bounded connection pool (`DBPool` → `DBLease`, `DBPoolReceipt`). |
| `policy(table: String, expression: String) -[DB]> RowPolicy DBError!` | policy | Compile a checked row-access policy. |
| `policy_audit(scope: DBScope) -[DB]> String` | text | Inspect policy decisions for a scope. |
| `policy_allows(policy: RowPolicy, user: String, owner: String) -[DB]> Bool` | Bool | Check one row by hand with the meaning queries enforce: `true` allows every valid user, `owner == user` only the owner; a blank or control-bearing user or owner never matches (D-DBPOLICY-CHECK1). |
| `policy_show(policy: RowPolicy) -[DB]> String` | text | Render `RowPolicy(table=<table>, expr=<canonical expression>)`. |
| `row_value(row: [String: DBValue], column: String) -[DB]> DBValue DBError!` | value | Read an untyped row value. |
| `row_int(row: [String: DBValue], column: String) -[DB]> Int DBError!` / `row_float(row, column) -[DB]> Float DBError!` | scalar | Read numeric row values. |
| `row_text(row: [String: DBValue], column: String) -[DB]> String DBError!` / `row_bool(row, column) -[DB]> Bool DBError!` | scalar | Read text or Boolean row values. |
| `decode<T: Decode>(row: [String: DBValue]) -[DB]> T [FieldError]!` | `T` | Decode a row into a typed value. A field's column is its `Rename(db:)` name, else its shared name. |
| `transaction(scope: DBScope, name: String, steps: [SQL]) -[DB, Time.Wait]> Int DBError!` | count | Execute a checked transaction. |
| `migrate(scope: DBScope, name: String, steps: [SQL]) -[DB, Time.Wait]> Int DBError!` | count | Apply a migration under its checksum. |
| `scope.live(sql: SQL) -[DB.Read]> Live<[[String: DBValue]], DBError> DBError!` | binding | Bind one policy-checked `SELECT` in the shared `core.web` live registry; read it with `core.web.state`. |

Scoped connection and pool handles provide checked `query`, `query_one`,
`execute`, lease, commit, rollback, and close operations. A policy expression
is intentionally small: the supported forms are `true` and `owner == user`
with a table identifier. Policy text has a bounded size. Schema and transaction
control belong to the migration/control path, not to arbitrary interpolated
application values. Migrations record their name and checksum so a changed
migration cannot silently replace an applied one.

`DBScope` implements the prelude trait `Driver` (`query`, `query_one`,
`execute`, `begin`, `commit`, `rollback`), which a generic bound names bare:
`count: fn<T: Driver>(conn: T)` takes any scope and calls its methods.

`DBScope.live` (D-LIVEQUERY1, #1158) refuses any statement other than one
`SELECT` and applies the scope's policy before binding, so a refused query
fails at the call. The binding is identified by the scope's user, the
policy-applied SQL and its bound values: the same query from the same user
returns the same binding, and another user's gets its own. `core.web.state`
runs the query on first read and again, with the policy applied anew, after a
scope write (`execute`, `transaction`, `migrate`) to the table it reads, or
after a rollback that undid one. A write to another table leaves it current.
No WebSocket transport carries these results yet (core.net.ws, #4957), so
`live_stats()` keeps reporting `ws_pushes=0`.

`DBPool.acquire(deadline:)` admits waiters first come, first served. A waiter
leaves the queue when it gets a lease, when its deadline passes, when the pool
starts draining, or when its task is cancelled at the wait, so a cancelled
acquire never blocks the waiters behind it. `DBLease.close()` returns the
connection once; a second close does nothing. While the pool drains it refuses
new leases (`database pool is draining`) but still takes back the ones out.
Statements themselves have no cancel operation; that and any engine other
than SQLite are separate owner choices.

Rows are inspectable `[String: DBValue]` maps. The closed value carrier has
`Null`, `Int`, `Float`, `Text`, `Bool`, and `Blob([U8])` variants; SQLite Boolean
affinity returns integer `0`/`1`, which the Boolean reader accepts. Blob reads
preserve every byte, including empty blobs, without text conversion. `DBError`
retains the driver's message rather than exposing a raw engine pointer.

Pool receipts remain opaque values produced by a pool. Their equality and
Show/Display/Debug projection are source-owned: each projection preserves the
`db.pool lifecycle=...` text and the complete ordered set of pool counters.

`open`, `open_memory`, `pool`, `policy`, `policy_audit`, `transaction`, and
`migrate` enter through ordinary Core source functions. Their reserved
Core-internal provider leaves retain the existing SQLite host bridge during
native-driver admission; this changes neither public signatures nor engine
identity. Pool `max` remains a required labeled argument. Transaction and
migration retain both `DB` and `Time.Wait`, including at the provider boundary.

The existing registered Rust SQLite bridge remains the raw host boundary under
the Jet source policy until runtime cutover. The separate CFFI `__Driver`
adapter is held; it is not part of this source-owned Core integration.

### `core.db.sql` — pure SQL grammar and row selection

[`Core/Db/Sql.jet`](../../../Core/Db/Sql.jet) is the canonical public SQL
kernel. Import it with `use sql: core.db.sql`; the former Core-private
runtime module has been relocated, without a compatibility module or re-export.
It carries no database connection or raw-SQL execution authority.

`parse_sql_query` produces a `JetSQLQuery` for `SELECT`, `WHERE`, `GROUP BY`,
`ORDER BY`, and `LIMIT`, including `COUNT`, `SUM`, `AVG`, `MIN`, and `MAX`.
`execute_sql_rows` validates field references and projects callback-provided
`JetSQLValue` cells into `JetSQLResultRow` values. The query's projection and
grouping carriers are `JetSQLAggregate`, `JetSQLSelectItem`, and `JetSQLGroupKey`.
Projection items expose `output_name` and `field`; `JetSQLValue.literal`
constructs a scalar from an SQL literal.

Core's SQL tests also use `sql_tokens`, `unquote`, `identifier`,
`compare_sql_cell`, `sql_aggregate_value`, and `sql_output_value`.
Schema traversal, alternate index selection, parser cursors, numeric helpers,
fixed-wide accumulation, grouping-key encoding, and sorting carriers remain
in its `priv internal: module`, rather than enlarging the public surface.
The enclosing SQL file can call those helpers and name the sorting carrier as
`internal.SQLOrderedRow`; this is ordinary enclosing-module access, not a
Core-runtime privilege. Inline-module function imports use `use internal.[…]`;
member types use their qualified names instead of that function import list.
The lifted declarations retain their private visibility for other files.

### `core.compute` — checked tensors and explicit devices

`core.compute` provides dense tensors, shape-checked operations, sparse CSR
helpers, value-based differentiation, linear algebra, FFT, and serialization.
`Auto` selects the CPU reference path; an explicit unsupported device returns a
typed capability error rather than silently falling back. The source is
[`Core/Compute/Compute.jet`](../../../Core/Compute/Compute.jet), with executable
examples under [`Examples/Tooling/`](../../../Examples/Tooling/).
This is one ranked Tensor operation family with an explicit placement contract (`D-COMPUTE1=D`; `D-COMPUTE-TYPE1=D`; `D-COMPUTE-PLACE1=D`). Gradient and value-and-gradient calls remain explicit, checked value-based transforms (`D-COMPUTE-GRAD1=E`).
The registered compute vocabulary is the sole nominal owner of tensors,
devices, streams, and transform receipts. Core source imports that vocabulary;
it does not declare a second storage record or synthesize empty-record equality.
Display, equality, checked serialization, device placement, and SIMD receipts
use the real compute provider in both compiler halves. `profile_show` reports
the dispatch and vector width that actually execute, not a hardcoded scalar
receipt. The source LU solver reads tensors through `shape`, `to_list`, `device`,
and `placement`, and does not construct replacement tensor storage.
Tensor bracket write windows retain the checked range's inclusive/exclusive
end and borrow the owner's first-axis storage. Their native call receives
those bounds and the enclosing source location, rather than a copied slice.

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
| `value_and_gradient(pred, target) -> VJPRun !ComputeError`, `vjp(pred, target) -> VJPRun !ComputeError` | result | Evaluate the fixed MSE value and gradient. |
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
[`Core/Service/Service.jet`](../../../Core/Service/Service.jet); the service
runtime model is also exercised by the service examples and tests.
The tree is the single public topology boundary; workers, delivery, and restart policies remain typed rather than string-keyed (`D-SERVICE1=D`).

The whole service runtime is Jet source: no native service runtime, handle or
route remains. It reaches the host only through the Core-private `__host_*`
leaves (wall clock, entropy, sleep, and file metadata, read, append, create,
rename, sync, remove and mode operations). The process-wide endpoint registry,
pending-delivery registry, durable-store gate and state sequence are Core
Process Shared sites (`Docs/spec/stdlib-api-laws.md`, "Internal Core Shared
publication"). Service calls name the file and wait effects they reach
(`D-SERVICE-EFFECTS1=A`): the `__host_*` file leaves carry `FS.Read` or
`FS.Write`, the sleep leaves carry `Time.Wait`, and a service call's row is
inferred from its Core source body like any other call. `service.tree` stays
`-[]>`; `start` and the workflow waits reach `FS.Read`, `FS.Write` and
`Time.Wait`. A cancelled or deadline-blown
`workflow.sleep` ends its wait without unwinding, so the workflow records
`sleep-cancelled` or `sleep-deadline` before the call fails.

Restart/delivery policy constructors and runtime retention projection are Jet
policy. `state_store` requires `FS.Read`: it inspects the target and lexical
ancestors with metadata-only reads, without normalizing away `..`, refusing
symbolic links and unsuitable file kinds while allowing absent paths.
Generation/event/dead-letter getters read the tree's own fields. The metadata
read does not acquire a Windows identity handle; constructor validation
therefore adds no identity-open failures.

`ServiceError` and `DeliveryReceipt` are ordinary Jet enum/record values.
All ten error cases carry a String; their service display is the message alone.
Receipts retain every original field, including authority and signature, and
their service display preserves the existing field order and labels.
`TaskOutcome` retains Finished, Panicked(String), Cancelled and DeadlineBlown:
display uses the raw panic reason, while Debug quotes and escapes that String.
`core.rt.RealtimeReceipt` preserves the eight scalar provider facts in their
original positional order. `Delivery` shows exactly `Delivery(` + its ID + `)`.
`Delivery.show()` and `DeliveryReceipt.show()` return that same display text.

Restart/delivery/state-empty selection and dead-letter draining enforce their
original validation order. The signing key remains the exact 32 bytes decoded
from the current `sa-` authority token, with no key cache.
The Core-internal `__authority_signing_key` shares that one decoder with jobs;
it is source policy, not a second signer or an additional authority store.
Endpoint input and authority-status validation also live in that module.
The reserved `__authority_validate` preserves the original check order,
including partition-before-generation for an exact registry entry, and reads
one consistent snapshot of the endpoint registry for its fallback scans.
The fresh-tree constructor supplies all initial fields in their original
evaluation order: generation 1, OneForOne restart with a 5/60000ms budget,
AtMostOnce delivery, no authority/snapshot/upgrade, and empty topology/state
stores.

Worker registration, failure/restart selection, restart-window counting and
mailbox-depth selection are Jet policy. Registration checks the top-level
handler function item and keeps only its name (no handler task runs
in-process), validates before entropy/allocation/publication, and supplies
every worker seed. Failure retains declaration-order group scope, strict
`at > saturating_sub(now, per_ms)` windows, reverse authority rollback,
escalation, durable-mailbox preservation and partition reassertion in the
original write order (`Services.rs:2917–3078`, restart helper `1983–2012`).
The restart helper replaces the worker's mailbox, so a restarted worker's new
channel is the one its registry entry publishes and endpoint sends reach. The
depth and count readers deliberately retain the weaker name/tree match after
endpoint validation; they do not substitute the stricter routing predicate.
Mailboxes move rather than fork their channel.

Snapshot/event-log selection, replay framing, topology/observation text, and
workflow history/outcome selection are Jet policies.
Group membership validation and chaos-failure ordering are Jet policies.
Group reparenting retains its original per-worker order, and chaos increments
its checked counter before selecting the first running worker.
Event append and snapshot commit validate their adapter, authority, payload,
and limits and frame UTF-8 byte lengths. Snapshot persistence and the
snapshot field assignment happen under the durable-store gate; event
persistence appends to the store before the local event record is appended.
No lock guard or second authority is exposed.
Directory resolution owns lookup, generation errors, expiration remapping,
and running/draining checks; its proof check calls the authority operation
without adding a registry or signing-key cache.
Tree sends validate routing and payloads, preserve the repeated authority
checks, remap mailbox-capacity failures to the worker-specific text, and
update accounting only after successful atomic authority sends.
Durable tree sends and receives own local deduplication, pending-message
admission, receipt marking, and rollback orchestration. A failed pending send
requeues the remaining authority suffix before restoring the pre-send mailbox.
Combined rollback failures preserve the original diagnostic and recovery text.
Generation rollout, start and stop are Jet policy; each registry change takes
the registry guard, the one linearization point for drain, partition, handoff
and endpoint send.
No service worker runs its handler in-process. `core.jobs.queue()` binds to
the package endpoint instead, whose authority `core.jobs` describes below.

| Signature | Result | Description |
|---|---|---|
| `tree(name: String) -> ServiceTree` | tree | Create a named service topology. |
| `tree.worker(name: String, handler, capacity: Int) -> ServiceEndpoint !ServiceError` | endpoint | Register a checked top-level handler with a bounded mailbox. |
| `tree.start()` / `tree.stop()` | — | Start or stop the service tree. |
| `tree.show() -> Dyn` | tree | Inspect topology and delivery state. |
| `tree.set_restart(strategy: ServiceRestart)` | — | Select one-for-one, all-for-one, or rest-for-one restart. |
| `runtime(store: String, retention: Duration) -> ServiceRuntime` | runtime | Describe runtime storage and retention. |
| `state_store(path: String) -[FS.Read]> ServiceStateStore !ServiceError` | store | Validate a durable state-store path and its lexical ancestors. |
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
implementation lives under [`Core/Archive/`](../../../Core/Archive/); it is not
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
| `zip_open(archive: [U8]) -> [U8] ArchiveError!` | reader | Validate and retain an immutable ZIP buffer; an empty buffer opens a writer holding the valid empty ZIP. |
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
`tests/run/CoreIO/ArchiveHostile.jet` exercises truncated, bad-checksum,
output-bomb, 4,097-entry, and traversal-name inputs for every codec.

### `core.archive.gzip` — bounded gzip files

`core.archive.gzip` handles whole-buffer gzip streams and text/file adapters.
It validates the gzip magic, size, CRC, and supported method before returning
bytes. See [`Core/Archive/Gzip.jet`](../../../Core/Archive/Gzip.jet).

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
[`Core/Archive/Zstd.jet`](../../../Core/Archive/Zstd.jet).

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
- Assignment that should not change → `name: value`
- Assignment that should change → `name := value`

`String.bytes()` and `String.from_bytes(...)` are the explicit text/byte
boundary. Use `Int.parse`/`Float.parse` for named parse errors, and
`core.text.parse.parse_int`/`parse_float` when an Option result is the desired
API.

## `core.jobs` — durable receiver lifecycle

`jobs.queue() -[DB, Env, FS, Time]> JobQueue ServiceError!` opens the
durable SQLite provider under the package authority. `JobQueue` has private state,
not an in-memory record collection passed between free functions. Queue
identity, capacity, retry budget, leases, and retention come from the
authority/provider policy.

| Receiver operation | Result | Contract |
| --- | --- | --- |
| `enqueue(job, payload, key:)` / `delay(job, payload, for:)` | `JobQueueReceipt !ServiceError` | Encode a checked job payload and atomically enqueue it; an optional key identifies duplicates. |
| `receipt(id)` / `inspect(limit, include_payload)` / `events(id)` | receipt / records / events | Observe signed receipts, bounded records, or ordered state events. |
| `claim(worker, limit)` / `heartbeat(claim)` | claims / receipt | Claim bounded due work and renew a checked lease. |
| `acknowledge(claim, result)` / `fail(claim, error)` | `JobQueueReceipt !ServiceError` | Settle a checked claim with a typed result or error. |
| `cancel(id, reason)` / `dead_letter(id, reason)` | `JobQueueReceipt !ServiceError` | Commit an explicit terminal state. |
| `recover_expired()` / `status()` / `pause()` / `resume()` / `wait(timeout)` / `prune()` | unit / status / count | Recover leases, inspect or control admission, wait, and prune by retention policy. |

Payloads and results carry `type_id`, encoded `bytes`, and explicit `publish`.
Claims carry their signed receipt, payload, worker, lease token, and lease
expiry. A status reports queued, running, retrying, completed, failed,
dead-lettered, and cancelled counts rather than exposing mutable queue storage.
All receiver operations retain typed `ServiceError` failures.

The source-owned lifecycle exposes the effects it actually consumes:

| Operation | Effects |
| --- | --- |
| `queue()` | `DB`, `Env`, `FS`, `Time` |
| `receipt` / `inspect` / `events` | `DB` |
| `claim` | `DB`, `Rand`, `Time` |
| `enqueue` / `delay` / `heartbeat` / `acknowledge` / `fail` / `cancel` / `dead_letter` / `recover_expired` / `status` / `pause` / `resume` / `prune` | `DB`, `Time` |
| `wait` | `DB`, `Time`, `Time.Wait` |

`JET_JOB_QUEUE_DB` is a startup-only provider-path exception: a nonempty UTF-8
startup value selects the database; otherwise `.jet/state/jobs.db` is used.
Logical environment overlays do not redirect this provider. Set an isolated
database path before launching examples or conformance programs.

The queue binds to the package endpoint, so durable jobs need no service setup
(D-DX-QUEUE1=A). Its authority is a private secret in `<database>.authority`.
The first run creates it with a no-replace publish of an exclusive 0600 file.
A concurrent first run that loses the race reads the winner's secret, so every
run of the package reaches the same rows and verifies the same receipts. An
existing secret that is not a regular file owned by the current user, or that
grants any group or other permission, is refused with `ServiceError.Policy`
and is never overwritten.

Queue status publication also retains `DB` alongside `Time`, matching the
provider calls reached while obtaining its wall-clock sample.

The state transitions, SQL, row projections, CBOR payloads, signed receipts,
retry policy, event fields, and commit-before-publication ordering are Jet
source in `Core/Jobs/Jobs.jet`. The private SQLite and Devtools leaves call
the existing host kernels; they do not implement a second queue or sink.
The compiler checks the declared `#Job` identity and its payload type before
normalizing the name and forwarding `enqueue`/`delay` to their checked source
targets; arbitrary function values or string-selected jobs are not admitted.

## `core` — built Core modules

The canonical module registry is [`Core.jet`](../../../Compiler/Prelude/Core.jet),
and the loader's exported list is the generated [`CoreCallRows.jet`](../../../Compiler/JetFoundation/Source/Registry/CoreCallRows.jet).
The list below is the source-facing module inventory. A module may be a
namespace-only provider boundary; its presence in the registry does not imply
that every provider operation is available on every target.
The built-module inventory is a source-facing registry, not a missing-domain ledger (`D-STDLIBLEDGER1`). Memory management is opt-in scoped automatic GC through `#Policy(gc)`; collector state remains compiler-private while ordinary code keeps bare owned values (`D-OPTGC1`).

- `app`
- `core`
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
- `core.db.sql`
- `core.encoding`
- `core.encoding.base32`
- `core.encoding.base64`
- `core.encoding.binary`
- `core.encoding.cbor`
- `core.encoding.csv`
- `core.encoding.eetf`
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
- `core.ui.xkb`
- `core.ui.font`
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

The sketch types are source-owned plain values: cloning one produces independent
storage, and mutating a sketch uses its `&self` receiver with an explicit `&` call.

### Writing Core in Jet

Jet source packages under [`Core/`](../../../Core/) are compiled by the Jet
compiler's normal frontend, like any other package.

`Core.jet` is the declaration registry, `Core/**/*.jet` is the source for the
Jet-owned package surface, and provider code supplies operations that require
an external runtime or operating-system capability. A change to an exported
signature must update the source module, the registry, the relevant conformance
corpus, and any examples that exercise the contract. The registry and loader
list are the executable authority; this reference explains the durable rules
and points to those files instead of duplicating their generated details.
The audited intrinsic/ABI kernel is the only provider seam; ordinary Core packages remain Jet source. Archive byte-format calls cross that seam only where the source implementation requires them.

The overloaded Unix connect and TLS client calls retain their checked public
argument forms before forwarding into their owning Jet entry points. HTTP
server serve/bind and static-file mounts likewise forward normalized options;
`HTTPMux` route and middleware registration lower to its Jet methods after the
existing escaping-callback admission. The original effects and resource
conventions remain in force.

### Examples in the repository

Executable examples are organized by capability. Useful entry points for this
part include:

- [`Examples/Text/TextCursor.jet`](../../../Examples/Text/TextCursor.jet)
  and [`binary-reader.jet`](../../../Examples/Data/Binary/BinaryReader.jet)
  for consuming scanners;
- [`Examples/Systems/Files/Files.jet`](../../../Examples/Systems/Files/Files.jet)
  and [`Examples/Systems/CLI/CLIArgs.jet`](../../../Examples/Systems/CLI/CLIArgs.jet)
  for capability-bound I/O;
- [`Examples/Data/JSON/JSONIntegerFidelity.jet`](../../../Examples/Data/JSON/JSONIntegerFidelity.jet)
  for typed encoding behavior;
- [`Examples/Math/Compute/ComputeTensor.jet`](../../../Examples/Math/Compute/ComputeTensor.jet)
  and [`app_live.jet`](../../../Examples/Tooling/DevLoop/AppLive.jet)
  for compute and web surfaces;
- [`tests/conformance/corpus/core/`](../../../tests/conformance/corpus/core/)
  for small, checked examples of the exported Core contracts.

Use the source module and its conformance corpus together when documenting an
API: the source establishes the export and error boundary, while the corpus
shows the syntax and boundary behavior that a caller can rely on.
