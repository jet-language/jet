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

Manual Core `Debug` implementations use the Prelude's consuming `debug(^self)`
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

Email address, attachment, envelope, and message construction validates in
`Core/Email/Email.jet`: control-character refusal precedes address parsing,
mailbox grammar is ASCII dot-atom or quoted local, MIME and physical-header
limits precede attachment size, and envelopes require 1–100 recipients.
The same source computes the native conservative wire bound and assembles MIME:
UTF-8 encoded words never split a scalar, transfer base64 folds at 76 bytes,
and native-order SHA-256 absorption determines mixed/alternative boundaries.
BCC participates in envelope and boundary generation, never visible headers.
Private whole-carrier views and builders only marshal native storage; refusal
errors retain the original Configuration operation strings.
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
`walk`, `walk_parallel`, and `walk_files` own traversal in Jet: depth starts at zero for root
entries, results sort by shown path, ancestor symlinks are refused, and each
directory inherits the existing ignore matcher. The private context preserves
native path identities (including non-Unicode names) until its scope guard
releases it. Failed directory batches are discarded while already queued work
continues; the earliest error is selected by native directory-path ordering.
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
Raylib's display/input/texture operations keep the existing registered host
bindings, with source-owned argument policy. Font shaping returns the existing
host result through a private leaf; the Jet entry owns the exact failure panic
and never selects an approximate fallback implicitly.

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
| `ABI`, `FFILanguage`, `Target`, `Track` | enum | Calling convention, foreign language, code-generation target, and tracking policy values |
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

The source declarations supply the public typed vocabulary; calls retain the
compiler-owned compile-time provider route even when that source package is loaded.

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
Declared Config files contribute package facts and build identity, not ordinary
source declarations; a Config binding cannot shadow a package function.

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
`sort`, `sort_desc`, `min` and `max` on named tuples compare members in label
order, not written order (S73, D-TUPLE-ORDER1=A). So `(key, index)` sorts by
`index`, then `key`; use `sort_by` to put another member first.
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

| Receiver | Operation | Receiver ownership | Law | Empty result | Status |
| --- | --- | --- | --- | --- | --- |

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
| `open(path, links:)` / `create(path, if_exists:, mode:, links:)` / `append(path, mode:, links:)` | `FileReader` / `FileWriter !IOError` | Open a close-on-exec stream handle. `if_exists: .Refuse` creates exclusively, `mode` sets a new file's bits before the umask, and `links: .Refuse` refuses a final symbolic link. |
| `open_directory(path)` | `Directory !IOError` | Hold a directory whose last part is a real directory, not a link. |
| `samefile(a, b)` | `Bool !IOError` | Compare the two entries' `FileID`s. |
| `extended_attribute_names(path)` / `extended_attribute(path, name)` / `set_extended_attribute(path, name, value)` | `[String]` / `[U8]?` / `!IOError` | Act on the path itself, never a final link's target. Names are sorted. |
| `exists(path)` / `is_dir(path)` / `is_file(path)` | `Bool` | Return false when the path cannot be observed; these helpers do not raise a missing-path error. |
| `stat(path)` / `lstat(path)` | `Stat !IOError` | Describe the entry itself, never a final link's target. |
| `set_mode(path, mode)` | `!IOError` | Set Unix mode bits; non-Unix providers use their documented readonly mapping. |
| `list_dir(path)` | `[DirEntry] !IOError` | List entries in name order. |
| `create_dir(path)` / `create_dir_all(path)` | `!IOError` | Both preserve the existing recursive directory-creation behavior, including missing parents. |
| `remove(path)` / `remove_dir(path)` / `remove_all(path)` | `!IOError` | Remove a file, empty directory, or tree. |
| `copy(from, to)` / `copy_dir(from, to)` | `!IOError` | Copy one regular file or a directory tree. Directory copy does not dereference symlinks or special entries. |
| `rename(from, to, if_exists:)` | `!IOError` | Rename or move a path; `.Replace` (default), `.Refuse`, or `.Swap`. |
| `symlink(original, link)` / `hard_link(original, link)` | `!IOError` | Create symbolic or hard links. |
| `read_link(path)` | `String !IOError` | Read a symbolic-link target. |
| `canonicalize(path)` / `absolute(path)` | `String !IOError` | Resolve an existing path with symlinks, or make an absolute lexical path without requiring existence. |
| `walk(root, ignore)` / `walk_parallel(root, ignore)` | `[WalkEntry] !IOError` | Traverse recursively with an optional ignore-file name. The parallel spelling keeps the same ordering and no-follow policy. |
| `walk_files(root, ignore)` | `[WalkEntry] !IOError` | Traverse regular files only. |
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

`walk`, `walk_parallel`, and `walk_files` accept the optional `ignore` value;
`fs.walk(root)` is the convenience form. `DirEntry` has `name`, `path`, and
`is_dir`. `Stat` has `size`, `kind`, `mode`, `readonly`, `is_file`, `is_dir`,
`is_symlink`, `id: FileID{device, inode}`, `links`, `owner: Int?` (None on
Windows), `size_on_disk`, `modified` and `changed` (`DateTime`, nanosecond
precision), and `created: DateTime?` (None where the file system keeps no
birth time). `WalkEntry` exposes `path`,
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
  read-only. Consuming `close(^lock)`, scope exit, or process exit releases
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
  `close(^lock)` or scope exit releases it, without deleting a name. Systems
  without directory flock report `Unsupported`. This differs from locking
  the regular file named by `files.lock(path)`.
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
| `reader.read_line()` | `?String !IOError` | Read from the shared buffered position, remove all trailing CR/LF bytes, and return `None` only at EOF. A final unterminated line is returned; invalid UTF-8 is `IOError.InvalidInput`. |
| `reader.read(max)` | `[U8] !IOError` | Read up to `max` bytes from the shared position; an empty list is EOF. |
| `reader.stat()` / `writer.stat()` | `Stat !IOError` | Describe the open file itself (fstat); a writer's buffered bytes count only after a flush. |
| `reader.lines()` in a loop source | stream of `String` | Pull lines without loading the complete file. |
| `writer.write_line(text)` / `writer.write_all(bytes)` | `!IOError` | Write text and a newline, or every byte of `bytes` (buffered). |
| `writer.flush()` / `writer.fsync()` | `!IOError` | Flush pending writer bytes; `fsync` also makes the file's data durable. |
| `writer.set_mode(mode)` | `!IOError` | Set the open file's permission bits; Windows keeps only read-only. |

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
codec behavior; the parser follows RFC 3986-style rules. Non-ASCII host labels
are lowercased byte-wise and Punycode-encoded (`xn--`, RFC 3492); no WHATWG
IDNA mapping is applied.

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
| `dkim(domain, selector, key, signed_headers)` | `DKIMConfig` | Configure one signing identity and its signed-header list. |
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

The safe limits, environment configuration, and private configuration predicates
live in `Core/Email`. Before creating a mailer, `smtp_from_env` checks limits,
host, port/security pairing, authentication name, CA PEM, and DKIM header policy,
in that order. The first invalid limit is reported in field declaration order.
`Limits.safe()` supplies the same six bounds used by `smtp_from_env`; DKIM
environment keys require canonical standard base64 independently of edition.
The decoder borrows the encoded `Secret`, uses whole four-byte groups, and
keeps partial decoded output in a `Secret` so malformed input follows the same
zeroizing ownership path as successful decoding.

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
| `request.body()` / `response.body()` | lazy `Body` | Keep the transport body lazy; `body.text(limit)` and `body.bytes(limit)` are fallible, bounded consumption operations, not construction of the owned source variants. |
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

`core.http.server` builds a typed multiplexer and serves HTTP/1.1. `bind` and
`serve` accept optional `HTTPServerTLS` and optional deadlines; `serve_once`
and `serve_once_listener` provide testable one-request entry points.
The Jet `tls(cert, key)` constructor preserves both PEM strings verbatim,
including empty or malformed material. Loading and validating those credentials
belongs to the listener's TLS setup, not to construction.


`core.http.serve(mux, deadline:)` is the loopback lifecycle entry point: it
returns an `HTTPServer` handle whose `shutdown()` drains pending requests and
returns an `HTTPShutdownReport`. It is distinct from the blocking
`core.http.server.serve(addr, mux, ...)` operation.

