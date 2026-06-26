# Jet Idea Master List - Sorted By Implementation Ease

**Source:** `P0-Extracted-Ideas.md`
**Sorted:** 2026-06-26
**Sort rule:** Rough low-hanging-fruit order by remaining unresolved implementation effort, not by original category.
**Preservation rule:** The sorted levels below keep one bullet for each current source bullet. They do not intentionally merge, split, reject, or remove any current source item.
**Lossiness caution:** The source list is already deduped. Any item marked `Bundle-risk` contains separable subfeatures, partial status, or wording that could hide original intent; recover or split the original notes before implementation/closure.

## Status Terms

`Ratified` means the design or policy has been owner-approved, but it may still need implementation work.

`Implemented` means code, shipped behavior, or project documentation already exists. It does not necessarily mean there is a separate ratification record unless one is cited.

`Ratified/Implemented` means both concepts are involved: either the design was approved and then built, or the bullet bundles some ratified parts with some implemented parts. Treat mixed statuses as "verify the remaining subparts before closing."

## New Scratch Notes

- Verse style concurrency/async task types (race, wait all, etc.) [Verse Concurrency](https://verselang.github.io/book/14_concurrency/)
- Fix inferred vs explicit const/var binding syntax -> CONSTVAL @ String = "string" or CONSTVAL @= "string" && VARIABLE: int = 3 or VARIABLE := 3
- Allow ignore multiple return values -> Jai can ignore errors: `content := read_entire_file(...);` works, even though the function returns an error. could accept the error value as `content, ok := read_entire_file(...);`
- Relook if switch statements -> 07_switch still uses || to check multiple patterns on the input var -> should be | with || for additional expressions alongside the pattern
- Broad gated build-time I/O: allow comptime code to read env vars, hit the network, run a subprocess, or codegen at build time (Jai's #run / Zig @embedFile-plus territory), behind a sandbox + an auditable .jet/build-io.lock of every accessed path + cache-invalidation on change. Powerful (full build scripting without a separate build step), but it adds a supply-chain attack surface that the S26 "no ambient I/O at comptime" law was written to refuse — the Nim/Jai evidence shows un-auditable spread once it ships.
- Following constructor syntax: fmt: Fmt = .{
        .gpa = gpa,
        .arena = arena,
        .io = io,
        .seen = .init(gpa),
        .any_error = false,
        .check_ast = check_ast_flag,
        .force_zon = force_zon,
        .color = color,
        .out_buffer = .init(gpa),
        .stdout_writer = &stdout_writer,
    };

- **First-class unknown/loading/pending/never:** Model loading/pending/never explicitly. Status: `Already expressible` with enums/options; no special feature needed.
- **Switch/multi-pattern cleanup:** Use single `|` for structural pattern alternatives and reserve `||` for boolean logic. Older `switch`/`when` examples are stale after D-IF revisions.
- **Transactional blocks:** Roll back mutations on failure. Status: `Implemented/Ratified` D-TXN1; rollback trait shape ratified as D-ROLLBACK-TRAIT.
- **Blessed protocol hooks:** Core syntax delegates to fixed hooks like iterator/index/literal suffix. Status: `Ratified/Implemented in pieces`; D-EXT1 formalizes the policy. Bundle-risk: individual hooks can have different implementation status.
- **Safety ladder documentation:** Beginner/working/expert tiers as an explicit model. Status: `Implemented/philosophy`; Tower c120 audits separation.
- **Full lazy iterator adapter set:** Rich map/filter/fold/window/chunk/group_by/etc. Status: `Ratified/Implemented` D-ITER1; verify individual gaps only if needed. Bundle-risk: adapter-by-adapter coverage may differ.
- **Observability in the box:** Structured logs, tracing, metrics. Status: `Implemented` E2-M12/D-OBS3; OTel exporter remains package-level future. Bundle-risk: exporter work should not be closed by core observability alone.
- <a id="idea-component-wise-vector-arithmetic"></a>**Component-wise vector arithmetic:** Vector operators apply lane-wise by default. Status: `Implemented` for closed SIMD lane types; broader user vector overloading is not open. Bundle-risk: closed SIMD behavior and user-defined overloading are different decisions.
- <a id="idea-arenas-temp-storage"></a>**Arenas/temp storage:** First-class arena and bump allocation patterns. Status: `Implemented/Ratified` core arena regions; compiler inference remains Tower c26. Bundle-risk: allocator API and compiler inference are separable.
- <a id="idea-implicit-swappable-allocator-context"></a>**Implicit swappable allocator context:** Jai-style ambient allocator. Status: `Implemented/Ratified` as `#Context(allocator: ...)` plus explicit allocator APIs; broad policy still evolving. Bundle-risk: shipped mechanism and policy completion differ.
- **Single-use / linear values:** Values that must be consumed exactly once. Status: `Implemented` D-LIN1 `#SingleUse`; explicit audited drop hatch remains unspecified. Bundle-risk: core feature and drop hatch are separable.
- **Typestate:** Order-sensitive APIs, wrong-state calls are compile errors. Status: `Implemented` D-STATE1; `state {}` declaration follow-up is Tower c163. Bundle-risk: typestate semantics and declaration sugar are separable.
- 
## Scratch Traceability

- `Coroutines/Async/Await` -> [Coroutines as primitives](#idea-coroutines-as-primitives); also related to [Coroutines / async-await / Go-scale networking](#idea-coroutines-async-await-go-scale-networking).
- `Selective / aliased imports "use a::b::{X, Y as Z}"` -> [Selective imports with aliases](#idea-selective-imports-with-aliases).
- `Modules that support generics` -> [Generic modules](#idea-generic-modules).
- `Jai's implicit swappable allocator ("context")` -> [Implicit swappable allocator context](#idea-implicit-swappable-allocator-context).
- `First-class arena/temp-storage patterns` -> [Arenas/temp storage](#idea-arenas-temp-storage).
- `Add manual scoped cleanup (defer)` -> [Manual scoped cleanup / defer](#idea-manual-scoped-cleanup-defer).
- `Built in vectors & swizzling` -> [Built-in vectors/swizzling](#idea-built-in-vectors-swizzling).
- `Allow ignore multiple return values` -> [Ignoring multiple return values / ignored errors](#idea-ignoring-multiple-return-values-ignored-errors).
- `Consider "def" keyword for alias - or "alias"` -> [Type alias keyword (`def` / `alias`)](#idea-type-alias-keyword-def-alias).
- `Full STDLIB/CoreLIB available in REPL` -> [Full CoreLib in the REPL](#idea-full-corelib-in-the-repl). Note: the current body narrowed this to CoreLib execution.
- `Support variadics & spreading` -> [Variadics and spreading](#idea-variadics-and-spreading).
- `Opt in library that offers a garbage collector` -> [Opt-in GC library](#idea-opt-in-gc-library).
- `Consider #builtin marker to allow non-use of module prefix on calls` -> [Prelude / no-prefix common names](#idea-prelude-no-prefix-common-names).
- `Public by default OR #scope_file` -> [Public-by-default or `#scope_file` visibility](#idea-public-by-default-or-scope-file-visibility).
- `Component-wise arithmetic by default` -> [Component-wise vector arithmetic](#idea-component-wise-vector-arithmetic).
- `fn TYPE.method ()` -> [External inherent methods / `extend` blocks](#idea-external-inherent-methods-extend-blocks).
- `$ as indicator for macros or comptime?` -> [`$` for comptime or macros](#idea-dollar-for-comptime-or-macros).
- `Analyze vs Typescript` -> [TypeScript/Swift competitive analysis](#idea-typescript-swift-competitive-analysis).
- `Use Jai-style build system` -> [Jai/Zig-style build system](#idea-jai-zig-style-build-system).
- `Cross platform native raylib builtin` -> [Cross-platform raylib builtin](#idea-cross-platform-raylib-builtin).
- `Consider how to improve using concepts from Zig build system` -> [Jai/Zig-style build system](#idea-jai-zig-style-build-system).
- `Cleanup old std.mem syntax (Ptr<T>)` -> Trace gap: [low-level pointer syntax cleanup](#trace-gap-low-level-pointer-syntax-cleanup).
- `Relook what labeled ref field in struct` -> [Labeled `ref` fields / original ownership model relook](#idea-labeled-ref-fields-original-ownership-model-relook).
- `Relook original ownership model - outdated example` -> [Labeled `ref` fields / original ownership model relook](#idea-labeled-ref-fields-original-ownership-model-relook).

## Level 1 - Documentation, Review Checklists, Formatter, And Tiny Diagnostics

- <a id="idea-typescript-swift-competitive-analysis"></a>**TypeScript/Swift competitive analysis:** Keep the "replace TS/Swift" feature lens explicit. Status: `Research note`; concrete ideas are protocol, UI, interop, web/app backends.
- **Stdlib ergonomic laws/checklist:** Written API review rubric for safe, obvious APIs. Status: `New/doc`; principles are mostly practiced but not a standalone checklist.
- <a id="idea-formatter-preserves-grouping-parentheses"></a>**Formatter preserves grouping parentheses:** `jet fmt` should not erase author-written parentheses used for clarity. Status: `New` if it still happens; no current Tower card found.
- **Formatter preserves single-line bodies:** Keep compact one-line bodies when author wrote them that way. Status: `Ratified` D-FMT1.
- **Assignment in conditions guard:** Ban or teach `if x = 5`. Status: `Likely already grammar-rejected/New`; no dedicated card found.
- **Float equality / semantic smell lints:** Warn on plausible bugs like float `==`, constant conditions, duplicate branches. Status: `New`; no Tower card found. Bundle-risk: multiple lint families are grouped.
- **Confusable-name lint:** Warn on `users` vs `user`, `l` vs `1` in same scope. Status: `New`; did-you-mean exists, same-scope confusables do not appear carded.
- **Copy-paste drift / structural-dup lint:** Detect 3-of-4 copied blocks updated. Status: `New/tooling`; no Tower card found.
- **Display/Debug split:** Separate user display from debug representation. Status: `New/minor`; formatting/interpolation exist.
- **Random split:** Fast seedable PRNG vs crypto RNG. Status: `Partial/Implemented`; `core.random` and `jet.crypto` exist, API clarity may need review. Bundle-risk: implementation may exist while naming/API policy remains.
- **Time library depth:** Dates, zones, calendar math, injectable clock. Status: `Implemented/Partial`; deterministic `Clock` exists, richer `jet.time` should be verified against expectations. Bundle-risk: several time APIs are grouped.
- **Tiny formal core / desugaring map:** Document the kernel every surface feature lowers to. Status: `New/process`; no formal kernel doc found.

## Level 2 - Small Standard Library, Package, And API Additions

- **Collections breadth:** Add `Set<T>` and `Deque<T>`/ring buffer. Status: `New`; no card found. Bundle-risk: `Set`, `Deque`, and ring buffer can ship independently.
- **UUID and encoding utilities:** UUID v4/v7, base64, hex. Status: `New/std`; base64 appears only in FFI examples, not as Core API. Bundle-risk: UUID and encodings are independent utilities.
- **Decimal money type:** Exact base-10 type plus lint against float money. Status: `New`; no `Decimal`/money lint card found. Bundle-risk: numeric type and lint are separate deliverables.
- **Path objects and atomic write/dir-walk:** Safer filesystem API than raw strings. Status: `Partial`; path/list_dir work exists, full Path/atomic-write surface may need verification. Bundle-risk: path type, atomic write, and traversal are separate.
- **TTL / expiring values and rotting secrets:** Values/cache entries expire automatically or become unusable. Status: `New/library`; no Tower card found. Bundle-risk: cache TTL and secret invalidation have different safety requirements.
- <a id="idea-cross-platform-raylib-builtin"></a>**Cross-platform raylib builtin:** First-party/native graphics/raylib package. Status: `New`; examples exist, built-in package not found.
- **Ownable component kit:** Copy-in-and-own UI components rather than locked theme/framework. Status: `New`; no UI card found.
- <a id="idea-manual-scoped-cleanup-defer"></a>**Manual scoped cleanup / defer:** Run cleanup at scope exit. Status: `Implemented` as `core.scope.guard`; `defer` keyword remains a reopen. Bundle-risk: library guard and language keyword are separable.
- <a id="idea-prelude-no-prefix-common-names"></a>**Prelude / no-prefix common names:** Curated auto-imported common set. Status: `Implemented` for `print`/`input`; user opt-out and `#builtin`-style library escape are `New/Reopen`. Bundle-risk: existing prelude, opt-out, and builtin escape are separate.
- **Misuse-resistant crypto API:** High-level `seal`/`sign` envelope hiding nonce footguns. Status: `New`; crypto primitives exist, envelope API not found.
- **Arbitrary-precision integers:** BigInt type. Status: `New/std`; no built-in BigInt found.
- **Text grapheme iteration/normalization:** Human-visible Unicode clusters and normalization. Status: `New/std`; strings are Unicode-aware but grapheme API was not found. Bundle-risk: grapheme iteration and normalization can ship separately.
- **JSON/CSV/TOML/YAML/archive modules:** First-party data format ring. Status: `Implemented/Planned`; YAML core implemented, full YAML tags are Tower c153. Bundle-risk: individual formats and YAML tags should be tracked separately.
- **Compression formats:** zip/tar/gzip/zstd/brotli-style support. Status: `Partial`; `jet.archive` planned/built-from-source track c50, broader codecs not all verified. Bundle-risk: archive formats and compression codecs are independent.
- **Honest numbers:** Numeric values track precision/error bounds. Status: `New/library`; overlaps uncertainty tags but not carded.
- <a id="idea-opt-in-gc-library"></a>**Opt-in GC library:** Library-provided garbage collector for long-running processes. Status: `New`; no card found.
- **`jet.linalg` / math package:** Vectors, matrices, decompositions, FFT later. Status: `Ratified/partly implemented` D-MATHLIB1/D-SIMD1/2. Bundle-risk: vectors, matrices, decompositions, and FFT are distinct.
- **Post-quantum crypto agility:** Hybrid X25519+ML-KEM behind safe API. Status: `New/far-horizon`; sequence after safe crypto envelope. Bundle-risk: algorithm choice should follow envelope API.

## Level 3 - Focused Syntax, Typechecker, Compiler, And Build Features

- <a id="idea-selective-imports-with-aliases"></a>**Selective imports with aliases:** Bring specific items into scope, optionally grouped. `use math.item` / `use math.{a, b as c}` Bundle-risk: single-item imports, grouped imports, and aliases can be staged.
- **Glob imports:** `use module.*` / `use a::*` to import everything. Status: `Reopen` because D-MOD2 rejects wildcard imports with E0612.
- **`pub(package)` visibility tier:** Visible within the package, hidden outside it. Status: `New/Reopen`; S18/D-MOD3 currently have only private + `pub`.
- **In-file namespace subgrouping:** Optional grouping block inside one file without creating a separate file. Status: `Partially implemented` as inline `module name { ... }`; `namespace {}` as a separate spelling is `New/Reopen`. Bundle-risk: grouping semantics and spelling are separable.
- <a id="idea-type-alias-keyword-def-alias"></a>**Type alias keyword (`def` / `alias`):** Add a transparent alias spelling. Status: `Reopen`; S14 rejects `def` aliases and D-SUGAR3 declined transparent type aliases for now.
- <a id="idea-ignoring-multiple-return-values-ignored-errors"></a>**Ignoring multiple return values / ignored errors:** Allow discarding secondary results/errors like Jai. Status: `Reopen`; Jet uses `T ? E`, and ignored fallible results are errors/tests today. Bundle-risk: multiple returns and ignored errors are different issues.
- <a id="idea-dot-inferred-construction"></a>**Dot-inferred construction:** Use `.{ ... }` / `T.{ ... }` for inferred construction. Status: `Ratified/Tower c158`; implementation pending, and exact Zig-style `.field =` syntax would still be a reopen. Bundle-risk: inferred construction and Zig-style field syntax differ.
- **Named build profiles:** User-defined `release`/`debug`/`ci` style profiles selected by flags. Status: `Ratified/Tower c159`.
- **Maturity tags:** `#Experimental` / `#Tested` / `#Hardened` dependency lattice. Status: `New`; likely rides tag/effect machinery.
- **Must-use results:** Ignored important results are errors unless intentionally discarded. Status: `Ratified/Partial`; `#SingleUse` and fallible ignored-result diagnostics cover core cases. Bundle-risk: existing diagnostics and broader must-use semantics differ.
- **Rollback hooks / custom rollback:** User customizes snapshot/restore or registers undo hooks. Status: `Ratified/Partial` D-ROLLBACK-TRAIT; associated-types completion gates full trait dispatch. Bundle-risk: snapshot/restore traits and undo hooks are separable.
- <a id="idea-built-in-vectors-swizzling"></a>**Built-in vectors/swizzling:** Vector types with `x/y/z/w` style access. Status: `Partial`; SIMD/linalg vectors are implemented/ratified, named swizzles deferred/not found. Bundle-risk: vector type support and swizzle syntax differ.
- **Visible uninitialization:** Opt out of zero-fill with write-before-read proof. Status: `Tower c76` ready; fixed-array prerequisite is now satisfied.
- <a id="idea-full-corelib-in-the-repl"></a>**Full CoreLib in the REPL:** Make the REPL able to execute Core calls inline. Status: `Tower c133` ready plan.
- **Content-addressed build cache:** Hash normalized definitions/build inputs for incremental work. Status: `Partial implemented` via `BuildCache.rs`/`SHA256.rs`; normalization contract still important. Bundle-risk: cache implementation and normalization contract differ.
- **Generated docs from semantic graph:** Jetdoc-style generated docs from the compiler graph. Status: `Partial`; doctests/docs exist, semantic doc generator not found.
- **Dossier/outline views for scattered types:** Stitched views of a type across files and impls. Status: `New/tooling`; LSP foundations exist, no dedicated Tower card found.
- **Breadcrumb/phantom-stub editor hints:** Show scattered methods near the type body. Status: `New/tooling`; likely rides semantic index.
- **Debugger / DAP full native stepping:** Step native builds in Jet terms. Status: `Tower c144`; interpreter debugger step 1 is implemented.
- **Compiler internal seams:** Split compiler into documented lexer/parser/sema/TIR/codegen APIs. Status: `Ratified/Tower c160`; this helps semantic tools but is not a public query API by itself. Bundle-risk: each seam/API can have different readiness.
- **Monorepo workspace surface:** First-class `workspace.jet` / workspace module for package sets. Status: `Tower c156` deciding.
- <a id="idea-generic-modules"></a>**Generic modules:** Instantiate a module with a type or value parameter. Status: `New`; generic types/functions exist, but module generics were not found.
- <a id="idea-external-inherent-methods-extend-blocks"></a>**External inherent methods / `extend` blocks:** Add methods to a type from another file. Status: `Partial/Reopen`; `impl Type {}` exists, S83 `Type~~member` is ratified/pending, but a distinct `extend` keyword is not current. Bundle-risk: external method placement, member lookup, and keyword choice differ.
- <a id="idea-variadics-and-spreading"></a>**Variadics and spreading:** Function variadics and spread syntax for calls/lists. Status: `Deferred/New`; D-FP6 deferred list spread, function variadics were not found. Bundle-risk: variadic params, call spread, and list spread are separate.
- <a id="idea-dollar-for-comptime-or-macros"></a>**`$` for comptime or macros:** Reserve a sigil for generated-code splice/comptime. Status: `Ratified/Tower c162` as `$` splice plus `comptime {}`; full macro meaning is rejected for v1 by D-METADEPTH1. Bundle-risk: splice/comptime and macro semantics are intentionally different.
- <a id="idea-broad-gated-build-time-io"></a>**Broad gated build-time I/O:** Allow env/network/subprocess/codegen at comptime behind audit and reproducible locks. Status: `Ratified/Tower c157` as D-CTEFFECT1 tiers plus `#Impure`; implementation pending. Bundle-risk: env, network, subprocess, codegen, audit, and locks are separable.
- <a id="idea-jai-zig-style-build-system"></a>**Jai/Zig-style build system:** Treat builds as programmable Jet rather than external scripts. Status: `Partial/Planned` via Jetpack, D-BUILDPROFILE1, D-WORKSPACE1, D-CTEFFECT1. Bundle-risk: build language, profiles, workspaces, and comptime effects are separable.
- **Stable semantic-index query API:** Expose compiler/LSP facts as a public query API. Status: `New`; internal `Source/LSP/SymbolDB.rs` exists, but no stable external API/card found.
- **Impact/blast-radius analyzer:** Show downstream effects of a change. Status: `New/tooling`; rides semantic index.
- **Replayable/reversible codemods:** Refactors as named objects that can be shipped/undone. Status: `New/tooling`; LSP rename/fixes exist, codemod object model not found.
- **Package build-from-source:** Jetpack realizes deps from source and ships ring packages. Status: `Tower c50`. Bundle-risk: source realization and ring package shipping may stage separately.
- **Plugin target:** Build/load plugin packages safely. Status: `Tower c81` deciding.
- <a id="idea-labeled-ref-fields-original-ownership-model-relook"></a>**Labeled `ref` fields / original ownership model relook:** Revisit stored references and ownership vocabulary. Status: `Partial/Implemented`; capability sigils and `ref` hardening landed, exact old note likely stale. Bundle-risk: likely needs original-note recovery before action.

## Level 4 - Cross-Cutting Runtime, Tooling, And Type-System Work

- <a id="idea-coroutines-as-primitives"></a>**Coroutines as primitives:** Suspend/resume execution without full async function coloring. Status: `New/Reopen`; async plan uses `@async`/`await`, not generic coroutines.
- **Structured concurrency / nursery scope:** Lexical task scope cannot exit until children finish; cancellation/deadline context. Status: `New/Planned-adjacent`; tasks exist, nursery scope not found. Bundle-risk: nursery, cancellation, and deadlines can be separate.
- **Try-both-keep-the-winner / race winner:** Run alternatives and keep the successful/winning one. Status: `New/Planned-adjacent`; transaction covers rollback, not general racing. Bundle-risk: success race and winner race may have different semantics.
- **Bare `undo` keyword:** Explicit language keyword to reverse an operation. Status: `Reopen`; D-SUGAR5 declined nearby cleanup sugar and transactions use `#Transact`.
- **Self-versioning values:** Values carry version and conversion history. Status: `Partial`; schema snapshots/migrations cover published shapes, not every value. Bundle-risk: schema versions and per-value history differ.
- **General refinement types:** User-defined constraints such as positive integers. Status: `Deferred` D-REFINE1; `[T#N]` fixed-size arrays are the implemented narrow case.
- **Out-of-bounds checks with proof escape:** Bounds checked by default, prove in-range to elide. Status: `Partial`; safe indexing exists, proof-based unchecked tier not found. Bundle-risk: runtime checks and proof escape are separable.
- **Effect ceilings / `#(no_net)` prohibitions:** Prove a call graph cannot perform a forbidden effect. Status: `Deferred` D-PROP1.
- **Budgets/cost/resource tags:** Time, allocation, latency, and complexity caps as type/effect facts. Status: `Deferred` D-BUDGET1; cost-axis scratchpad remains broader than current ballot. Bundle-risk: multiple resource axes are grouped.
- **Tracked uncertainty/freshness/precision:** Values carry "estimate", "possibly stale", "±5%" dimensions. Status: `Partial/New`; Option/taint cover two axes, broader uncertainty is not carded. Bundle-risk: uncertainty, freshness, and precision are separate dimensions.
- **Failure-aware comprehensions:** List builders auto-skip failed elements. Status: `New`; gentle logic-programming slice not carded.
- **Failure-as-hole / return-a-hole:** Typed missing/hole values flow instead of crashing. Status: `Partial/Reopen`; options/results/enums cover explicit cases, no universal hole. Bundle-risk: typed holes and failure propagation need separate treatment.
- **Ask-why / value provenance:** Ask a value where it came from. Status: `New/Reopen`; not covered by current logging/tracing.
- **Time-travel variable history:** Keep variable histories for debugging. Status: `New/Reopen`; related to debugger, but no value-history feature found.
- **Latency/deadline propagation:** Deadlines flow downhill through calls. Status: `Partial/New`; Smart Context could carry this, but no deadline-specific card found. Bundle-risk: context carrier and deadline semantics differ.
- **Approximate algorithms:** Trade accuracy for speed automatically or by policy. Status: `New/library`; no card found.
- **Auto-parallelism:** Sequential-looking maps/folds run in parallel when proven safe. Status: `New/Reopen`; current design favors explicit tasks and rejects hidden machinery in D-REACT1.
- **Ask-your-codebase query engine:** Structural questions like "where can balance go negative?" Status: `New`; should ride semantic-index API.
- **Content-addressed definitions/names:** Definition identity is body hash; renames become alias moves. Status: `New/Reopen`; conflicts with file-is-program instincts, no current card found.
- **Database driver interface:** General `database/sql`-style interface, parameterized-only. Status: `Partial`; SQLite/db work exists through c50/`jet.db`, generic driver interface not found. Bundle-risk: interface, parameter policy, and drivers are separable.
- **Networking crown jewels:** HTTP client/server, routes, TLS, URL, WebSocket. Status: `Partial/Planned`; HTTP/routing/TLS exist, WebSocket/Go-scale server is Epoch 3 async plan. Bundle-risk: several independent networking surfaces are grouped.
- **Embedded/no-runtime library layering:** `core ⊂ alloc ⊂ std`, same code from server to microcontroller. Status: `Partial/Planned`; freestanding/no-std direction exists, full ring layering is not designed. Bundle-risk: library split and platform story are separate.
- **Explicit allocation at boundaries:** Caller-supplied buffers/allocators for fixed-memory use. Status: `Ratified/Partial`; allocator/arena work exists, broad std API convention not complete. Bundle-risk: allocator mechanism and std convention differ.
- **Typed styles:** CSS/style layer with compile-time property/unit checking. Status: `New`; rides units and UI track.
- **Accessibility by default:** Components ship ARIA/focus/keyboard behavior and release-gated a11y diagnostics. Status: `New`; no card found. Bundle-risk: component behavior and diagnostics gate differ.
- **Native app backend:** FFI to native widgets first, own-renderer later. Status: `New`; C FFI exists, native UI strategy not carded. Bundle-risk: widget FFI and own renderer are separate phases.
- **Web backend JS DOM + WASM:** Emit JS DOM ops for views, keep logic/compute in WASM. Status: `New/planned-adjacent`; no web backend card found. Bundle-risk: DOM emission and WASM compute partition differ.
- **JS/npm and Swift interop:** Let users call existing ecosystems from day one. Status: `New`; only C/Rust FFI and package plans exist. Bundle-risk: JS/npm and Swift are separate interop tracks.
- **JIT tier:** Add Cranelift JIT, later own bytecode/native JIT. Status: `Tower c139 ready`, c140/c141 frozen. Bundle-risk: Cranelift JIT and later custom tiers differ.
- **Arena allocator compiler inference:** Compiler chooses arena placement. Status: `Tower c26` far-horizon.
- **Signed package cache:** Signed binary/source package cache. Status: `Tower c56 frozen`; checksum/signing floor partially covered by package signing decisions. Bundle-risk: binary cache, source cache, checksums, and signing differ.
- **Publish/registry UX:** Real registry upload, semver resolver, publish flow. Status: `Tower c96`. Bundle-risk: upload, resolver, and publish UX are separable.

## Level 5 - Large Platform Strategy And Semantic Reversals

- <a id="idea-coroutines-async-await-go-scale-networking"></a>**Coroutines / async-await / Go-scale networking:** Add a high-concurrency async runtime rather than only blocking tasks. Status: `Planned/Reopen` via Epoch 3 `async-networking.md`; current shipped model teaches against `async`/`await` and uses tasks/channels. Bundle-risk: coroutines, async syntax, runtime, and Go-scale networking are separate.
- <a id="idea-public-by-default-or-scope-file-visibility"></a>**Public-by-default or `#scope_file` visibility:** Flip private-by-default or make a positional public/private split. Status: `Reopen`; S18 explicitly chose private-by-default plus per-item `pub`. Bundle-risk: default visibility and file-scope marker differ.
- **Marked DSL blocks:** Local syntax islands such as `sql!{...}`. Status: `Ratified as stdlib-only future` by D-EXT1; no general third-party DSL block found.
- **User-authored derives and reflection:** Libraries author derives using a typed reflection API. Status: `Tower c155` deciding; v1 ceiling ratified as reflection/derives only. Bundle-risk: derive authoring and reflection API differ.
- **Reflection read API:** A type exposes fields/types/markers to comptime code. Status: `Open decision` D-METAREFLECT1 under c155.
- **Derive output mechanism:** User derives emit source fragments that re-enter lexer/parser/sema. Status: `Open decision` D-METADERIVE1 under c155; D-CTCODEGEN1 already ratifies the architecture rule.
- **Full information-flow/compliance tracking:** Security-label lattice such as "EU data cannot leave EU." Status: `Deferred` D-IFC1.
- **Time-varying roles:** Roles that change over time beyond basic typestate. Status: `Deferred` D-ROLE1.
- **Record-and-replay:** Deterministically replay executions for debugging. Status: `Deferred` D-REPLAY1.
- **Living graph / core reactivity:** Values update dependents like a spreadsheet. Status: `Reopen`; D-REACT1 chooses library/tooling, not core evaluation semantics.
- **Typed client/server protocol:** Declare protocol/session once and generate both sides. Status: `Deferred` D-PROTO1; strategically important for TypeScript replacement. Bundle-risk: protocol declaration, sessions, and generation can be staged.
- **One type system across the wire:** Eliminate hand-synced TS/Zod/tRPC layers. Status: `Partial`; serde exists, protocol/session generation remains D-PROTO1.
- **Render-target abstraction:** Web/native/embedded/TUI as backend traits. Status: `New`; research says decide early, no card found. Bundle-risk: every target has different constraints.
- **Reactive UI stack:** Reactivity → view model → typed styles → headless/styled kit → motion/app kit. Status: `New/Reopen`; depends on whether core reactivity decision is reconsidered. Bundle-risk: full stack roadmap combines several layers.
- **Motion as reactive state:** Animation is derived state, not a separate runtime. Status: `New/Reopen`; downstream of reactivity call.
- **Adaptive runtime:** Adjust to battery/network/load/carbon or fidelity under load. Status: `New/library/framework`; no card found. Bundle-risk: environmental signals and adaptation policies are separate.
- **Adaptive fidelity under load:** One knob for quality/perf tradeoff. Status: `New/library/framework`.
- **Carbon/battery-aware runtime policies:** Runtime responds to environmental constraints. Status: `New/library/framework`. Bundle-risk: carbon and battery policy may need different data sources.
- **Content-addressed package/store identity:** Use hashes for dependencies, cache entries, and reproducibility. Status: `Partial implemented` in package/build cache; broader content-addressed definitions remain separate. Bundle-risk: package/store identity and definition identity differ.

## Level 6 - Far-Horizon Research Or Rejected/Reopen Macro-Scale Work

- **Proc macros / generated AST injection:** User code rewrites arbitrary code or injects typed AST. Status: `Reopen`; D-METADEPTH1 and D-CTCODEGEN1 reject this for v1.
- **Reader macros / grammar mutation:** Libraries add sigils, keywords, or grammar. Status: `Rejected/Reopen`; D-EXT1 treats Tier 4 as a global footgun.
- **Full Jai metaprogramming/message loop:** Compiler-message-loop style user macros. Status: `Tower c154 frozen`, post-self-host; v1 explicitly rejects it.
- **Post-self-host macro ecosystem:** Reconsider deeper metaprogramming after self-host. Status: `Tower c154 frozen`.
- **Reversible computation / solve for input:** Run functions backward or solve constraints. Status: `Deferred` D-REVERSE1.
- **Formal verification / proof integration:** SMT/proof-carrying code, liveness/always-responds proofs. Status: `Deferred` D-VERIFY1.
- **Formal proofs for responsiveness/performance:** Always-responds and constant-time proof tracks. Status: `Deferred` D-VERIFY1/D-BUDGET1. Bundle-risk: responsiveness and performance proof systems differ.
- **Full logic-programming subset:** Multi-answer/failure-driven computation beyond comprehensions. Status: `New/Reopen`; research notes caution against the full version.
- **Structural merge:** Merge by semantic identity rather than text. Status: `New/far-horizon`; not carded.
- **Structural merge by meaning:** Version-control merges understand program structure. Status: `New/far-horizon`.
