# Incremental checking: prior art, Jet design, and staged plan (2026-09-30)

This note is the design behind card #2517 and its follow-up stages. Plans,
state, and criteria live in Tower. This file records what other compilers do,
what Jet takes from each, the design that follows from the owner's rulings,
and the order in which the work can land.

Owner inputs this design follows:

- **Priority.** A fast bootstrap and dogfood loop for the self-hosted
  compiler, with no resident service (D-JPK-NODAEMON1 stands).
- **Ruling, 2026-09-30.** Incremental checking uses per-package interface
  records on disk, in the style of Go and GHC. It also takes the strongest
  parts of Rust's incremental query system: fine-grained dependency tracking,
  red/green invalidation, early cutoff, and an on-disk query cache.
- **Package rule.** A package is either a directory with `package.jet`
  (recursive, minus nested packages) or a single file with a leading
  `package { … }` header. All files of one package share one namespace
  (D-MOD-CYCLE1=A). Packages form an acyclic graph.
- **Ratified contracts still in force.** D-INCR-UNIT1=A (three layers: item
  reuse, interface fingerprints that invalidate importers only, sealed
  package artifacts; nothing skips sema or diagnostics on a hit that still
  needs checking). D-BUILD-NOCHANGE1=A (a memoized check counts as checking
  when its key covers every input; `--no-cache` and `--verify` are the expert
  hatches). D-BUILD-STORE1=E (the `jet-store` API: content-addressed blobs,
  action records, atomic publication, verification, leases, capacity).
  D-LIB-REUSE1=B (sealed package objects; generic bodies travel as typed IR
  and are instantiated where they are used).
- **Ruling, 2026-09-30 (storage, final).** The shared content-addressed cache
  (the `jet-store` record and artifact store) stays under `~/.cache`.
  Everything else, which is per-workspace state (last-run pointers, stamps,
  the receipts index, reports, build outputs, the records index, logs), lives
  in one `.jet/` folder at the workspace root, never in per-package `.jet/`
  folders. Code that creates `.jet/` paths resolves the workspace root first.

The owner's package ruling changes one thing in D-INCR-UNIT1: layer 2 is
now the **package** interface, not the module interface. Inside a package,
item-level reuse (layer 1) takes the role that module granularity used to
play.

## 1. Where Jet stands today