`core.http.serve(mux, deadline:)` is the loopback lifecycle entry point: it
returns an `HTTPServer` handle whose `shutdown()` drains pending requests and
returns an `HTTPShutdownReport`. It is distinct from the blocking
`core.http.server.serve(addr, mux, ...)` operation.

`core.http.serve(mux, deadline:)` is the loopback lifecycle entry point: it
returns an `HTTPServer` handle whose `shutdown()` drains pending requests and
returns an `HTTPShutdownReport`. It is distinct from the blocking
`core.http.server.serve(addr, mux, ...)` operation.

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
| `tls(cert, key)` | `HTTPServerTLS` | Build explicit server TLS material. |
| `sse(body)` | `HTTPResponse` | Build a server-sent-events response. |
| `cors_policy(origins)` / `cors(mux, policy)` | policy/unit | Validate a CORS origin policy and install it. |
| `access_log(request, status)` | `String` | Render a stable access-log line. |
| `request_id(mux)` | unit | Install request-id middleware. |

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
| `connect(url)` | `WsConn !WsError` | Validate and dial a cleartext WebSocket URL. |
| `upgrade(request)` | `WsConn !WsError` | Validate GET, Upgrade, Connection, version 13, and key headers before upgrading. |
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
The session-started carrier fact is recorded before validating the session ID
and capabilities. CDP requires the first `goog:cdp` field to be exactly true.
Socket/command operations and the three native carrier tails remain registered
raw leaves; there is no whole-handshake adapter.

| API | Result | Description |
| --- | --- | --- |
| `config()` / `config_from_env()` | `BrowserTestConfig !BrowserError` | Load checked browser-test configuration. |
| `make_profile(name)` / `profile(name)` / `timeout(ms)` | `BrowserProfile` / `BrowserProfile !BrowserError` / `BrowserTimeout !BrowserError` | Construct an isolated profile, check a pinned BiDi profile (`bidi-2024.11` or `bidi-2025.5`), or check a timeout in `1..=600000` milliseconds. |
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
[`crypto_suite.jet`](../../../Examples/features/crypto/crypto_suite.jet) and
[`crypto_envelope.jet`](../../../Examples/features/crypto/crypto_envelope.jet).

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
carries the `Secret` effect. A `KeyRef<T>` contains name/version identity, not
key bytes; the provider owns persistence and key-material lifetimes.
Its `Display` is `repo:{name}@v{version}` for every key type `T`; rendering never
reads key material or private provider metadata and imposes no `Display` bound
on `T`.

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
| `load<T>(&key)` / `status<T>(&key)` | `T !VaultError` / `KeyStatus !VaultError` | Load an exact Active or Retired key, or inspect `Active`, `Retired`, or `Revoked`. |
| `prepare_generate<T>(name)` / `prepare_store<T>(name, ^key)` | `MutationPlan<T> !VaultError` | Prepare a typed mutation without publishing it. |
| `prepare_import_signing` / `prepare_import_x25519` | typed plan `!VaultError` | Prepare a checked 32-byte signing or X25519 secret import. |
| `prepare_rotate<T>(name)` | `MutationPlan<T> !VaultError` | Prepare a rotation for a named generation. |
| `prepare_retire<T>(&key, reason)` / `prepare_revoke<T>(&key, reason)` | `MutationPlan<T> !VaultError` | Bind a retirement or revocation to an exact key and reason. |
| `authorize_write<T>(&plan, reason)` | `VaultWrite<T> !VaultError` | Request one provider-authorized write for the exact plan. |
| `commit_generate/store/rotate/retire/revoke<T>(^write, ^plan)` | typed result `!VaultError` | Consume the write and plan in an atomic mutation. |
| `export_to_recipients<T>(&key, recipients)` | `WrappedVaultKey !KeyWrapError` | Export a typed key for one to sixteen distinct X25519 recipients. |
| `export_to_passphrase<T>(&key, &passphrase)` | `WrappedVaultKey !KeyWrapError` | Export a typed key under a bounded nominal Secret. |
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
[`vault_keys.jet`](../../../Examples/features/crypto/vault_keys.jet) and
[`expiring_secret.jet`](../../../Examples/features/memory/expiring_secret.jet).

The wrapper stores its optional credential and original clock observer in Jet.
It samples that observer under the storage guard, and expires only when
`now > deadline`; at equality the credential remains available. Expiry takes
the payload out of its slot and ends the owned payload's scope, so its
`Close` follows the same zeroization path as dropping still-live storage.
The sole wipe host operation is the existing plain-byte `__zeroize` leaf.
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
[`auth_tokens.jet`](../../../Examples/features/crypto/auth_tokens.jet) and
[`auth_sessions.jet`](../../../Examples/features/crypto/auth_sessions.jet).

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
a shown document over the live-query channel. This module is not an
authenticated remote transport or
a general thread-lock API; use the typed transport and task modules for those
contracts. The thread-lock ledger names (`broadcast`, `clear`, `lock`, `rlock`,
`signal`, `trylock`, `unlock`, `wait`, `thread`, `timer`, and `locked`) remain
declined under D-CORESURF-SMALL1.

### `core.watcher` — file, process, and port watches

`core.watcher` creates explicit watch handles for files, process IDs, and TCP
ports (D-WATCH-SCOPE1). Polling is explicit: a handle or set does not own a
background thread or shell process, and the public module has no callback
registration method.

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

`WatchDomain` is `File`, `Process`, or `Port`. `WatchEventKind` is `Created`,
`Modified`, `Removed`, `Error`, `Exited`, or `Ready`. `WatchEvent` carries
`domain`, `kind`, `path`, `detail`, `pid`, and `port`. Match all variants when
handling a closed event enum. See
[`watcher.jet`](../../../Examples/features/io/watcher.jet).

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
use core.term as term