| Area | Current state | Evidence |
|---|---|---|
| Unit of checking (Rust) | One `ProgramBundle` holds every module of the program, Core modules included. Every phase runs over the whole bundle. | `crates/jet-sema/src/Sema/Bundle/Pipeline/CheckInner.rs`, `Completion.rs:764-772` ("Core modules are checked in the same bundle") |
| Item cache (Rust) | `IncrementalSemaCache` keeps checked `Func` values, diagnostics, effect summaries, comptime inputs, address-taken sets, ledger references, and pending diagnostics per function. It lives in process memory only and is enabled only for `CompileMode::Check`. | `crates/jet-sema/src/Sema/Bundle.rs:755-920`, `Bundle/Validation.rs:509-641`, `Completion.rs:237-243` |
| Module invalidation (Rust) | `begin_bundle` marks a module dirty when its interface fingerprint or resolved imports change, then dirties the reverse import closure. | `Bundle.rs:823-897` |
| Query engine | `jet-queries` records dependencies and revisions but has no output fingerprints. A recomputed query always bumps its generation, so there is no early cutoff. There is no persistence. | `crates/jet-queries/src/lib.rs:144-176` |
| Cross-process reuse | The Receipt (card #2517, batch 1) replays a whole unchanged invocation from typed diagnostics. Any source edit falls back to a full check. | `crates/jet-store/src/receipt.rs`, `Source/CheckReceipt.rs` |
| Self-hosted driver | `JetDriver` loads packages with identities (`JetDriverPackageInfo`, `JetDriverSourceUnit.package_identity`). It has no digest-keyed reuse beyond the Receipt closure in `Identity.jet`. | `Compiler/JetDriver/Source/Driver/Identity.jet:47-88,266-304` |
| Self-hosted sema | Checking returns values (`SemaGraphModuleResult`, `TFunc` with `effects`) instead of mutating the AST. The effect solve still runs over one graph that holds every module. | `Compiler/JetSema/Source/Sema/CheckProgram.jet:20-44,2254-2421`; `Effects/Checks.jet:1109-1166` |
| Scale | Rust `jet check` time per line grows from 4.6 ms at 31k lines to 12.2 ms at 58k lines, and memory grows about 2.4 times for 1.9 times the lines. The self-hosted compiler has 223 files and about 171k lines in nine packages; JetSema alone has about 57k. | `Docs/research/check-scaling-2026-09-30.md`; `~/.cache/jet-luna/compiler-modules/inventory.json` (#3862) |

**Why the Rust checker cannot skip a module today** (PersistCheckCache's
report, confirmed in source):

1. Body checking rewrites each `Func` in place: it fills in types, lowers
   desugarings, and narrows failure carriers (`Completion.rs:263-329`;
   `solve_inferred_failure` rewrites `return_type` at
   `crates/jet-sema/src/Sema/mod.rs:302-321`). Codegen consumes the rewritten
   AST.
2. The completion phases after body checking walk every module's checked
   bodies again. These phases are the effect solve, #3708 failure inference,
   liveness, taint, `used_core`, memory facts, the web partition, the app
   graph, and others (`Completion.rs:330-772`).
3. Several accumulators are bundle-global: `global_addr_taken`,
   `embed_inputs`, the one `NameLedger`, the devtools registry, and effect
   summaries keyed by loader aliases.

To skip a module across processes under that structure, the checked AST
would have to be stored losslessly. That is PersistCheckCache's option A,
and it is very large. The package design below avoids it: a package is
never checked against a dependency's bodies, only against its interface. The
completion phases then become per-package phases whose facts cross package
edges as summaries.

## 2. How other compilers do it

### rustc: query DAG, red/green, fingerprints, on-disk cache

- Every query invocation is a node in a dependency DAG, and the DAG records
  the order in which each node read its inputs. After a run, rustc saves the
  DAG, a 128-bit fingerprint of each result, and selected results. The
  **try-mark-green** step colors a node green when all of its recorded reads
  are green, without re-executing the node or loading its value. A node with
  a red read is re-executed. If its new fingerprint equals the old one, it
  is still green (**early cutoff**), so its dependents are not touched.
  Reads are replayed in their original order because a changed early read
  can change the later control flow.
  [Incremental compilation](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html)
- Results on disk use stable identities (`DefPathHash`, not session-local
  `DefId`), and fingerprints are computed by stable hashing over those
  identities. Fingerprinting is "the main reason why incremental compilation
  can be slower than non-incremental compilation". Values are loaded only
  when a green node's value is actually needed. **Cache promotion** reloads
  green results before the new cache is written, so an intermediate result
  is not lost. `eval_always` covers queries that read ambient inputs, and
  "projection queries" act as firewalls that keep small dependents green
  when a large query changes. Spans are volatile, so including them in
  results causes needless recomputation
  ([rust#47389](https://github.com/rust-lang/rust/issues/47389)).
  [Incremental compilation in detail](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html)
- The backend reuses whole codegen units: a CGU dep-node that marks green
  means its object file on disk is still valid (same source).
- The dependency graph is per crate. The build-optimization proposal
  (`Docs/proposals/automatic-build-optimization.md`, II.4) observes that a
  dependent crate recompiles whenever its dependency's crate hash changes,
  and that hash covers every body. That is why the proposal splits crates
  into interface and implementation halves.

**Jet takes:** stable identities in every record; fingerprints over results
with early cutoff; ordered read lists for item nodes; results loaded lazily
only when a green node's value is demanded; cache promotion; projection
firewalls (the interface record is the firewall in front of a package's
bodies); `eval_always`-style handling for ambient reads, which here means
recorded, verified discovered inputs. **Jet does not take** a fine-grained
graph over every internal query of the whole program. Jet uses a coarse
package graph and applies item-level nodes only inside a package that is
being rechecked. That keeps the fingerprinting cost rustc warns about
bounded.

### Go: action IDs, export data, content IDs, cached output

- A build action's **action ID** hashes all of its inputs. For a compile
  action that includes, for every import, the **content ID of that
  dependency's export data**, not of its object code
  (`fmt.Fprintf(h, "import %s %s\n", p1.ImportPath, buildExportID(a1.buildID))`,
  [`cmd/go/internal/work/exec.go`](https://github.com/golang/go/blob/master/src/cmd/go/internal/work/exec.go)).
  A body edit that leaves export data unchanged therefore leaves every
  importer's action ID unchanged. That is package-level early cutoff.
- Build IDs record `actionID/contentID(export)/contentID(object)`. Keying
  the next step on content IDs rather than action IDs is what lets
  self-hosted builds converge
  ([`buildid.go`](https://github.com/golang/go/blob/master/src/cmd/go/internal/work/buildid.go)).
  The tool ID is the compiler binary's content hash on development builds
  and the release version string on releases.
- The compiler's text output is cached under
  `cache.Subkey(actionID, "stdout")` and replayed on a hit (`showStdout`,
  same file), so warnings reappear on cached builds.
- Export data (Unified IR) is a sectioned binary file with a string table,
  element indexes, and a trailing SHA-256 fingerprint
  ([`noder/doc.go`](https://github.com/golang/go/blob/master/src/cmd/compile/internal/noder/doc.go)).
  Before writing it, the compiler's linker step "updates compiler extensions
  data (e.g. inlining cost, escape analysis results)" and "prunes out any
  unnecessary details (e.g. non-inlineable functions)"
  ([`noder/unified.go`](https://github.com/golang/go/blob/master/src/cmd/compile/internal/noder/unified.go)).
  Inferred facts therefore travel with the interface, and bodies travel only
  where importers need them.
- `go/analysis` makes whole-program analyses modular with serialized
  per-package **facts**. The driver computes facts for dependencies first,
  and the encoding "must be deterministic, to avoid spurious cache misses"
  ([`go/analysis/doc.go`](https://github.com/golang/tools/blob/master/go/analysis/doc.go)).

**Jet takes:** the package as the unit; importers keyed on the dependency's
interface content digest; inferred facts (effects, failures, memory) in the
interface the way Go carries escape and inlining results; bodies in the
interface only where importers need them (generics, `#Inline(Always)`,
compile-time evaluation); cached diagnostic output replayed on a hit (typed
in Jet, not bytes); deterministic encoding; tool identity from the binary's
content digest on development builds and the version string on releases.

### GHC: `.hi` interfaces, per-entity fingerprints, usages

- An interface file holds the exports, direct dependencies, **usages**, and
  declarations, plus several hashes: the source hash, the ABI hash (exports
  and declarations), the export-list hash, the orphan hash, and the
  interface hash
  ([Recompilation avoidance](https://gitlab.haskell.org/ghc/ghc/-/wikis/commentary/compiler/recompilation-avoidance)).
- Every exported entity has its own fingerprint, and that fingerprint
  includes the fingerprints of the external names it mentions. A module
  records a usage for each external entity it names, with the entity's
  fingerprint. It is recompiled only when one of those fingerprints changes.
  Adding an export that nobody uses therefore recompiles no importer.
- Unfoldings (inlinable bodies) are part of an entity's fingerprint under
  `-O`, so importers that may have inlined an old body recompile. Instances
  are attached to their class or type so that coherence changes propagate.
  Orphans need special handling. Mutually recursive groups are fingerprinted
  as one unit. Collections are sorted canonically so fingerprints stay
  stable. Only direct information is stored, which keeps checks fast.
- Template Haskell and plugins read implementations, so a module that uses
  them also depends on object hashes.

**Jet takes:** per-item interface fingerprints and recorded usages, so an
importer rechecks only when an item it names changed; a body digest folded
into an item's fingerprint only for bodies that importers consume (generic
templates, inline bodies, and comptime-evaluated functions, which play the
role of GHC's unfoldings and TH); fingerprints over whole mutually
recursive groups (per strongly connected component); canonical ordering;
impls attached to their trait or type (the package-level orphan rule means
Jet has no orphans); direct information only.

### Swift: provides/depends, fingerprints, priors, emit-module pipelining

- Within a module, the driver tracks what each file **provides** and
  **depends on** (top-level, nominal, member, and dynamic-lookup names). The
  depends sets come from instrumented name lookups. "The golden rule of
  dependency analysis is to be conservative." External modules are tracked
  per module. Because every file resolves its imports, "if an external
  dependency changes, everything in the module is rebuilt"
  ([DependencyAnalysis.md](https://github.com/swiftlang/swift/blob/main/docs/DependencyAnalysis.md)).
- The newer driver keeps a serialized **priors** graph between builds. It
  tracks fingerprinted external dependencies so that a changed imported
  module invalidates only the nodes that use what changed
  ([`ModuleDependencyGraph.swift`](https://github.com/swiftlang/swift-driver/blob/main/Sources/SwiftDriver/IncrementalCompilation/ModuleDependencyGraph.swift)).
  The dependency graph can change during a build, so Make-style static
  graphs do not work ([Driver.md](https://github.com/swiftlang/swift/blob/main/docs/Driver.md)).
- An **emit-module** job produces the module interface early with
  `-experimental-skip-non-inlinable-function-bodies-without-types`, so
  dependents can start before the bodies are compiled
  ([`EmitModuleJob.swift`](https://github.com/swiftlang/swift-driver/blob/main/Sources/SwiftDriver/Jobs/EmitModuleJob.swift)).

**Jet takes:** read tracking at the name-lookup layer (sema's resolver
records what each item body read); a persisted priors graph with a
conservative fallback when the priors are missing or corrupt; per-usage
invalidation across the package edge. Swift's whole-module rebuild on any
external change is exactly what the usages avoid. **Emit-interface
pipelining** is taken only for items with fully declared contracts. Jet
infers effects and failure sets (#3708, D-FAIL-INFER-UNION1), so an item's
interface can depend on its body. An interface-only pass may skip a body
only when the item's signature, failure contract, and effect row are all
written.

### TypeScript: `.tsbuildinfo`, `.d.ts` signatures, project references

- For each file, the builder keeps a **signature**, which is the hash of its
  emitted `.d.ts`. Dependents are affected only when that shape changes
  (`updateShapeSignature`,
  [`builderState.ts`](https://github.com/microsoft/TypeScript/blob/v5.9.3/src/compiler/builderState.ts)).
  `.tsbuildinfo` persists the file versions and signatures, the
  `referencedMap`, the pending emits, and **`semanticDiagnosticsPerFile`**.
  Unchanged files replay their stored diagnostics
  ([`builder.ts`](https://github.com/microsoft/TypeScript/blob/v5.9.3/src/compiler/builder.ts)).
- Project references check a project against its dependencies' `.d.ts`
  outputs. `tsc -b` acts as if `noEmitOnError` were on, because otherwise
  "you'd only see [an error] once" when a later build skips an up-to-date
  project
  ([Project References](https://www.typescriptlang.org/docs/handbook/project-references.html)).
- `--isolatedDeclarations` (TS 5.5) requires exported items to be annotated
  so that declarations can be generated without a checker. That enables
  parallel declaration emit and parallel checking of dependent projects
  ([TS 5.5 notes](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-5-5.html#isolated-declarations)).

**Jet takes:** dependents check against the interface only; the interface
digest acts as a shape signature; per-package typed diagnostics are stored
and replayed, which removes the "errors appear only once" problem instead of
suppressing output; the isolated-declarations insight that written contracts
make interface extraction independent of bodies. **Jet does not take**
timestamp-based up-to-date checks. Stamps may skip a hash, but they never
decide freshness.

### Skip (skiplang) and Hack

- Skip's type checker is written as **memoized functions** over names:
  `memoized fun fun_def(embeddedMode, funName)` and
  `memoized fun class_def(…)` in `skipTyping.sk`,
  `memoized fun maybeGetFun(funName)` in `skipNaming.sk`, and
  `memoized fun getFun(funName)` / `definitions(file)` in `skipExpand.sk`
  ([archived skiplang/skip, `src/frontend`](https://github.com/skiplang/skip/tree/master/src/frontend)).
  The runtime records which memoized calls and which mutable cells each
  memoized call read. When a cell changes, it invalidates the dependents.
  Memoized functions must be transitively side-effect free and must return
  frozen values; the compiler enforces this. "Traces" avoid recomputing
  dependents when a result did not change, which is early cutoff
  ([How memoization works](https://github.com/skiplang/skip/blob/master/docs/blog/2017-01-04-how-memoization-works.md)).
- The checker is kept warm by `skip_server`, a resident loop
  (`src/server/skipServer.sk`). Hack likewise relies on a resident process,
  saved state, fine-grained reverse dependencies, and randomized checks that
  incremental results equal clean results
  (`Docs/research/mine-for-jet-2026-09-30-roc-skip.md`).

**Jet takes:** item checks as pure functions of immutable inputs, keyed by
stable names, with recorded reads and early cutoff. JetSema already returns
values instead of mutating, which makes this natural in the self-hosted
compiler. Jet also takes Hack's "incremental must equal clean" proof, as
`--verify` plus a randomized edit-replay test. **Jet does not take** the
resident process (owner priority, D-JPK-NODAEMON1). Persistence stands in
for residency.

### What Jet takes, in one table

| Mechanism | From | Jet form |
|---|---|---|
| Package as the reuse unit; importer keyed on dependency interface digest | Go, GHC, TS project refs | package check key includes the interface digest of each direct dependency |
| Inferred facts in the interface | Go (escape, inline cost), `go/analysis` facts | effect, failure, memory, taint, ownership, and web facts per exported item |
| Per-item fingerprints and usages | GHC | an importer rechecks only when an item it named changed |
| Red/green with early cutoff; ordered reads; lazy load; cache promotion | rustc, Skip | item graph inside a red package; package-level cutoff on interface digest |
| Stable identities; stable hashing; spans kept out of fingerprints | rustc, GHC | package-qualified item paths; spans relative to their item; declaration locations in a section outside the digest |
| Replay of recorded diagnostics | Go (stdout), TS (`semanticDiagnosticsPerFile`) | typed diagnostics per package, rendered for each invocation |
| Bodies only where importers need them | Go export data, GHC unfoldings | generic templates, inline bodies, and comptime-callable bodies, each with its own fingerprint |
| Written contracts allow body-free interfaces | Swift emit-module, TS isolated declarations | interface-first scheduling for fully declared items only |
| Deterministic encoding; trailing checksum; sections | Go UIR, `go/analysis` | std-only binary record codec (I6) |
| Incremental equals clean | Hack, D-BUILD-NOCHANGE1 | `--verify`, `--no-cache`, randomized edit replay |

## 3. The Jet design

### 3.1 The one rule

> A package check reads its own sources, its own manifest, the compiler and
> Core identity, the target facts that sema reads, the interface records of
> its direct dependencies, and the discovered inputs it declares. It never
> reads a dependency's bodies except the ones published in that dependency's
> interface, and it never reads the root program's policy.

Everything else follows from this rule. Checks that the root imposes on
dependencies run at the program level against interface facts. These
include effect budgets (D-EFFBUDGET1), application authority grants,
unreachable exports, and web partition promotion. Because of that, a
dependency's record is valid for every program that uses it, the same way a
Go package's cache entry is shared by every binary. The rule also removes
today's leak in the Rust cache key, where the root's `package_guarantees`
enter every module's environment (`Bundle.rs:922-967`).

### 3.2 The package interface record

The record is keyed by content and made of canonical sections. Each
exported item carries its own fingerprint. The **interface digest** is the
SHA-256 of the canonical encoding of the sections marked "digest" below.

| Section | Contents | Digest |
|---|---|---|
| Header | schema `jet.iface/v1`, package identity, compiler and Core identity, edition, direct dependency identities with the interface digests this record was checked against | yes |
| Declarations | every `pub` and `pub(package)`-visible declaration as the checked declaration AST with bodies erased: functions and methods with parameter types, access conventions (`^`, `&`, views), written and **inferred** failure contracts, type parameters and bounds; structs, enums, distinct types, aliases, unit families, tags, effect declarations, markers, facts, protocols; `pub use` re-exports resolved to their owner | yes |
| Trait and impl table | traits with method signatures and **declared dispatch effect bounds** (D-EFF3); every impl with its trait identity, type identity, where-clauses, and method signatures, attached to the trait's or the type's package (D-MOD-CYCLE1's package-level orphan rule); derives; delegation facts | yes |
| Constants | comptime-evaluated values of `pub` constants and typed field defaults, as typed values, so importers never re-evaluate them | yes |
| Effect summaries | for each exported callable: the solved rows (`effects`, `panic`, `taint`, `secret`, `calls-exec`) from `solve_reachability` (`crates/jet-sema/src/Sema/Effects.rs:1441-1495`); flags `maximal`, `unbounded_trait_dispatch`, and "no outcome" (for E0433); `#(via f)` pass-through as a summary parametric in the callback parameter; for generic callables, the unsolved edges to trait-method nodes so that a use site can substitute concrete impl rows | yes |
| Failure sets | #3708: the inferred failure set of each unannotated exported function (empty means `Never!`), the union D-FAIL-INFER-UNION1 lets callers match, and the E2404 obligations settled at export | yes |
| Ownership and memory summaries | per callable: returned-view sources (`Receiver`, `Parameter(i)`, `Static`), consumed and escaping parameters, region caps, `#Close` protocol facts on nominals, per-fact memory projections (`MemoryFacts::project_memory_fact`), strongly held type sets for D-SHARED-CYCLE1 with holes for type parameters | yes |
| Target and web facts | OS gates per callable (D-OSTARGET1), natural web bucket, marker, and ceiling per function, ABI export shapes | yes |
| Templates | typed bodies that importers consume: generic functions and methods, generic modules (D-CONF-GENSPELL1), `#Inline(Always)` bodies, and bodies of functions reachable from compile-time evaluation. Each template has its **own** fingerprint and enters the digest only through the items that expose it | per item |
| Package facts | `used_core` set, FFI callback set, exact-int reachability, inferred runtime layer, job table, devtools publications, output declarations, package-level effect union for budget checks | yes |
| Usages | for each direct dependency: the items this package named or read, with the item fingerprints seen (GHC usages). Used for invalidation and for program-level export liveness | no (checked, not published) |
| Locations | declaration spans and origin revisions for each exported item, used only to render "defined here" labels in importers' diagnostics | **no** |

Two rules keep the digest stable:

- **Symbolic locations.** An importer's diagnostic that points into a
  dependency stores `(package, item path, label)`, not a byte span. The
  renderer resolves it from the current Locations section. Moving a
  declaration inside its file therefore changes no importer's record. This is
  rustc's span lesson applied at the package edge.
- **Stable identities.** Keys are package-qualified semantic paths
  (`NameLedger::semantic_identity`), never loader aliases or indices. Today's
  effect keys such as `alias::name` (`Completion.rs:314-328,475-495`) must
  move to these paths (rustc's `DefPathHash` lesson).

### 3.3 Fingerprints and keys

| Name | Definition | Role |
|---|---|---|
| **Compiler identity** | the content digest of the `jet` binary on development builds, the version string on releases (Go tool ID), plus the Core source digest and the record schema versions | part of every key; a change empties reuse once |
| **Source digest** | SHA-256 over sorted `(package-relative path, SHA-256 of bytes)` for member files, plus the manifest bytes | package check key |
| **Target facts** | only the facts sema reads: active OS, target machine and sema-visible features, edition, gates (`Bundle.rs:922-967` minus the build clock, as today) | package check key |
| **Discovered inputs** | files, `embed_*` inputs, and environment reads made during the check, as `(label, digest)` pairs, all through the sealed reader (card criterion 2/6) | verified before reuse, like the Receipt's `inputs` |
| **Package check key** | H(compiler identity, target facts, source digest, direct dependency interface digests) | the Go action ID; addresses the check record |
| **Interface digest** | SHA-256 of the canonical digest sections | the Go export content ID and GHC ABI hash; importers key on it |
| **Item fingerprint** | H(item's interface payload, fingerprints of the external items it mentions, template fingerprint where one is exposed); computed over each strongly connected component as a group | usages; item-level cutoff across the package edge |
| **Item input fingerprint** | H(canonical item AST with spans relative to the item, fingerprints of every declaration and summary it read, in read order) | item red/green inside a package |
| **Program key** | H(root package identity, verb and mode, all package interface and summary digests in the closure) | the program-level record |

Hashing uses the existing std-only SHA-256. Stamps (size, mtime, ctime,
inode) may skip rehashing an unchanged file but never decide freshness (as
in the build-optimization proposal).

### 3.4 Package graph: red/green with early cutoff

The program's packages are visited in topological order, with independent
packages in parallel. For a package P:

1. Compute P's check key from its source digest and the interface digests
   its dependencies produced **in this run**.
2. **Green by key.** If the store holds a check record for that key and every
   discovered input still verifies, P is green. Only the record header,
   interface digest, and diagnostics pointer are read. Bodies, templates,
   and item graphs stay on disk (rustc: color without loading).
3. **Green by usage.** Otherwise, if P's sources are unchanged since its
   previous record (from the priors), and each dependency whose interface
   digest changed still has the same fingerprint for every item in P's
   usages, then P is green. Its previous results are republished under the
   new key, which is rustc's cache promotion and GHC's usage check.
4. **Red.** Otherwise P is checked against its dependencies' interfaces,
   reusing items from its previous record (section 3.5). This publishes a new
   record.
5. **Cutoff.** If P's new interface digest equals the previous one,
   importers see the same key input and stay green without further work.
   If it differs, importers go through step 3, and only those whose usages
   touch a changed item become red.

A comment-only edit rechecks one package, reuses every item in it, and
yields the same interface digest, so no importer runs. A private body edit
does the same unless it changes an exported inferred fact. When a body edit
changes a `pub` function's effect row or failure set, the interface digest
changes, and only importers that named that function recheck.

### 3.5 Inside a red package: items

A red package runs as a small persisted query graph over its items
(Skip-style memoized item functions; rustc-style ordered reads). The node
kinds are:

- `decl(item)`: the checked declaration. Its output is the item's interface
  payload.
- `body(item)`: body checking. Its outputs are diagnostics, the local
  summary (effect edges and direct effects, failure edges, memory calls,
  taint returns, view sources), name references, discovered inputs, and the
  checked body for build.
- `solve(scc)`: package-local fixpoints over the condensed call graph for
  effects, failure inference, memory, and taint. Dependency functions enter
  as **sealed nodes** whose rows come from their interface.
- `post(item)`: checks that need solved rows (effect boundaries, inferred
  purity, replayable effects, secret grants, region caps, callback bounds,
  discarded results).
- `iface(package)`: the projection that produces the interface record.

Each node stores its input fingerprint, its output fingerprint, and its
ordered reads. On the next check a node is green when its own input is
unchanged and try-mark-green succeeds on its reads. A re-executed node whose
output fingerprint is unchanged keeps its readers green. `solve(scc)` reruns
only for strongly connected components whose member summaries changed, and
it propagates only changed rows. Item spans are stored relative to the item
start, so inserting a line above an item does not invalidate it; the
renderer rebases spans.

Within a package, files share one namespace (D-MOD-CYCLE1=A). Declaration
reads therefore go to item names, not files, which makes Swift-style file
depends sets unnecessary.

### 3.6 Program-level phase

Some facts flow **down** the package graph or need the whole program, so no
single package can own them. They run once per program key, over package
records only, with no bodies:

- Output resolution, the `run` entry and CLI shape (E1308), the entry and
  module name clash (E0105), and removal of build-only entries.
- `program_effects` for the selected entry, application authority, effect
  budgets (D-EFFBUDGET1) checked at each dependency edge against the
  dependency's package effect union, and dedupe of duplicate root panics.
- Web partition. Promotion runs in both directions: a JS caller pulls an
  unmarked Wasm callee into JS (`WebPartition.rs:594-644`). The final
  buckets and cross-partition diagnostics are computed from each package's
  web facts and call edges.
- The app graph (D-WEBAPP1), job graph collisions and cycles, and unfed
  devtools state fields.
- Unreachable exports (D-STRUCT-LIVE1), from the union of all usages.
- The `used_core` union with `expand_core_reachable_closure`, the maximum
  inferred runtime layer, and the FFI callback union.
- Diagnostic union and ordering: Core lints filtered, then
  `order_diagnostics_root_first`.

The program record stores these diagnostics and projections. A no-change
run reads the program record, the package records' diagnostic pointers, and
nothing else.

### 3.7 The on-disk store

Record payloads live in the shared content-addressed cache, the `jet-store`
record and artifact store under `~/.cache` (owner ruling, 2026-09-30). That
store provides the mechanics: content-addressed blobs, action records, atomic
publication, digest verification, leases, and capacity. Records are keyed by
content, so identical records produced by two runs, two programs that share a
dependency, or two workspaces are one entry, the reason Go uses one cache for
all builds. Per-workspace state lives in one `.jet/` folder at the workspace
root, never in per-package folders.

| Record kind | Address | Payload |
|---|---|---|
| `jet.pkg-check/v1` | action record under the package check key | interface digest; handles to the interface, diagnostics, item-graph, and body objects; discovered inputs; usages |
| `jet.iface/v1` | blob addressed by content | section 3.2 |
| `jet.diags/v1` | blob | typed diagnostics with item-relative spans, origin revisions, and symbolic cross-package locations (the Receipt's codec generalized) |
| `jet.items/v1` | blob | item graph: nodes, input and output fingerprints, ordered reads, per-item summaries and diagnostics ranges |
| `jet.body/v1` | blob, build lenses only | checked bodies lowered to TIR/MIR for this package |
| `jet.program/v1` | action record under the program key | program-level diagnostics and projections; the package check keys it used |
| `jet.pkg-object/v1` | action record keyed by (package body digest, dependency interface digests, lens, profile, target) | per-package compiled output (section 3.9) |

The workspace `.jet/` holds the lock, a small **priors** pointer (the latest
program record per root and mode, which also serves as "previous record" in
step 3 of section 3.4), the stamp table, the records and receipts index,
reports, build outputs, and logs. Deleting `.jet/` or the cache costs speed,
never correctness.

**Encoding.** I6 rules out serde. Records use one std-only binary codec in
`jet-foundation`, shaped like Go's Unified IR:

- a magic number, schema name, and version, followed by a string table;
- length-prefixed sections with element indexes, so a reader decodes only
  the items it needs;
- canonical ordering throughout;
- a trailing SHA-256.

A test-only decoder renders any record as JSON; a user-facing inspect view
is a new command and waits for an owner ruling (section 8). The Receipt
payload (`jet.receipt/v1`, JSON today) moves to the same codec in the same
cutover, so the compiler keeps one record format. JetFoundation implements
the same byte format in Jet, and a conformance test encodes one set of
records with both implementations and compares the bytes.

Integrity and concurrency follow the store: atomic publication, digest
verification on read, leases, and quarantine when a key recomputes to a
different result. A corrupt or undecodable record is a miss with one `note:`
line, never an error.

### 3.8 Diagnostic replay

A green package's diagnostics come from its `jet.diags` record. They are
rendered for the current terminal (color, width, `--json`), and the program
phase merges and orders them exactly as a cold check would. The invariant
from D-BUILD-NOCHANGE1 and I4 holds: warm output, text and `--json`, is
byte-identical to `--no-cache` output. `--verify` rechecks every package and
compares the records, reporting any divergence as a compiler defect.

### 3.9 Build: per-package compiled output

`check` stops at the program record. `build`, `run`, `test`, and `eval` also
need code for every package. A package's `jet.pkg-object` holds its lowered
TIR and MIR, and later the unit crates or objects of #2519. It is keyed by
the package's body digest and its dependencies' interface digests.

- Generic templates are instantiated at the use site (D-LIB-REUSE1). The
  dependent's record lists its **instantiation demands** (template identity
  and type arguments), which #2520 phase 2 uses to place and deduplicate
  instances.
- Link keys use object content digests, as in Go.
- The JIT and the interpreter load per-package MIR, so `jet run` reuses
  green packages the same way `jet check` does.

The Rust side does not need a lossless checked-AST format because what is
stored is the lowered form. The self-hosted host already has a MIR codec in
`Compiler/Bootstrap/Host/RuntimeMirCodec.rs`.

## 4. What changes in the Rust checker

The Rust checker is the bootstrap checker, and today it checks the
self-hosted compiler (`Compiler/Bootstrap/check.sh`). It therefore needs
package granularity for `jet check` first. Build lenses and item-level reuse
come after measurement.

**Structural changes.**

- **R1. Package partition.** Each `LoadedModule` carries its package
  identity, and the bundle carries the package DAG. Core modules become
  dependency packages whose records are keyed by compiler identity, so they
  are checked once per compiler.
- **R2. Dependencies as declarations.** A green dependency's modules enter
  registration with bodies erased and templates kept. Summaries and
  constants are seeded from its record. Registration diagnostics of green
  packages are dropped; they replay from the record.
  - In the first form (stage 2), the declarations come from re-parsing the
    dependency's sources and erasing bodies, as TypeScript does for source
    project references. Only summaries, diagnostics, and usages need the
    codec.
  - Decoding declarations from `jet.iface` replaces the re-parse only if
    measurement shows that parsing and registering dependencies dominates.
- **R3. Summaries in and out.** Each completion phase below runs over the
  red packages' own modules. It reads dependency facts from records and
  writes the package's facts to its record.
- **R4. Stable keys.** Effect, taint, memory, and failure maps are keyed by
  semantic identity (R8 in the table below).
- **R5. Root-free keys.** Root policy (`application_authority`, budgets) is
  removed from the per-package environment and checked at the program level.
- **R6. Build lenses** use `jet.pkg-object` (stage 4). Until then, `build`,
  `run`, `test`, and `eval` check every package in process and rely on the
  Receipt for no-change runs.

**Per-phase changes** (all in `crates/jet-sema/src/Sema/Bundle/Pipeline/`
unless noted):

| Phase (today) | Package form | Summary crossing the edge |
|---|---|---|
| Registration, imports, unqualified imports, re-exports, imported traits (`CheckInner.rs:232-2176`) | red packages register sources; dependencies register erased declarations | declarations, re-exports, trait and impl table, derives, marker, fact, and effect declarations, state graphs, unit families |
| Strong `Shared` cycles (`Completion.rs:28`) | per package | strongly held type sets with type-parameter holes |
| Impl targets and orphan check (`:30-45`) | per package; coherence checked in the downstream package, the only one that sees both sides | impl table |
| Output resolution, run and CLI entry, E0105, build-entry strip (`:49-56,127-227,748-750`) | program level (root) | output declarations, CLI derive facts |
| Const static classification (`:58-125`) | per package (already local to the module) | none |
| Declared effect facts (`:231-233`) | per package | effect declarations |
| Job collisions and graph (`:247-250`) | program level | job table |
| Body checking (`:263-329`, `Validation.rs:509-641`) | red packages only; comptime calls into a dependency use its template, and the call is recorded as a usage of the template fingerprint | outputs: local summaries, discovered inputs, name references |
| Lambda summaries, trait dispatch seeds, `via` (`:314-320,381-391`) | per package | trait dispatch bounds; parametric `via` summaries |
| #3708 failure inference (`:330-337`; `Sema/mod.rs:250-340`) | greatest fixpoint per package (per module today; D-MOD-CYCLE1 widens it); a dependency callee's set comes from its interface, so E2404 against it settles at call-check time | failure sets |
| Address-taken exclusion from inference (`mod.rs:271`) and E0918 (`:358-380`) | package-local; a dependent that takes a narrowed function as a value gets a compiler-generated adapter to the declared callable type, and E0918 is reported at the dependent's take site | none; the take is local |
| Devtools publications and unfed fields (`:338-356`) | publications per package; unfed-field check at the program level | publications |
| Taint return facts and qualified reachability (`:392-440`) | solve over the package's nodes with dependency nodes sealed | solved rows per exported callable |
| Entry `required_effects`, output effects (`:420-474`) | program level | solved rows |
| Service handlers, autodiff purity, effect boundaries, inferred purity, replayable, secret grants, region caps, callback bounds, discarded results (`:428-585`) | per package, after the local solve | callee rows, "no outcome" flag, callback bounds |
| Pending diagnostic settlement (`:586-608`) | per package | none |
| Memory facts (`:615-635`) | declarations checked per package | per-callable memory projections |
| Web partition (`:636-640`) | program level | natural bucket, marker, ceiling, call edges |
| App graph (`:643-649`) | program level (root) | route and handler facts |
| Liveness (`:653-657`, `Bundle/Liveness.rs:41-78`) | unused imports and private functions per package; unreachable exports at the program level | usages |
| OS target (`:661`) | per package | OS gates |
| Fact tags and states (`:666-667`) | per package | taint rows, state graphs |
| `used_core`, forced inserts, closure, UI capabilities (`:669-730`) | per package set; closure at the program level | `used_core`, usage spans stay local |
| Scoped GC promotions (`:733`) | per package | none |
| Helper layer inference (`:734`) | per package against its own ceiling; program maximum | inferred layer |
| Name ledger publication (`:735`) | per-package ledger in the record; merged for tooling (`jet-semindex`) | references and definitions |
| Duplicate root panic, Core lint filter, ordering (`:757-773`) | program level | none |

`IncrementalSemaCache` stays as the in-process item cache for editor
sessions. Its `CachedFunctionBody` fields are exactly a `body(item)` node's
outputs plus the checked `Func`. Persisting them for the Rust checker (stage
3R) is optional: completion walks bodies inside a red package, so item reuse
there also requires every phase to read per-item summaries. That work is
done only if stage 2 measurement shows large single packages, such as
JetSema at about 57k lines, still miss the loop target.

## 5. The self-hosted JetDriver and JetSema

The self-hosted compiler implements the same model natively, and it fits
better. JetSema returns values: `SemaGraphModuleResult`, and `TFunc` with
`effects`. Item checks are therefore memoizable pure functions, as in Skip.

- **Identity (`JetDriver/…/Identity.jet`).**
  `jet_driver_package_check_key(package, dependency_interface_digests,
  host_facts)` sits beside `jet_driver_receipt_closure` and uses the same
  field framing. Package source digests come from the authorized snapshot.
  The loader already has `JetDriverPackageInfo.dependencies`.
- **Host store capability.** A typed host call (`store_get(kind, key)`,
  `store_put(kind, bytes)`, `stamp(path)`) through the existing
  `Bootstrap/Host` adapter. It is the sealed reader for the self-hosted
  compiler, and there is no raw file access from Jet.
- **Codec (`JetFoundation`).** A Jet encoder and decoder for the one binary
  record format, plus the conformance test against the Rust codec. This
  replaces the Rust `DiagnosticCodec` and `EntryCodec` for records.
- **Sema against interfaces.**
  - `sema_registration_prepare_graph` accepts dependency modules rebuilt
    from `jet.iface`. Their `program` holds the erased declarations and
    templates, so the walks over `program.items` in taint, failure law, unit
    families, reflection layout, and state graphs keep working. These walks
    are in `Taint.jet:336-387`, `FailureLaw.jet:224-336`,
    `Expressions/Literals.jet:16-55`, `Comptime/Reflection.jet:558-562`, and
    `SemanticIndex.jet:467-508`.
  - Walks that read dependency **bodies** are restricted to the red
    package: `sema_taint_check_body_graph` (`Taint.jet:1428-1432`) and core
    usage (`CoreUsage.jet:378-380,1188-1189`).
  - `sema_finalize_registration_graph` checks only modules whose
    `package_identity` is red (`CheckProgram.jet:2268-2275`).
  - `sema_effects_check_modules` (`Effects/Checks.jet:1109-1166`) receives
    dependency functions as sealed `SemaEffectNode`s with fixed rows, so
    `sema_effect_solve` iterates only over the package's own nodes.
- **Item memo.** `sema_check_function(decl_fingerprint, reads)` is the unit
  of reuse. Its result, a `TFunc` with facts, diagnostics, call facts, and
  name references, is stored in `jet.items` and reloaded lazily. It takes
  the Skip shape: pure, keyed by stable name, results frozen, reads
  recorded.
- **Interface projection.** A new `sema_package_interface(graph, package)`
  and a digest function build the record of section 3.2 from
  `SemaGraphResult`.
- **Pipeline (`Pipeline.jet`, `CompilerQueries.jet`).** `jet_driver_compile`
  and `jet_driver_compiler_check` become a loop over the package DAG that
  runs section 3.4 per package, then the program phase. Comptime
  orchestration (`jet_driver_run_comptime`,
  `jet_driver_orchestrate_comptime_bodies`) runs per package, and
  dependency constants come from records.

## 6. Measurement gates on the `Compiler/` package graph

**Prerequisite.** #3862 has landed `package.jet` for the nine packages:
JetFoundation (about 40k lines), JetLexer (2.6k), JetParser (8.8k),
JetOptimizer (6.2k), JetSema (57k), JetCodegen (30k), JetEval (20k),
JetDriver (6k), and Bootstrap. The package graph is acyclic
(`inventory.json`, `packageSccs`).

**Method.**

- Release compiler on the pinned machine, following the #666 method: fixed
  warmups, 20 samples, and a record of compiler and Core identities, peak
  RSS, and stable counters.
- Counters are the regression gates: packages checked and reused, items
  checked and reused, records decoded, and bytes read. Wall times are
  gated relative to the same run's baselines, never against invented
  absolutes.
- Each scenario's diagnostics, text and `--json`, must be byte-identical to
  a `--no-cache` run of the same tree.

| Scenario | Action | Gate |
|---|---|---|
| **Cold** | empty store, `jet check` of the Bootstrap entry package | package-mode wall ≤ today's single-unit check of the same sources (the split must not regress cold; superlinear per-unit cost suggests it improves); all nine packages checked |
| **Cold with Core warm** | store holds only Core records | Core modules report 0 checks |
| **No-change** | second identical run | 0 package checks, 0 item checks, only the program record and diagnostic records decoded; wall ≤ 50 ms (the #2517 bar) |
| **Comment-only edit** | edit a comment in one JetSema file | 1 package checked, 0 items rechecked (stage 3 and later), interface digest unchanged, 0 dependents checked |
| **Body edit** | change a private helper body in JetSema without changing any exported fact | 1 package checked; JetCodegen, JetEval, JetDriver, and Bootstrap reused; wall ≤ JetSema's own cold check + 10% (stage 2), and later ≤ 10% of it (stage 3) |
| **Inferred-fact edit** | make a `pub` JetFoundation function gain an effect | JetFoundation checked; exactly the dependents whose usages name that function checked; others reused by usage |
| **Interface edit, unused** | add a new `pub` function to JetFoundation | JetFoundation checked, 0 dependents checked |
| **Interface edit, used** | change a `pub` signature used by JetParser only | JetFoundation and JetParser checked, then early cutoff if JetParser's interface is unchanged |
| **Verify** | `--verify` after each scenario | every record identical |
| **Randomized replay** | N seeded edits replayed warm against clean (Hack's saved-state verifier idea) | identical diagnostics and records |

These scenarios become rows of the checked compiler-speed corpus beside the
existing `jit-*` and `aot-release-*` rows. The build-time cells of
D-BUILDBENCH1 reuse them.

## 7. Stages

Days and weeks refer to one focused implementer. Stages 1 and 5a can run in
parallel; stage 2 needs #3862.

| Stage | Scope | Size |
|---|---|---|
| **S1 Identity and records** | Package partition and DAG in the Rust bundle (R1); stable semantic keys for summaries (R4); fingerprint functions (source, interface, item); the std-only binary record codec in `jet-foundation`; `jet-store` record kinds in the shared cache under `~/.cache`; priors, stamps, and the records index in the workspace-root `.jet/` (workspace root resolved first; never per-package folders); `explain-build` rows per package with the reason (green by key, green by usage, red, cutoff); counters. Proof: two runs and two checkout paths give equal digests; a comment or body edit leaves the interface digest unchanged; a signature edit changes it. | **days** (4–6) |
| **S1b Sealed reader** | One audited reader for check-time file and environment reads in sema, comptime, and the loader, with the injected-read test (#2517 criteria 2/6). Needed before any key is trusted beyond the Receipt's refusals. | **days** (3–5) |
| **S2 Rust package checks (`jet check`)** | R2 in its re-parse form, R3, R5, the per-phase table in section 4, the program phase, per-package diagnostic records and replay, usage cutoff, and Core as dependency packages. Check mode and the bootstrap check. | **weeks** (2–3) |
| **S2g Gates** | The section 6 scenarios on `Compiler/` as corpus rows and focused tests; `--verify`; randomized replay. | **days** (2–4), after S2 |
| **S3R Rust item reuse (optional)** | Persist `body(item)` outputs and make completion phases read per-item summaries inside a red package. Only if S2g shows JetSema-sized packages miss the loop target. | **weeks** (2) |
| **S4 Build lenses** | `jet.pkg-object` per package (TIR/MIR, then #2519 units); instantiation demands; `run`, `test`, and `eval` reuse green packages; the Receipt remains only for whole-invocation replay. Joins #2519 and #2520. | **weeks** (3–5) |
| **S5a Self-hosted identity and codec** | `Identity.jet` package keys, the JetFoundation codec with conformance bytes, and the host store call. | **days** (4–6) |
| **S5b Self-hosted package checks** | Sealed effect nodes, interface projection, package-restricted finalize and body walks, the `Pipeline.jet` package loop, and the program phase in JetSema. | **weeks** (2–3) |
| **S5c Self-hosted item memo** | `sema_check_function` memo with recorded reads, SCC-local re-solve, and lazy loads. | **weeks** (1–2) |

Order for the bootstrap loop: S1 → S1b → S2 → S2g gives the dogfood win in
the checker that runs today. S5a starts alongside S1 once the codec layout
is frozen. S3R is decided by S2g's numbers. S4 follows S2.

## 8. Owner questions this design surfaces

Owner chat answers count as rulings; none of these needs a ballot.

1. **Resolved (owner, 2026-09-30).** The shared content-addressed cache stays
   under `~/.cache`; all per-workspace state lives in one workspace-root
   `.jet/`, never in per-package folders. Section 3.7 follows this.
2. **Diagnostic location changes.**
   - E0918 moves to the take site when an `#Inline(Always)` function of
     another package is taken as a value.
   - A dependent that takes a function narrowed by #3708 as a value gets an
     adapter instead of blocking inference in the owning package.
   - Both are observable, keep one meaning on every tier, and need snapshot
     updates. Neither adds syntax, so they are likely implementation choices
     under D-FAILURE-FOUNDATION1 and I4. Confirm.
3. **Interface-first scheduling.** Skipping bodies to publish interfaces
   early is sound only for items with written contracts. Jet should not add
   a TypeScript-style "isolated declarations" requirement without an owner
   ruling.
   The design uses the pipelining only where contracts are already written.
4. **ReceiptStore callers** (`prove`, `status`, `package`, and others) remain
   an open #2517 question and are unchanged by this design.
5. **A user-facing record view.** Showing interface and check records as
   JSON (for example under `jet inspect`) is a new command surface. It is not
   needed for any stage; `explain-build` already reports per-package reuse.

## Sources

- rustc dev guide:
  [Incremental compilation](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html);
  [Incremental compilation in detail](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html);
  [rust#47389](https://github.com/rust-lang/rust/issues/47389).
- Go:
  [`cmd/go/internal/work/buildid.go`](https://github.com/golang/go/blob/master/src/cmd/go/internal/work/buildid.go);
  [`cmd/go/internal/work/exec.go`](https://github.com/golang/go/blob/master/src/cmd/go/internal/work/exec.go)
  (`buildActionID`, `import %s %s` with `buildExportID`);
  [`cmd/compile/internal/noder/unified.go`](https://github.com/golang/go/blob/master/src/cmd/compile/internal/noder/unified.go);
  [`cmd/compile/internal/noder/doc.go`](https://github.com/golang/go/blob/master/src/cmd/compile/internal/noder/doc.go);
  [`golang.org/x/tools/go/analysis` doc](https://github.com/golang/tools/blob/master/go/analysis/doc.go).
- GHC:
  [Recompilation avoidance](https://gitlab.haskell.org/ghc/ghc/-/wikis/commentary/compiler/recompilation-avoidance).
- Swift:
  [DependencyAnalysis.md](https://github.com/swiftlang/swift/blob/main/docs/DependencyAnalysis.md);
  [Driver.md](https://github.com/swiftlang/swift/blob/main/docs/Driver.md);
  [`ModuleDependencyGraph.swift`](https://github.com/swiftlang/swift-driver/blob/main/Sources/SwiftDriver/IncrementalCompilation/ModuleDependencyGraph.swift);
  [`EmitModuleJob.swift`](https://github.com/swiftlang/swift-driver/blob/main/Sources/SwiftDriver/Jobs/EmitModuleJob.swift).
- TypeScript:
  [`builderState.ts`](https://github.com/microsoft/TypeScript/blob/v5.9.3/src/compiler/builderState.ts);
  [`builder.ts`](https://github.com/microsoft/TypeScript/blob/v5.9.3/src/compiler/builder.ts);
  [Project References](https://www.typescriptlang.org/docs/handbook/project-references.html);
  [TypeScript 5.5 isolated declarations](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-5-5.html#isolated-declarations).
- Skip:
  [skiplang/skip README](https://github.com/skiplang/skip);
  [`src/frontend/skipTyping.sk`, `skipNaming.sk`, `skipExpand.sk`, `src/server/skipServer.sk`](https://github.com/skiplang/skip/tree/master/src);
  [How memoization works](https://github.com/skiplang/skip/blob/master/docs/blog/2017-01-04-how-memoization-works.md).
  Hack via `Docs/research/mine-for-jet-2026-09-30-roc-skip.md`.
- Jet: the files cited inline, `Docs/proposals/automatic-build-optimization.md`,
  `Docs/research/check-scaling-2026-09-30.md`, card #2517, #3862's
  `inventory.json`, and PersistCheckCache's report.