fn run() {
    name :: term.input("your name? ") ?? return
    print("hi, {name}")
    term.eprint("(log) done")
    out := term.stdout()
    &out.write("done") ?? return
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

`style` is a no-op when output is not a TTY unless `style_force` is used.
`Key` carries `Char(Char)`, `F(Int)`, and `Ctrl(Char)` payloads. Its unit variants
are `Enter`, `Escape`, `Backspace`, `Tab`, `Delete`, the four arrow keys, and
`Unknown`. `confirm` treats a bare Enter as no; `choose` returns an input error
after its finite retry budget or when stdin closes.

Read a buffered handle with `&reader.read_line()`: each call mutably lends
the reader and returns `String? !IOError`. Ordinary failure propagation
preserves the optional line, so EOF remains `None` rather than an I/O error.

The executable terminal-source examples cover binary-file composition, UTF-8
stderr, CRLF line reads and EOF, plus confirmation with a closed-input default:
[`terminal_source.jet`](../../../Examples/features/io/terminal_source.jet) and
[`terminal_confirm.jet`](../../../Examples/features/io/terminal_confirm.jet).
Their programs and transcripts are shared by the golden and native harnesses;
they do not require a bespoke Rust test or direct `rustc` invocation.

### `core.args` — declarative command-line parsing (D-ARGS1)

`core.args` provides an `ArgsSpec` builder, explicit `decode_argv` tree parsing,
and typed `decode<T>` / `merge<T>` using the target's checked `#CLI` shape.
`ArgsSpec` is the checked builder handle, not a record containing only flag
names. The builder consumes a spec and returns a new value at each method call;
parsing is separate from process execution so tests can pass an explicit
argument vector.

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
| `decode()` | `Dyn` | Decode the current process arguments, including `help` and `h` flags. |
| `decode_argv(argv)` | `Dyn` | Decode an explicit argument vector. |
| `merge(base, overlay)` | `Dyn` | Recursively merge objects; overlay arrays and scalars replace. |
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
| `executable()` / `username()` | `String -[Env]>` | Return process facts. `executable()` is the running program's path: the built binary, or the script's absolute path under `jet run`; launching it runs the program again on either tier. |
| `hostname()` | `String -[Env, FS]>` | Return the host name. |
| `release()` / `version()` | `String -[Env, FS, Exec, Time.Wait]>` | Read operating-system facts; macOS queries run `uname` or `sw_vers`. |
| `pid()` / `getpid()` / `getppid()` / `cpu_count()` | `Int -[Env]>` | Return process IDs or a logical CPU count of at least one. |
| `getuid()` / `geteuid()` / `getgid()` / `getegid()` / `getpgrp()` | `Int -[Env]>` | Return POSIX identity and process-group values where supported. |
| `getgroups()` | `[Int] -[Env]>` | Return supplementary group IDs. |
| `expand(value)` | `String -[Env]>` | Expand `$VAR`, `${VAR}`, and the supported percent form from the environment. |
| `uptime()` | `Float -[Time, Env, FS]>` | Read system uptime from `/proc/uptime` on Linux. |
| `loadavg()` / `times()` | `[Float] -[Env]>` | Return load averages or process CPU times. |
| `exitcode(status)` / `success(status)` | `Int` / `Bool` | Interpret a wait status without an effect. |
| `sync()` | `Unit -[FS, Env]>` | Flush filesystem buffers. |
| `getpgid(pid)` / `getsid(pid)` | `Int !IOError -[Env]>` | Inspect POSIX process-group or session state. |

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
| `fork()` / `setsid()` | `Int !IOError -[Env]>` | Fork or create a process session. |
| `wait()` / `waitpid(pid, options)` | `Int !IOError -[Env, Time.Wait]>` | Reap a child or selected child. |
| `kill(pid, signal)` | `!IOError -[Env]>` | Send a signal. |
| `setuid` / `setgid` / `setpgid` / `setpgrp` / `initgroups` | `!IOError -[Env]>` | Change credentials, groups, or process-group membership. |
| `pipe()` / `close_fd(fd)` | `[Int] !IOError -[FS, Env]>` / `Unit -[FS, Env]>` | Create or close raw process descriptors. |
| `mkfifo(path, mode)` | `!IOError -[FS, Env]>` | Create a named pipe. |
| `atexit(handler)` | `Unit` | Register a `fn()` handler that runs at the explicit process boundary after scope cleanup (D-FAIL-EXIT1). |

The `core.sys` module does not export `on_interrupt`; use the process signal
and task APIs for interrupt handling.

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
[`data_analysis.jet`](../../../Examples/features/tooling/data_analysis.jet).

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
| `process.which(name)` | `?String -[FS, Env]>` | Find an executable using filesystem checks and the host search path. A candidate that is a link counts when its target is a file; `files.which` and `Path.which` search the same way. |
| `process.exit(code)` | `Never -[Exec]>` | Terminate the current process at the explicit process boundary. |
| `process.on_signal(signal)` | `Unit -[Exec]>` | Register the process signal policy for one `ProcessSignal` value. |
| `process.signal_number(signal)` | `Int` | Convert a `ProcessSignal` value to its platform signal number where one exists. |
| `process.list2cmdline(argv)` | `String` | Format an argument vector for display using Windows command-line quoting rules. |
| `ProcessSignal` | type | Closed variants: `Term`, `Hup`, and `Int`, matching the shared signal dispatcher. |
| `ProcessReceipt` | type | The immutable result of a run, including status, output, policy digests, backend, authority, descendants, limits, and redaction facts. |
| `ProcessSpec` | type | The executable, arguments, stream policy, authority, terminal policy, limits, and detachment policy for a child. |

A child that cannot start is an `IOError`, never a receipt: its kind
follows the launch's OS error the way a file operation's does (`NotFound`
for a missing program, `PermissionDenied` for one that may not be run,
`Other` for the rest), with operation `.Resolve`, the program as its
resource and the OS error code. An empty command (`process.cmd([])`) or an
empty `process.pipeline([])` is `InvalidInput` naming that empty call. A
program that starts and then exits nonzero, 127 included, is a receipt.

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
| `child.stdout.lines()` / `child.stderr.lines()` | line stream | Read from the shared buffered pipe position, removing one LF and its preceding CR only. A final bare CR remains; invalid UTF-8 is `IOError.Other` on `process output`. The parent remains responsible for draining it. |

A terminal session is an opt-in process mode. It supplies terminal-sized input
and output through the terminal policy and fails with a typed error when the
selected backend cannot provide one; it is not an implicit replacement for
captured or inherited streams (D-PROCESS-SESSION1=A, D-PROCESS-SESSION2=D).

### `core.math` — numeric functions

`core.math` supplies integer operations, IEEE floating-point wrappers, numeric
predicates, checked and saturating arithmetic, and elementary functions. The
floating-point constant-like values are functions, so use `math.pi()` and
`math.e()` rather than treating them as fields.
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
use core.math as math

fn run() {
    angle := math.pi() / 4.0
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
| `value.pow(exponent)` | Int and Float | Numeric power with the existing exactness and negative-exponent rules. A written negative Int exponent produces Fraction; a dynamic negative Int exponent stops with the existing arithmetic fault. |

These intrinsic methods lower to the ordinary typed numeric operations, not
user function calls, and call the same numeric runtime on every tier.
An update writes `value = value.bit_xor(other)`. `!value` is Boolean
negation only. Shifts retain `<<` and `>>`.

Floating classifiers such as `math.is_nan` require Float or F32, not Int.
Multi-operand floating calls such as `math.hypot`, and floating `pow`
operands, must use matching Float/F32 widths; convert explicitly before
calling them. Source-owned Core wrappers preserve these same constraints.

| Function | Returns | Description |
| --- | --- | --- |
| `math.acos(x)`, `acosh(x)`, `asin(x)`, `asinh(x)`, `atan(x)`, `atan2(y, x)`, `atanh(x)` | `Float` | Inverse trigonometric and hyperbolic functions. The extended float family is part of the shared math surface (D-CORESURFACE1). |
| `math.abs(value: Int)` / `math.abs_float(value: Float)` | `Int` / `Float` | Absolute value for integer or floating-point input. |
| `math.copy(value: Float)` | `Float` | Return the floating-point value unchanged, including its IEEE representation. |
| `math.min(a: Int, b: Int)` / `math.max(a: Int, b: Int)` | `Int` | Integer extrema. |
| `math.clamp(value, lo, hi)` | `Int` | Clamp an integer; reversed bounds are accepted by swapping the bounds. |
| `math.is_even(value)` / `math.is_odd(value)` | `Bool` | Test integer parity. |
| `math.sign(value: Int)` | `Int` | Return `-1`, `0`, or `1` for a negative, zero, or positive integer. |
| `math.isqrt(value)` | `?Int` | Return the integer square root when the input is nonnegative. |
| `math.gcd(a, b)` / `math.lcm(a, b)` | `Int` | Greatest common divisor or least common multiple. |
| `math.gcd_many(values)` / `math.lcm_many(values)` | `Int` | Fold integer gcd or lcm over a list. |
| `math.cos(x)`, `cosh(x)`, `sin(x)`, `sinh(x)`, `tan(x)`, `tanh(x)`, `math.cot(x)` | `Float` | Trigonometric and hyperbolic functions. |
| `math.exp(x)`, `exp2(x)`, `exp_m1(x)`, `expm1(x)`, `math.erf(x)`, `math.erfc(x)` | `Float` | Exponential and error functions. |
| `math.ln(x)`, `ln_1p(x)`, `log1p(x)`, `log10(x)`, `log2(x)`, `logb(x)`, `log(x, base)` | `Float` | Natural, base-specific, and compensated logarithms. |
| `math.sqrt(x)`, `cbrt(x)`, `hypot(x, y)` | `Float` | Roots and hypotenuse. |
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
| `math.is_nan(x)`, `is_inf(x)`, `is_finite(x)` | `Bool` | Test NaN, infinity, or finite status. `is_finite` accepts both `Float` and `F32`. `isnan`, `isinf`, and `isfinite` are aliases. |
| `math.cmp(a, b)` | `Int` | Accept matching `Float` or `F32` operands and return `-1`, `0`, or `1`; NaN sorts after non-NaN values, two NaNs compare equal, and signed zeros compare equal. |
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
Their `Display` implementations use the canonical `fn display(self) -> String`
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
indexing, vector addition and subtraction, and element-wise multiplication.
`Vec3` also supports scalar multiplication and division, and the `dot`,
`cross`, `length`, and `normalize` operations. `Mat3` and `Mat4` constructors
use column-major storage; they provide `from_array`, `to_array`, addition,
subtraction, matrix multiplication, `matmul`, `transform`, and `transpose`.
Reductions use a scalar left-to-right fold, so the reduction order is part of
the reproducible fixed-array contract (D-LINALG1; `to_array` is the fixed-array bridge, D-FIXARR1).
Arithmetic results are ordinary math values: lane reads, swizzles, and methods
operate directly on them, including temporary receivers such as `(a + a).dot(a)`.
See [`vector_result_values.jet`](../../../Examples/features/lowlevel/vector_result_values.jet).

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
use core.math.random as random

fn roll_at(rng: &RNG) -> String {
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
`Solver.new(seed)` starts with zero checks and failures, `&solver.require(ok)`
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

The checksum-verifying `Tools/agent/gen-unicode-tables.mjs` owns the pinned
mapping and property assets, including the native runtime's fixed-word
lookup functions. Regenerate them from `tests/data/unicode/ucd`; never copy
host Unicode tables or hand-maintain a native whitespace/casing list.

The generator preserves each seed/reference boundary: Foundation's split/cut
helpers return Rust `Option`, while the AOT prelude borrows `str` inputs and
returns Jet's `Outcome`. Run `--check` to check all generated assets for exact
byte identity; regenerating Unicode tables must not migrate either ABI.

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
| `text.nfc(value)` / `nfd(value)` / `nfkc(value)` / `nfkd(value)` | `String` | Apply the named normalization form. |
| `text.graphemes(value)` / `words(value)` / `sentences(value)` | `[String]` | Return grapheme segments or Unicode 17.0.0 UAX #29 words/sentences. Words keep only segments containing an Alphabetic or Numeric scalar; sentences trim Unicode White_Space and drop empty segments. |
| `text.char_indices(value)` | `[String]` | Return scalar strings with their UTF-8 byte indexes. |
| `text.display_width(value)` | `Int` | Compute terminal display width under the portable text-width policy (D-TEXTWIDTH1=B); ambiguous scalars are narrow. |
| `text.grapheme_views(value)` / `word_views(value)` / `line_views(value)` / `byte_views(value)` | `[String]` | Produce views or segments for the requested boundaries. |
| `text.is_alphabetic(value)` / `is_numeric(value)` / `is_whitespace(value)` | `Bool` | Test the corresponding scalar property. |
| `text.has_control(value)` | `Bool Never!` | True iff any scalar is Unicode 17 Cc (`U+0000..U+001F` or `U+007F..U+009F`); empty text is false. One UTF-8 byte scan stops at the first match without creating per-character strings or a decoded-scalar list. |
| `text.trim(value)` / `trim_start(value)` / `trim_end(value)` | `String` | Remove supported whitespace at both or one end. |
| `text.pad_start(value, width, fill)` / `pad_end(value, width, fill)` | `String` | Pad to a requested display width. |
| `text.center(value, width, fill)` | `String` | Center a value in a requested width. |
| `text.starts_any(value, prefixes)` / `ends_any(value, suffixes)` | `Bool` | Test any of several prefixes or suffixes. |
| `text.splitn(value, separator, limit)` / `rsplitn(value, separator, limit)` | `[String]` | Split from the left or right with a maximum part count. |
| `text.inspect(value)` | `[String]` | Return each scalar as `U+XXXX NAME`, using pinned Unicode names (Unicode 1.0 names for controls, `UNASSIGNED` where no name is recorded). |
| `text.Cursor` / `text.cursor(value)` / `text.cursor_advance(text, cursor)` | type / cursor | Construct and advance a text cursor. |

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
tiers. `Date` aliases `LocalDate`; immutable zone tables are plain values,
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
are `MONDAY :: 0` through `SUNDAY :: 6`.

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
`#Rename`, `#Skip`, `#Flatten`, `#RenameAll`, `#DenyUnknownFields`,
`#Discriminant`, and `#Untagged` describe the wire shape. Unknown fields are
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
path and position; the `_allow_duplicates` siblings are the explicit opt-in.
The reader enforces depth `256` and a maximum item size of `16,777,216` bytes.

| Function or type | Returns | Description |
| --- | --- | --- |
| `json.parse(text)` / `decode(text)` | `Dyn !EncodingError` | Parse one strict JSON value. |
| `json.loads(text)` / `load(text)` | `Dyn !EncodingError` | Compatibility aliases for parsing text. |
| `json.dumps(value)` / `dump(value)` / `to_string(value)` | `String` | Emit compact JSON. |
| `json.to_string_pretty(value)` | `String` | Emit two-space pretty JSON. |
| `json.canonical(value)` | `String !EncodingError` | Emit RFC 8785 JCS bytes: UTF-16 key order, ECMAScript numbers; reject NaN, infinities, and an `Int` that binary64 cannot hold exactly. |
| `json.parse_allow_duplicates(text)` | `Dyn !EncodingError` | Parse like `parse`, but keep the later value of a repeated object name. |
| `json.pointer(tree, path)` | `Dyn !EncodingError` | Select by RFC 6901 JSON Pointer; a missing member or bad step is a located error, JSON `null` is a value. |
| `json.patch(doc, ops)` / `patch_with_limits(doc, ops, limits)` | `Dyn !EncodingError` | Apply an RFC 6902 operation array atomically; `doc` is unchanged on failure. |
| `json.events(value)` | `[DataEvent]` | Produce the canonical event sequence for a tree. |
| `json.reader(text)` / `reader_allow_duplicates(text)` | `JSONReader` | Pull events over text already held in memory; the duplicate-permitting reader reports each key occurrence. Its `next()` returns `DataEvent !EncodingError`, including an error after exhaustion. |
| `json.reader(^input, limits)` / `json.writer(^output, limits, canonical)` | `JSONReader !EncodingError` / `JSONWriter !EncodingError` | Construct bounded file-backed event streams. |
| `JSONReader.next()` | `?DataEvent !EncodingError` | Return the next event or stable end-of-input. |
| `JSONWriter.write(event)` / `flush()` / `finish()` | `Unit !EncodingError` | Write events, flush output, or validate and finish the stream. |

The text and file overloads retain separate checked layouts. A consumed
`^FileReader` selects the bounded file contract; a `String` selects the source
text parser. Neither overload constructs the other overload's handle.

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
| `jsonl.reader(text)` | `JSONLReader` | Pull one nonblank record at a time from text already in memory; parse no record beyond the last requested one. |
| `jsonl.reader(^input, limits)` / `jsonl.writer(^output, limits)` | `JSONLReader !EncodingError` / `JSONLWriter !EncodingError` | Construct bounded file-backed line streams. |
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
stream retains its separate native callback carrier.

| Function or type | Returns | Description |
| --- | --- | --- |
| `csv.rows(text, delimiter, header, skip_blank)` | `[CSVRow] !EncodingError` | Parse physical rows with explicit delimiter, header and blank-line policy. |
| `csv.parse(text)` / `decode(text)` | `Dyn !EncodingError` | Parse rows as an array of arrays. |
| `csv.to_string<T: Encode>(rows)` | `String` | Encode explicit row arrays or typed records; records use the first record's field order as the header. Rows are separated by LF, with no trailing LF. |
| `csv.reader(^source, limits, delimiter, header, skip_blank)` / `csv.writer(^target, limits)` | `CSVReader !EncodingError -[FS]>` / `CSVWriter !EncodingError -[FS]>` | Construct incremental file-backed CSV objects. Reader defaults retain the header and blank records. |
| `CSVReader.next()` | `?CSVRow !EncodingError -[IO]>` | Return the next physical row or stable end-of-input; errors retain their byte offset, physical location, and record/field path. |
| `CSVWriter.write(^fields)` / `flush()` / `finish()` | `Unit !EncodingError -[IO]>` | Write comma-delimited string fields; CRLF is deferred until the next record or finish. Flush does not finish a record. |
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
| `xml.reader(^input, limits, options)` | `XMLReader !EncodingError` | Construct a bounded file-backed XML event reader. |
| `xml.attribute(element, name)` / `content(element)` / `expanded_name(node)` / `root(document)` | varies `!XMLError` | Inspect attributes, content, expanded names, and the document root. |
| `XMLWriter.write(event)` / `flush()` / `finish()` | `Unit !EncodingError` | Write XML events, flush output, or validate and finish the stream. |

XML rendering is fallible at both runtime and compile time. Invalid document
trees return the located `XMLError`; callers must handle it explicitly rather
than relying on an empty-string fallback. Compile-time code that stops on the
error is reported by the ordinary compile-time-stop diagnostic.

### `core.encoding.cbor` — bounded CBOR

`core.encoding.cbor` handles RFC 8949 integer, text, array, map, simple,
and float16/32/64 values, including indefinite strings and containers.
Dynamic `parse` rejects byte strings; typed `decode<[U8]>` preserves them.
Tags and duplicate text map keys are unsupported. Canonical mode requires
shortest arguments and float widths, a canonical NaN, definite containers,
and bytewise map-key order. Encoding uses the shortest exact float width.
Whole-value parsing and preferred float-width checks share a sealed source
part; its helper functions are not exports of the public CBOR module.

| Function or type | Returns | Description |
| --- | --- | --- |
| `cbor.parse(bytes, options)` | `Dyn !CBORError` | Parse a whole value under depth, item, allocation, and canonical limits. |
| `cbor.to_bytes<T: Encode>(value)` / `to_bytes_canonical(value)` | `[U8] !CBORError` | Encode ordinary or canonical CBOR. |
| `cbor.reader(^input, limits)` / `writer(^output, limits)` | `CBORReader !EncodingError` / `CBORWriter !EncodingError` | Construct bounded file-backed event streams. |
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
| `status()` | `[DataStatus] -[FS, Env]` | Report native and bridge path, ownership, trust, fallback, and replacement facts. Live R availability reads `PATH` and `JET_DATA_R_BRIDGE`, then checks files. |
| `require_bridge(provider)` | `Unit !DataError -[FS, Env]` | Fail closed when a requested `py`, `r`, or `gpu` bridge is unavailable; use the same live availability facts as `status`. |

Query methods filter, sort, map, extrema, joins, grouping, collection, and
watching while retaining typed positions for errors. `Query<T>.plan()` returns
deterministic operation names; `collect()` materializes `[T] !DataError`.
The source `query(rows)` facade constructs the shared deferred carrier through
the data provider, rather than rebuilding its private storage as a source
record. Filter, sort, and grouping selectors receive an owned row value at
the provider callback boundary.
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

### `core.data.plot` — deterministic data plots

`core.data.plot` turns aligned labels and numeric values into a deterministic
plan and renders that plan as text or SVG. The source implementation is a
portable inspection renderer: it does not require a GPU, and it makes missing
values explicit as zero. The module contract is defined in
[`Core/Data/Plot.jet`](../../../Core/Data/Plot.jet) and exercised by the plot
corpus under [`tests/conformance/corpus/core/data/plot/`](../../../tests/conformance/corpus/core/data/plot/).

The aggregate selector retains the native absence value, distinct from
`Count`, `Sum`, `Mean`, `Min`, and `Max`. Core internals obtain that value
through the registered `__plot_aggregate_none()` raw enum constructor;
there is no renamed source variant or alternate aggregate representation.


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
steps, so every tier prints the same text. Fixed-precision rounding scales by
`10^frac` and rounds half away from zero; non-finite values print as ordinary
`Float` text. `duration` does not emit unbounded trailing zero units.
The library-call boundary is deliberate: ordinary human formatting remains explicit (`D-HUMANFMT1`), and `pretty` follows the expanded deterministic Debug shape (`D-FMT-PRETTY1`).

### `core.log` — structured logging

`core.log` writes structured records through a configured sink. Levels are
`debug`, `info`, `warn`, `error`, `critical`, and `fatal`; `warning` is an
alias for `warn`. A record can carry typed fields and a redaction marker. The
package source is [`Core/Log/Log.jet`](../../../Core/Log/Log.jet).

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
| `setup(spec: String) -[Log]>` | — | Apply comma-separated `key=value` settings; a bare `auto`, `json` or `text` selects the format. |
| `enabled(level: String) -[Log]> Bool` | Boolean | Test whether a level is enabled. |
| `flush() -[Log]>` / `disable() -[Log]>` | — | Flush records or disable logging. |

Sink names are normalized to `stderr`, `json`, `jsonl`, or `text`; there is
no `stdout` sink. `jsonl` and `text` sinks require a path. An unsupported sink
kind, a file sink without a path, or an unknown `setup` setting leaves the
configuration unchanged and logs an `error` record naming the rejected
setting. Keep secrets in `redact` fields so
the sink, rather than each caller, owns the redaction decision.
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
| `after(delay: Duration, value: Int{0}) -[Time]> Receiver<Int>` | receiver | Deliver one value after a duration. |
| `interval(period: Duration) -[Time]> Receiver<Int>` | receiver | Deliver successive tick counts starting at one. |
| `yield_now() -[Time]>` | — | Yield to another runnable task. |
| `current_task() -> String` | text | Identify the running task. |
| `timeout(delay: Duration) -[Time]>` | — | Wait for a duration before returning. |

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

The source scheduler-channel constructors preserve that pair rather than
returning `TaskState`. Bounded scheduler channels clamp capacities below one
to one. Interval periods truncate to whole milliseconds and clamp to at least
one millisecond; their first delivered tick is one.

The source `Condition` extension delegates notification to the existing shared
condition protocol, preserving parked waiters rather than fabricating state.
The realtime stream extension likewise observes and cancels the same provider
worker; its source callback constructor rejects nonpositive rates before
crossing the provider boundary.

The source scheduler-channel constructors preserve that pair rather than
returning `TaskState`. Bounded scheduler channels clamp capacities below one
to one. Interval periods truncate to whole milliseconds and clamp to at least
one millisecond; their first delivered tick is one.

### `core.prelude` — small compiler-owned helpers

`core.prelude` is available without a separate package import. Its functions
are deliberately small barriers and predicates used by generic code and
constant evaluation; they are not alternate syntax for bindings. See
[`Core/Prelude/Prelude.jet`](../../../Core/Prelude/Prelude.jet).

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
[`Core/Testing/Testing.jet`](../../../Core/Testing/Testing.jet).

| Signature | Result | Description |
|---|---|---|
| `assert_equal(comparison: TestComparison) -> Bool` | Boolean | Check the comparison's recorded proof fields. |
| `compare<E>(cases: [Dyn], reference: fn(Dyn) -> Dyn E!, candidate: fn(Dyn) -> Dyn E!, relation: String) -> TestComparison E!` | comparison | Compare reference and candidate outputs over explicit cases. The callbacks' shared failure type passes up (D-CALLBACK-ERR1=A); callbacks that cannot fail make the comparison `Never!`. |
| `snap(name: String, value: String) -[FS, Env]> Bool` | Boolean | Compare or create `__snapshots__/<sanitized-name>.snap`; `JET_UPDATE_SNAPSHOTS=1` rewrites it. Snapshot names retain ASCII letters, digits, `-`, and `_`, replacing other scalars with `_`; an empty name becomes `case`. |
| `golden(name: String, value: String) -[FS]> Bool` | Boolean | Check a named golden value. |
| `fixture(name: String) -[FS]> String` | text | Load a named fixture. |
| `temp_dir(prefix: String) -[FS]> String` | path | Allocate an isolated temporary directory. |
| `corpus(glob: String) -[FS]> [String]` | paths | Select files matching a corpus glob. |
| `fake_clock(unix_ms: Int) -> Clock` | clock | Create a deterministic clock at a Unix-millisecond value. |
| `fake_rng(seed: Int) -> RNG` | RNG | Create deterministic random state. |
| `fake_data(seed: Int) -> Fake` | fake | Create a deterministic fake-data carrier. |
| `test_suite() -> Test` | suite | Create the default suite record. |
| `world(body: fn(DeterministicWorld))` | — | Run a body with deterministic world services. |
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
`Err`, so `rx :: re.compile(text)` propagates inside a fallible function. The
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
rendering capabilities. The source is [`Core/Ui/Ui.jet`](../../../Core/Ui/Ui.jet).

`box` stores its children in the returned tree. Its runtime boundary therefore
accepts an owned child list; compiler borrow-elision must not treat that native
call as lending the caller's list.

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
| `preview(name: String, viewport: ?UIPreviewViewport, body: fn() -> UINode) -> UIPreview` | preview | Describe a named preview. |
| `playground(name: String, viewport: ?UIPreviewViewport, body: fn() -> UINode) -> UIPreview` | preview | Describe a named interactive playground. |
| `desktop() -> UIPreviewViewport`, `phone() -> UIPreviewViewport`, `tablet() -> UIPreviewViewport` | viewport | Select standard preview dimensions. |
| `key_event(code: String) -> InputEvent` | event | Create `InputEvent.Key{code: code}`. |
| `resize_event(width: Float, height: Float) -> InputEvent` | event | Create `InputEvent.Resize{size: Size{width: width, height: height}}`. |
| `reactive_render(body: fn())` | — | Ask the provider to render a reactive body. |
| `gtk_backend() -> GtkBackend`, `tui_backend() -> TUIBackend`, `null_backend() -> NullBackend` | backend | Select a backend handle. |
| `aria_role_button() -> UIAriaRole`, `aria_role_container() -> UIAriaRole` | role | Construct standard accessibility roles. |
| `aria_role_label() -> UIAriaRole`, `aria_role_text_input() -> UIAriaRole` | role | Construct label or text-input roles. |
| `node_accessibility(n: UINode, metadata: UIAccessibility) -> UINode` | node | Attach accessibility metadata. |
| `node_color(label: String, width: Float, height: Float, color: String) -> UINode` | node | Construct a colored node. |
| `node_role(label: String, width: Float, height: Float, role: UIAriaRole) -> UINode` | node | Construct a node with a role. |
| `node_shortcut(n: UINode, shortcut: UIShortcut) -> UINode` | node | Attach a shortcut descriptor. |

Preview viewport arguments keep the single optional carrier inserted by the
checker; lowering does not wrap an already promoted argument again. Preview and
playground registries can be imported directly as `core.ui.[previews, playgrounds]`.
Host file-dialog, clipboard, shortcut, accessibility, and IME consumers declare
their corresponding `UI.*` authority needs and use the current failure carrier;
IME polling yields an optional event after host failure propagation.

Backends expose their own command or event projection through the backend
handle. The provider/catalog may add interactive node and mount operations,
but those operations remain typed provider boundaries. Build a portable tree
first, then use backend-specific capabilities; do not infer that a node tree
supplies a platform-native callback API.
The native GTK `present` call runs its application loop until a window
close request quits that loop. GTK's default close handler still owns window
teardown; a signal-owned loop reference keeps its callback data live.
UI backend allocation and host capability/dialog/clipboard/IME/drag polling
enter Jet source before reserved Core-only provider leaves. Public UI
capability admission and the existing per-function Core effect rows remain
unchanged. Lexical filesystem grants and accessibility metadata construction
are Jet policy, not host facade calls.
Terminal/GTK backend allocation, shortcut registration/dispatch, and host
accessibility attach/project also enter their Jet public functions first.
`attach` preserves its public `Unit !UIHostError` contract: Core discards the
private provider's returned canonical node only on success and propagates
the same typed host error on failure.
TUI capability selection, ASCII-only case folding, encoded-OS flag semantics,
dimension parsing, and defaults are Jet policy. The private environment-byte
leaf samples the same startup facts; the shared terminal leaf remains the
canonical stdout `IsTerminal` sample. No environment effect is added to the
existing public capability row.

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
Setting a synchronized signal updates the provider state, not the Jet carrier
binding, so its receiver call uses `count.set(value)` without an `&` mark. UI
button handlers use the trailing block form, `ui.button("Click") { … }`.

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
| `live(key: String, data: String) -> LiveQuery` | query | Create a live-query carrier. |
| `subscribe(key: String) -> LiveQuery` | query | Create a subscribed live-query carrier. |
| `invalidate(key: String) -> Int` / `transact_invalidate(key: String) -> Int` | token | Create invalidation tokens. |
| `live_get(q: LiveQuery) -> String` / `live_show(q: LiveQuery) -> String` | text | Read or display live-query data. |
| `live_stats() -> String` | text | Read provider live-query statistics. |
| `signal_push(q: LiveQuery, payload: String) -> LiveQuery` | query | Push data and advance query generation. |
| `sync(key: String, data: String) -> String` | text | Format a synchronized key/value update. |

`app` and `core.web` live-query operations retain the same runtime-owned
registry as database and data-loader consumers. `LiveQuery` is an opaque
identity, not a source record snapshot; invalidation, generations, statistics,
and sync-session publication always inspect or mutate that one registry.

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
| `from_addr<T>(address: Int) -> *T` | pointer | `#Unsafe` function: reconstitute a pointer; every call sits in an audited `#Unsafe` region. |
| `volatile_read<T>(pointer: *T) -> T` | value | `#Unsafe` function: perform a volatile read; every call sits in an audited `#Unsafe` region. |
| `volatile_write<T>(pointer: *T, value: T)` | — | `#Unsafe` function: perform a volatile write; every call sits in an audited `#Unsafe` region. |

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

User-defined mixed arithmetic names its right operand in the hook declaration:
`impl Money.Add(Int) { ... }`. An untyped `impl Money.Add` is the same-type
hook, not an overload inferred from the method parameter. Add and multiply
hooks may opt into reverse lookup with `#Commutative`, as in
`impl Money.Mul(Int) #Commutative { ... }`; subtract and divide retain their
declared operand order. The exact foundations example exercises both operand
orders without changing its integer or rational representation.

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
and `is_finite`. Named bit methods preserve operand width. A shift keeps its
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
[`Core/Net/Net.jet`](../../../Core/Net/Net.jet).

DNS failures carry the built-in `NetDNSError` inside `NetError.DNS`: `NotFound`
retains the queried name, while `Failure` retains the resolver's message.

`SocketAddr` and `UDPPacket` retain the native provider carrier so resolver and
socket results keep the nominal types accepted by transport operations. Jet owns
IP parsing/canonical rendering, socket-text parsing/rendering, and lossy UDP
UTF-8 decoding. Compiler-private, audited leaves transfer only native address
representation; they do not choose validation, defaults, or error policy.
`SRVRecord` is explicitly assembled from resolver fields.

`socket_addr` checks the port range `0..=65535` and parses numeric IPv4/IPv6
addresses in Jet. Ordinary host names use the platform resolver, including its
configured name-service sources; answers retain platform order and the first
answer is selected without sorting or deduplication. An empty successful
answer reports the existing “no address” network error, rather than a fabricated
address. Explicit `DnsResolver.at` lookups remain separate from ordinary
platform resolution.

| Signature | Result | Description |
|---|---|---|
| `gethostname() -[Env]> String` | text | Read the local host name. |
| `ip_addr(text: String) -> IPAddr !NetError` | address | Parse an IP address. |
| `ip_to_string(addr: IPAddr) -> String` / `ip_is_ipv4(addr: IPAddr) -> Bool` | text/Boolean | Render an address or test its family. |
| `socket_addr(host: String, port: Int) -[Net]> SocketAddr !NetError` | address | Build an address after checking the port. |
| `socket_addr_parse(text: String) -> SocketAddr !NetError` | address | Parse a rendered socket address. |
| `socket_host(addr: SocketAddr) -> String` / `socket_port(addr) -> Int` | scalar | Read address components. |
| `socket_to_string(addr: SocketAddr) -> String` / `socket_type(stream: TCPStream) -> String` | text | Render an address or identify a stream. |
| `tcp_listen(addr: String) -[Net]> TCPListener !NetError` / `tcp_listen_addr(addr: SocketAddr) -[Net]> TCPListener !NetError` | listener | Bind a TCP listener. |
| `tcp_accept(listener: TCPListener) -[Net, Time.Wait]> TCPStream !NetError` | stream | Accept one connection. |
| `tcp_connect(addr: String) -[Net, Time.Wait]> TCPStream !NetError` / `tcp_connect_addr(addr: SocketAddr) -[Net, Time.Wait]> TCPStream !NetError` | stream | Connect to a TCP endpoint. |
| `tcp_connect_timeout(addr: SocketAddr, timeout_ms: Int) -[Net, Time.Wait]> TCPStream !NetError` | stream | Connect with a nonnegative timeout. |
| `tcp_connect_happy(host: String, port: Int, timeout_ms: Int) -[Net, Time.Wait]> TCPStream !NetError` | stream | Connect through the address-family race. |
| `create_connection(host: String, port: Int) -[Net, Time.Wait]> TCPStream !NetError` / `create_server(host, port) -[Net]> TCPListener !NetError` | endpoint | Build host/port TCP endpoints. |
| `send(&stream: TCPStream, text: String) -[Net, Time.Wait]> Int !NetError` | count | Write text through the caller's exclusive stream window. |
| `tcp_read(&stream: TCPStream) -[Net, Time.Wait]> String !NetError` / `tcp_read_bytes(&stream, n) -[Net, Time.Wait]> [U8] !NetError` | text/bytes | Read from a TCP stream. |
| `tcp_read_text(&stream, n) -[Net, Time.Wait]> String !NetError` | text | Read and decode up to `n` bytes. |
| `tcp_write(&stream, text: String) -[Net, Time.Wait]> Unit !NetError` / `tcp_write_bytes(&stream, bytes) -[Net, Time.Wait]> Int !NetError` | unit/count | Write text or bytes. |
| `tcp_write_all_bytes(&stream, bytes) -[Net, Time.Wait]> Unit !NetError` / `tcp_write_text(&stream, text) -[Net, Time.Wait]> Unit !NetError` | unit | Write all bytes or text. |
| `tcp_shutdown(&stream, how: NetShutdown) -[Net]> Unit !NetError` / `tcp_close(&stream) -[Net]> Unit !NetError` | — | Shut down or close a stream. |
| `tcp_reply(^stream, status: String, body: String) -[Net, Time.Wait]> Unit !NetError` | — | Consume a stream, write one checked reply with a three-digit status, and close it. |
| `tcp_ready(&stream, interest: NetReadyInterest, deadline_ms: Int) -[Net, Time.Wait]> NetReady !NetError` | readiness | Wait for a readiness interest until a deadline. |
| `ready_readable(ready: NetReady) -> Bool` / `ready_writable(ready) -> Bool` | Boolean | Inspect readiness. |
| `tcp_local_addr(stream) -[Net]> String !NetError`, `tcp_peer_addr(stream) -[Net]> String !NetError` | text | Read rendered stream endpoints. |
| `tcp_local_socket_addr(stream) -[Net]> SocketAddr !NetError`, `tcp_peer_socket_addr(stream) -[Net]> SocketAddr !NetError`, `listener_local_socket_addr(listener) -[Net]> SocketAddr !NetError` | address | Read typed stream/listener endpoints. |
| `set_timeout(&stream, timeout_ms: Int) -[Net]> Unit !NetError` | — | Set a nonnegative timeout on the caller's stream, without copying the handle. |
| `set_read_timeout(&stream, timeout_ms: Int) -[Net]> Unit !NetError` / `set_write_timeout(&stream, timeout_ms: Int) -[Net]> Unit !NetError` | — | Set read or write timeouts in milliseconds. |
| `nodelay(stream) -[Net]> Bool !NetError` / `set_nodelay(&stream, on: Bool) -[Net]> Unit !NetError` | option | Read or set TCP no-delay. |
| `ttl(stream) -[Net]> Int !NetError` / `set_ttl(&stream, hops: Int) -[Net]> Unit !NetError` | option | Read or set the TTL. |
| `sendfile(&stream, path: String) -[Net, Time.Wait]> Int !NetError` | count | Send file bytes through a TCP stream. |
| `udp_bind(addr: String) -[Net]> UDPSocket !NetError` / `udp_bind_addr(addr: SocketAddr) -[Net]> UDPSocket !NetError` | socket | Bind a UDP socket. |
| `udp_local_addr(socket: UDPSocket) -[Net]> SocketAddr !NetError` / `udp_set_timeout(socket, timeout_ms: Int) -[Net]> Unit !NetError` | address/— | Inspect a UDP endpoint or set its timeout. |
| `udp_send_to(socket, text, addr) -[Net, Time.Wait]> Int !NetError` / `udp_send_bytes_to(socket, bytes, addr) -[Net, Time.Wait]> Int !NetError` | count | Send text or bytes to a datagram address. |
| `udp_recv_from(socket, limit) -[Net, Time.Wait]> UDPPacket !NetError` / `udp_receive(socket, limit) -[Net, Time.Wait]> UDPPacket !NetError` | packet | Receive a bounded datagram. |
| `udp_packet_data(packet)`, `udp_packet_bytes(packet)`, `udp_packet_addr(packet)` | scalar | Read packet text, bytes, or address. |
| `udp_packet_original_len(packet) -> Int` / `udp_packet_truncated(packet) -> Bool` | scalar | Inspect datagram length and truncation. |
| `unix_listen(path: String) -[Net]> UnixListener !NetError` / `unix_connect(path) -[Net, Time.Wait]> UnixStream !NetError` | endpoint | Use Unix-domain sockets. |
| `unix_accept(listener) -[Net, Time.Wait]> UnixStream !NetError` / `unix_read(&stream) -[Net, Time.Wait]> String !NetError` | stream/text | Accept or read a Unix stream. |
| `unix_write(&stream, text) -[Net, Time.Wait]> Unit !NetError` / `unix_read_bytes(&stream, n) -[Net, Time.Wait]> [U8] !NetError` | unit/bytes | Write text or read bytes through the caller's exclusive stream window. |
| `unix_write_all_bytes(&stream, bytes) -[Net, Time.Wait]> Unit !NetError` / `unix_shutdown(&stream, how) -[Net]> Unit !NetError` / `unix_close(&stream) -[Net]> Unit !NetError` | — | Write all bytes or close the same Unix stream. |
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
TCP reply, TCP option access, and UDP bind enter their Jet public functions
before the corresponding reserved `__tcp_reply`, `__set_*`, `__nodelay`,
`__ttl`, and `__udp_bind*` leaves. Raw leaves retain existing native error
mapping, borrow/move conventions, and interpreter-route availability.
UDP address, timeout, send, and receive operations and Unix listen, accept,
read, byte-write, shutdown, and close also enter Jet source first. Their
reserved raw leaves preserve exactly the public operation's existing effect
row and supported interpreter/native tiers.
HTTP client `get`, `post`, and `request`, and server `mux`, `serve_once`,
`serve_once_listener`, `static_file`, `static_file_range`, `cors`, and
`request_id` likewise enter their Jet source functions before the reserved
raw leaves. Existing specialized public admission remains unchanged;
callback-bearing host leaves retain their original unsupported AOT tiers.
HTTP router creation, parsing, and dispatch, TLS byte I/O, and WebSocket
connect/upgrade are also source-owned; the reserved transport/carrier leaves
keep their old effects (including no new WebSocket effect) and tier support.
Reserved Unix connect, TLS client handshake, HTTP serve/bind, static-files,
and middleware leaves use full positional signatures inside Core. Their
public overload selection, implicit option defaults, and escaping callback
admission still precede source forwarding; registration of a raw alias alone
does not replace those public admission checks.

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

### `core.net.tls` — TLS stream wrapper

`core.net.tls` upgrades a connected TCP stream to a verified TLS 1.2/1.3
stream. It is Jet source over the vendored Mbed TLS 3.6 engine; configuration
carriers are validated before network use. See [`Core/Net/Tls.jet`](../../../Core/Net/Tls.jet).

| Signature | Result | Description |
|---|---|---|
| `ClientConfig.default()` | `ClientConfig` | Start with system trust and inclusive TLS 1.2–1.3 version bounds. |
| `ClientConfig.default().with_alpn(protocols)` | `ClientConfig !IOError` | Validate and offer the configured ALPN protocol list. |
| `RootCertificates.from_pem(bytes)` | `RootCertificates !IOError` | Validate a custom PEM root bundle before network use. |
| `ClientIdentity.from_pem(cert_chain: bytes, private_key: bytes)` | `ClientIdentity !IOError` | Validate a PEM certificate chain and matching private key. |
| `ClientConfig.default().with_trust(policy)` | `ClientConfig !IOError` | Select `.System`, `.SystemPlus(roots)`, or `.CustomOnly(roots)` trust. |
| `ClientConfig.default().with_client_identity(identity)` | `ClientConfig !IOError` | Add a validated mTLS client identity to an immutable configuration. |
| `ClientConfig.default().with_version_bounds(min: version, max: version)` | `ClientConfig !IOError` | Select inclusive `.TLS12` / `.TLS13` bounds; reversed bounds fail before network use. |
| `tls.client(^tcp, server_name:, config:, deadline:)` | `TLSStream !NetError` | Consume the connected TCP stream, verify the server name, apply configuration, and use the earliest handshake deadline. |
| `client(host: String, port: Int)` | `TLSStream !IOError` | Convenience form that connects and verifies a host and port. |
| `read(&stream: TLSStream, n: Int)` / `read_text(&stream, n)` | `[U8] !IOError` / `String !IOError` | Read bounded TLS bytes or checked UTF-8 text. |
| `write(&stream: TLSStream, bytes: [U8])` / `write_all(&stream, bytes)` | `Int !IOError` / `!IOError` | Write application bytes, partially or completely. |
| `write_text(&stream: TLSStream, text)` / `close(stream)` | `!IOError` | Write text or send close-notify on the same handle; repeated close is harmless. |
| `stream.peer_identity()` | `TLSPeerIdentity` | Read the verified server name, certificate chain, cipher suite, and negotiated TLS version. |
| `stream.read(limit, deadline:)` / `stream.write_all(bytes, deadline:)` | matching results | Apply explicit per-call deadlines on the same TLS handle. |
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
`tls.client(^tcp, server_name:, config:, deadline:)` keeps the immutable
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
kernel. Import it with `use core.db.sql as sql`; the former Core-private
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
in its `priv module internal`, rather than enlarging the public surface.
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
examples under [`Examples/features/tooling/`](../../../Examples/features/tooling/).
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
| `mse_loss(pred, target) -> Tensor !ComputeError` | tensor | Compute a one-element mean-squared-error tensor. |
| `sgd_step(weights, grad, lr: Float) -> Tensor !ComputeError` | tensor | Apply a value-based SGD step. |
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

Restart/delivery policy constructors and runtime retention projection are Jet
policy. `state_store` requires `FS.Read`: it inspects the target and lexical
ancestors with metadata-only reads, without normalizing away `..`, refusing
symbolic links and unsuitable file kinds while allowing absent paths. Generation/event/dead-letter getters
read native storage metadata without reconstructing scheduler state.
The private metadata carrier does not acquire a Windows identity handle;
constructor validation therefore adds no identity-open failures.

`ServiceError` and `DeliveryReceipt` are ordinary Jet enum/record values.
All ten error cases carry a String; their service display is the message alone.
Receipts retain every original field, including authority and signature, and
their service display preserves the existing field order and labels.
`TaskOutcome` retains Finished, Panicked(String), Cancelled and DeadlineBlown:
display uses the raw panic reason, while Debug quotes and escapes that String.
`core.rt.RealtimeReceipt` preserves the eight scalar provider facts in their
original positional order.

Restart/delivery/state-empty selection and dead-letter draining enforce their
original validation order in Jet. Private single-field host leaves only read
or write the existing service tree storage, under the caller's exclusive borrow.
The signing key remains the exact 32 bytes decoded from the current `sa-`
authority token; this port adds no key cache or security-policy change.
The Core-internal `__authority_signing_key` shares that one decoder with jobs;
it is source policy, not a second host signer or an additional authority store.
Endpoint input and authority-status validation also live in that module.
The reserved `__authority_validate` preserves native check order, including
partition-before-generation for an exact registry entry, and reads one
consistent snapshot of the existing endpoint registry for its fallback scans.
The fresh-tree constructor supplies all initial fields in Jet, in their native
evaluation order: generation 1, OneForOne restart with a 5/60000ms budget,
AtMostOnce delivery, no authority/snapshot/upgrade, and empty topology/state
stores. Its two temporary host leaves only allocate the existing task runtime
and materialize those supplied fields. Seven private type-name bindings expose
the existing field types without introducing a new carrier; both compiler
mappers bind the same names and retire at the Jet-runtime cutover.

Retaining service transports receive copies of borrowed caller values
(names, policies, messages, and endpoint identities). A `.copy()` result is
fresh and passes without a `^` receiver mark; `^` moves locally owned
mailboxes and queues instead, preserving their channel identity.

Worker registration, failure/restart selection, restart-window counting and
mailbox-depth selection are Jet policy. Registration retains the checked
top-level function item, validates before entropy/allocation/publication, and
supplies every worker seed. Failure retains declaration-order group scope,
strict `at > saturating_sub(now, per_ms)` windows, reverse authority rollback,
escalation, durable-mailbox preservation and partition reassertion in native
write order (`Services.rs:2917–3078`, restart helper `1983–2012`). The depth and
count readers deliberately retain native's weaker name/tree match after
endpoint validation; they do not substitute the stricter routing predicate.

Worker mechanical transports **die at the Jet-runtime cutover**: one aggregate
parts leaf supplies only absent worker/group supervision facts; setters write
the existing fields, and partition status/failure share the original single
mutex acquisition (`3070–3073`). Seven paired private aliases preserve exact
native shapes: `__WorkerMailbox` (`JetServiceMailbox`, `758–762`, fields
endpoint/capacity/channel), `__WorkerChannel` (the Arc channel at `761`),
`__WorkerHandler` (`JetServiceWorkerHandler = fn()`, `821`), `__WorkerStatus`
(`JetServiceSupervisorStatus`, `123–130`), `__WorkerTopologyKind`
(`JetServiceTopologyKind`, `134–137`), `__WorkerTopologyState` and
`__WorkerRestartReason` (the existing DevtoolsTopologyPanel enums at `62–70`
and `282–288`). Only the existing checked worker parameter forwards a function
pointer. Mailboxes move: native `Mailbox::clone` forks its channel and is not
a valid source transfer. Alias, checked-slot and no-clone fixtures cover these
seams; they introduce no state, policy cache or replacement runtime carrier.

Snapshot/event-log selection, replay framing, topology/observation text, and
workflow history/outcome selection are Jet policies. Durable reads retain the
existing host storage/replay operation during the Rust-seed tier; field
projections expose only existing facts and retire at the Jet-runtime cutover.
Group membership validation and chaos-failure ordering are Jet policies.
Group reparenting retains its original per-worker order, and chaos increments
its checked counter before selecting the first running worker.
Event append and snapshot commit validate their adapter, authority, payload,
and limits in Jet and frame UTF-8 byte lengths there. Snapshot persistence and
the snapshot field assignment remain inside the same existing host mutex;
event persistence retains the existing locked store append before the local
event record is appended. No lock guard or second authority is exposed.
Directory resolution owns lookup, generation errors, expiration remapping,
and running/draining checks in Jet; its proof check calls the existing
authority operation without adding a registry or signing-key cache.
Tree sends validate routing and payloads in Jet, preserve the native repeated
authority checks, remap mailbox-capacity failures to the worker-specific text,
and update accounting only after successful atomic authority sends.
Durable tree sends and receives own local deduplication, pending-message
admission, receipt marking, and rollback orchestration in Jet. Each existing
authority/storage operation retains its internal locks; a failed pending send
requeues the remaining authority suffix before restoring the pre-send mailbox.
Combined rollback failures preserve the original diagnostic and recovery text.
Generation rollout and start/stop policies remain native pending a reviewed
lock-lifetime seam; this source port does not claim those rows are complete.

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
`Examples/features/io/archive_hostile.jet` exercises truncated, bad-checksum,
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
- Assignment that should not change → `name :: value`
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
`HTTPMux.middleware` forwards only after the existing escaping-callback
admission. The original effects and resource conventions remain in force.

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
