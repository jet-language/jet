# Jet Idea Master List

**Status:** Owner Review
**Cleaned:** 2026-06-25
**Rule:** This is a deduped master list, not a rejection list. No unique idea was intentionally removed; repeated ideas were merged and stale syntax was annotated against the current project state.

**Status legend:** `Implemented` = built or documented as shipped; `Ratified` = owner-approved, may still be pending implementation; `Tower` = current live/frozen Tower card; `Planned` = roadmap/sidequest plan exists; `Deferred` = captured for later ballot; `New` = no current card/decision found in this pass; `Reopen` = would reverse or materially revise an earlier decision.

## Language Surface And Code Organization

- **Coroutines / async-await / Go-scale networking:** Add a high-concurrency async runtime rather than only blocking tasks. Status: `Planned/Reopen` via Epoch 3 `async-networking.md`; current shipped model teaches against `async`/`await` and uses tasks/channels.
- **No function colors:** Keep ordinary-looking code and avoid async-coloring where possible. Status: `Implemented` for blocking tasks/channels; Epoch 3 async would revisit how visible the async boundary is.
- **Selective imports:** Bring specific items into scope, optionally grouped. Status: `Implemented` as `use math.item` / `use math.{a, b}`; old `use a::b::{X}` syntax is stale.
- **Aliased imports:** Rename an imported module/file namespace at the use site. Status: `Implemented` for `use core.fs as fs`; item-level alias inside grouped imports was not found.
- **Glob imports:** `use module.*` / `use a::*` to import everything. Status: `Reopen` because D-MOD2 rejects wildcard imports with E0612.
- **`pub use` re-export facades:** Public API can differ from file layout by re-exporting submodule items. Status: `Implemented` by D-MOD4 with `pub use sub.Item`.
- **`pub(package)` visibility tier:** Visible within the package, hidden outside it. Status: `New/Reopen`; S18/D-MOD3 currently have only private + `pub`.
- **Public-by-default or `#scope_file` visibility:** Flip private-by-default or make a positional public/private split. Status: `Reopen`; S18 explicitly chose private-by-default plus per-item `pub`.
- **In-file namespace subgrouping:** Optional grouping block inside one file without creating a separate file. Status: `Partially implemented` as inline `module name { ... }`; `namespace {}` as a separate spelling is `New/Reopen`.
- **Generic modules:** Instantiate a module with a type or value parameter. Status: `New`; generic types/functions exist, but module generics were not found.
- **External inherent methods / `extend` blocks:** Add methods to a type from another file. Status: `Partial/Reopen`; `impl Type {}` exists, S83 `Type~~member` is ratified/pending, but a distinct `extend` keyword is not current.
- **Type alias keyword (`def` / `alias`):** Add a transparent alias spelling. Status: `Reopen`; S14 rejects `def` aliases and D-SUGAR3 declined transparent type aliases for now.
- **Variadics and spreading:** Function variadics and spread syntax for calls/lists. Status: `Deferred/New`; D-FP6 deferred list spread, function variadics were not found.
- **Ignoring multiple return values / ignored errors:** Allow discarding secondary results/errors like Jai. Status: `Reopen`; Jet uses `T ? E`, and ignored fallible results are errors/tests today.
- **Prelude / no-prefix common names:** Curated auto-imported common set. Status: `Implemented` for `print`/`input`; user opt-out and `#builtin`-style library escape are `New/Reopen`.
- **Full CoreLib in the REPL:** Make the REPL able to execute Core calls inline. Status: `Tower c133` ready plan.
- **`$` for comptime or macros:** Reserve a sigil for generated-code splice/comptime. Status: `Ratified/Tower c162` as `$` splice plus `comptime {}`; full macro meaning is rejected for v1 by D-METADEPTH1.
- **Dot-inferred construction:** Use `.{ ... }` / `T.{ ... }` for inferred construction. Status: `Ratified/Tower c158`; implementation pending, and exact Zig-style `.field =` syntax would still be a reopen.
- **Enum dot expansion / implied constructors:** Use `.Variant` and dot inference where the type is known. Status: `Implemented` for enum variants; generalized dot construction is c158.
- **Switch/multi-pattern cleanup:** Use single `|` for structural pattern alternatives and reserve `||` for boolean logic. Status: `Implemented` by D-PATO; older `switch`/`when` examples are stale after D-IF revisions.
- **Formatter preserves grouping parentheses:** `jet fmt` should not erase author-written parentheses used for clarity. Status: `New` if it still happens; no current Tower card found.
- **Formatter preserves single-line bodies:** Keep compact one-line bodies when author wrote them that way. Status: `Ratified` D-FMT1.
- **One canonical syntax policy:** Teach foreign spellings but do not accept aliases long-term. Status: `Implemented/Ratified` by S14.

## Metaprogramming, Build-Time Code, And Extensibility

- **Library extensibility tier policy:** Define allowed power tiers from protocols to DSLs to macros. Status: `Ratified` D-EXT1; Tier 1 hooks open to all, stdlib-only marked DSLs, proc/reader macros rejected for v1.
- **Blessed protocol hooks:** Core syntax delegates to fixed hooks like iterator/index/literal suffix. Status: `Ratified/Implemented in pieces`; D-EXT1 formalizes the policy.
- **Marked DSL blocks:** Local syntax islands such as `sql!{...}`. Status: `Ratified as stdlib-only future` by D-EXT1; no general third-party DSL block found.
- **Proc macros / generated AST injection:** User code rewrites arbitrary code or injects typed AST. Status: `Reopen`; D-METADEPTH1 and D-CTCODEGEN1 reject this for v1.
- **Reader macros / grammar mutation:** Libraries add sigils, keywords, or grammar. Status: `Rejected/Reopen`; D-EXT1 treats Tier 4 as a global footgun.
- **User-authored derives and reflection:** Libraries author derives using a typed reflection API. Status: `Tower c155` deciding; v1 ceiling ratified as reflection/derives only.
- **Reflection read API:** A type exposes fields/types/markers to comptime code. Status: `Open decision` D-METAREFLECT1 under c155.
- **Derive output mechanism:** User derives emit source fragments that re-enter lexer/parser/sema. Status: `Open decision` D-METADERIVE1 under c155; D-CTCODEGEN1 already ratifies the architecture rule.
- **Full Jai metaprogramming/message loop:** Compiler-message-loop style user macros. Status: `Tower c154 frozen`, post-self-host; v1 explicitly rejects it.
- **Broad gated build-time I/O:** Allow env/network/subprocess/codegen at comptime behind audit and reproducible locks. Status: `Ratified/Tower c157` as D-CTEFFECT1 tiers plus `#Impure`; implementation pending.
- **Narrow build-time embedding:** `embed_file` / `embed_bytes` only, literal paths. Status: `Ratified` D-CTIO1; broader I/O is the c157 follow-on.
- **Jai/Zig-style build system:** Treat builds as programmable Jet rather than external scripts. Status: `Partial/Planned` via Jetpack, D-BUILDPROFILE1, D-WORKSPACE1, D-CTEFFECT1.
- **Named build profiles:** User-defined `release`/`debug`/`ci` style profiles selected by flags. Status: `Ratified/Tower c159`.
- **Monorepo workspace surface:** First-class `workspace.jet` / workspace module for package sets. Status: `Tower c156` deciding.

## Effects, Capabilities, And Safety Tags

- **Effect system:** Infer and expose each function's effects (`Net`, `Fs`, `Db`, etc.). Status: `Ratified/Implemented` D-EFF1/2/3/4/5.
- **Effect ceilings / `#(no_net)` prohibitions:** Prove a call graph cannot perform a forbidden effect. Status: `Deferred` D-PROP1.
- **Scoped capabilities:** Grant a power for a lexical scope, revoke on exit, prevent escape. Status: `Implemented` D-SCAP1 with `#grant(...)`.
- **Smart Context:** Lexically swap ambient context fields such as allocator/logger. Status: `Implemented` D-CTX1 with `#Context(field: value)`.
- **Checked determinism:** `#Pure fn` means reproducible, not just "no obvious side effects." Status: `Implemented` D-DET1.
- **Injected Clock/Rng:** Time/randomness passed as deterministic capabilities instead of globals. Status: `Implemented` D-DET1/D-DET-CAPAPI.
- **`assume_deterministic` escape:** Expert block that suspends determinism checks. Status: `Implemented` D-DET1.
- **Taint tracking:** Untrusted values cannot reach sinks without sanitizer. Status: `Implemented` D-TAINT1 / D-TAINT-SAN.
- **Full information-flow/compliance tracking:** Security-label lattice such as "EU data cannot leave EU." Status: `Deferred` D-IFC1.
- **Units of measure:** Dollars vs euros, ms vs seconds, typed dimensions. Status: `Implemented` D-UNIT1/D-QUAL3 via `#UnitFamily` minting distinct numeric types.
- **Distinct IDs/newtypes:** Prevent mixing `OrderId` and `CustomerId`. Status: `Implemented` D-DIST1/2/3.
- **Single-use / linear values:** Values that must be consumed exactly once. Status: `Implemented` D-LIN1 `#SingleUse`; explicit audited drop hatch remains unspecified.
- **Must-use results:** Ignored important results are errors unless intentionally discarded. Status: `Ratified/Partial`; `#SingleUse` and fallible ignored-result diagnostics cover core cases.
- **Typestate:** Order-sensitive APIs, wrong-state calls are compile errors. Status: `Implemented` D-STATE1; `state {}` declaration follow-up is Tower c163.
- **Time-varying roles:** Roles that change over time beyond basic typestate. Status: `Deferred` D-ROLE1.
- **Maturity tags:** `#Experimental` / `#Tested` / `#Hardened` dependency lattice. Status: `New`; likely rides tag/effect machinery.
- **Tracked uncertainty/freshness/precision:** Values carry "estimate", "possibly stale", "±5%" dimensions. Status: `Partial/New`; Option/taint cover two axes, broader uncertainty is not carded.
- **Budgets/cost/resource tags:** Time, allocation, latency, and complexity caps as type/effect facts. Status: `Deferred` D-BUDGET1; cost-axis scratchpad remains broader than current ballot.
- **TTL / expiring values and rotting secrets:** Values/cache entries expire automatically or become unusable. Status: `New/library`; no Tower card found.

## Transactions, Rollback, And Reliability

- **Transactional blocks:** Roll back mutations on failure. Status: `Implemented/Ratified` D-TXN1; rollback trait shape ratified as D-ROLLBACK-TRAIT.
- **Irreversible-effect guard in transactions:** Reject network/fs/exec inside rollback blocks unless deferred post-commit. Status: `Implemented` D-TXN2.
- **Post-commit hooks:** Run irreversible effects only after transaction commit. Status: `Implemented` D-TXN3/D-TXN4 with named transaction handles.
- **Rollback hooks / custom rollback:** User customizes snapshot/restore or registers undo hooks. Status: `Ratified/Partial` D-ROLLBACK-TRAIT; associated-types completion gates full trait dispatch.
- **Try-both-keep-the-winner / race winner:** Run alternatives and keep the successful/winning one. Status: `New/Planned-adjacent`; transaction covers rollback, not general racing.
- **Bare `undo` keyword:** Explicit language keyword to reverse an operation. Status: `Reopen`; D-SUGAR5 declined nearby cleanup sugar and transactions use `#Transact`.
- **Record-and-replay:** Deterministically replay executions for debugging. Status: `Deferred` D-REPLAY1.
- **Reversible computation / solve for input:** Run functions backward or solve constraints. Status: `Deferred` D-REVERSE1.
- **Safe schema changes:** Breaking data-shape changes require migrations. Status: `Implemented/Ratified` D-MIGRATE1/2.
- **Self-versioning values:** Values carry version and conversion history. Status: `Partial`; schema snapshots/migrations cover published shapes, not every value.

## Type System And Bug Prevention

- **Make bad states impossible:** Sum types, distinct types, typestate, and linear/single-use values. Status: `Implemented/Ratified` across S30, D-DIST, D-STATE, D-LIN.
- **No null / forced maybe handling:** Absence is `T?`, not ambient null. Status: `Implemented` S32/S71.
- **First-class unknown/loading/pending/never:** Model loading/pending/never explicitly. Status: `Already expressible` with enums/options; no special feature needed.
- **General refinement types:** User-defined constraints such as positive integers. Status: `Deferred` D-REFINE1; `[T#N]` fixed-size arrays are the implemented narrow case.
- **Formal verification / proof integration:** SMT/proof-carrying code, liveness/always-responds proofs. Status: `Deferred` D-VERIFY1.
- **Out-of-bounds checks with proof escape:** Bounds checked by default, prove in-range to elide. Status: `Partial`; safe indexing exists, proof-based unchecked tier not found.
- **Assignment in conditions guard:** Ban or teach `if x = 5`. Status: `Likely already grammar-rejected/New`; no dedicated card found.
- **Checked integer overflow:** Trap by default; explicit wrapping/saturating/checked opt-ins. Status: `Implemented` D-NUMOPS1/2.
- **Decimal money type:** Exact base-10 type plus lint against float money. Status: `New`; no `Decimal`/money lint card found.
- **Honest numbers:** Numeric values track precision/error bounds. Status: `New/library`; overlaps uncertainty tags but not carded.
- **Float equality / semantic smell lints:** Warn on plausible bugs like float `==`, constant conditions, duplicate branches. Status: `New`; no Tower card found.
- **Confusable-name lint:** Warn on `users` vs `user`, `l` vs `1` in same scope. Status: `New`; did-you-mean exists, same-scope confusables do not appear carded.
- **Copy-paste drift / structural-dup lint:** Detect 3-of-4 copied blocks updated. Status: `New/tooling`; no Tower card found.
- **Safety ladder documentation:** Beginner/working/expert tiers as an explicit model. Status: `Implemented/philosophy`; Tower c120 audits separation.

## Concurrency, Dataflow, And Runtime Semantics

- **Tasks and channels:** Safe concurrency without shared mutable state. Status: `Implemented` in `core.tasks`; task detach is D-DETACH1.
- **Structured concurrency / nursery scope:** Lexical task scope cannot exit until children finish; cancellation/deadline context. Status: `New/Planned-adjacent`; tasks exist, nursery scope not found.
- **Coroutines as primitives:** Suspend/resume execution without full async function coloring. Status: `New/Reopen`; async plan uses `@async`/`await`, not generic coroutines.
- **Auto-parallelism:** Sequential-looking maps/folds run in parallel when proven safe. Status: `New/Reopen`; current design favors explicit tasks and rejects hidden machinery in D-REACT1.
- **Living graph / core reactivity:** Values update dependents like a spreadsheet. Status: `Reopen`; D-REACT1 chooses library/tooling, not core evaluation semantics.
- **Reactive library:** Signals/derived/effects as ordinary values. Status: `Implemented` D-REACT1 with `jet.reactive`.
- **Ask-why / value provenance:** Ask a value where it came from. Status: `New/Reopen`; not covered by current logging/tracing.
- **Time-travel variable history:** Keep variable histories for debugging. Status: `New/Reopen`; related to debugger, but no value-history feature found.
- **Failure-as-hole / return-a-hole:** Typed missing/hole values flow instead of crashing. Status: `Partial/Reopen`; options/results/enums cover explicit cases, no universal hole.
- **Failure-aware comprehensions:** List builders auto-skip failed elements. Status: `New`; gentle logic-programming slice not carded.
- **Full logic-programming subset:** Multi-answer/failure-driven computation beyond comprehensions. Status: `New/Reopen`; research notes caution against the full version.
- **Adaptive runtime:** Adjust to battery/network/load/carbon or fidelity under load. Status: `New/library/framework`; no card found.
- **Latency/deadline propagation:** Deadlines flow downhill through calls. Status: `Partial/New`; Smart Context could carry this, but no deadline-specific card found.
- **Approximate algorithms:** Trade accuracy for speed automatically or by policy. Status: `New/library`; no card found.

## Tooling, Compiler APIs, And Refactoring

- **Stable semantic-index query API:** Expose compiler/LSP facts as a public query API. Status: `New`; internal `Source/LSP/SymbolDB.rs` exists, but no stable external API/card found.
- **Ask-your-codebase query engine:** Structural questions like "where can balance go negative?" Status: `New`; should ride semantic-index API.
- **Dossier/outline views for scattered types:** Stitched views of a type across files and impls. Status: `New/tooling`; LSP foundations exist, no dedicated Tower card found.
- **Breadcrumb/phantom-stub editor hints:** Show scattered methods near the type body. Status: `New/tooling`; likely rides semantic index.
- **Generated docs from semantic graph:** Jetdoc-style generated docs from the compiler graph. Status: `Partial`; doctests/docs exist, semantic doc generator not found.
- **Impact/blast-radius analyzer:** Show downstream effects of a change. Status: `New/tooling`; rides semantic index.
- **Replayable/reversible codemods:** Refactors as named objects that can be shipped/undone. Status: `New/tooling`; LSP rename/fixes exist, codemod object model not found.
- **Content-addressed build cache:** Hash normalized definitions/build inputs for incremental work. Status: `Partial implemented` via `BuildCache.rs`/`SHA256.rs`; normalization contract still important.
- **Content-addressed definitions/names:** Definition identity is body hash; renames become alias moves. Status: `New/Reopen`; conflicts with file-is-program instincts, no current card found.
- **Structural merge:** Merge by semantic identity rather than text. Status: `New/far-horizon`; not carded.
- **Tiny formal core / desugaring map:** Document the kernel every surface feature lowers to. Status: `New/process`; no formal kernel doc found.
- **Compiler internal seams:** Split compiler into documented lexer/parser/sema/TIR/codegen APIs. Status: `Ratified/Tower c160`; this helps semantic tools but is not a public query API by itself.
- **Debugger / DAP full native stepping:** Step native builds in Jet terms. Status: `Tower c144`; interpreter debugger step 1 is implemented.

## Standard Library, Core Library, And Packages

- **Tiny composable interfaces:** Reader/Writer/Iterator underpin files, sockets, encoders. Status: `Implemented` for streaming I/O and iterators.
- **Full lazy iterator adapter set:** Rich map/filter/fold/window/chunk/group_by/etc. Status: `Ratified/Implemented` D-ITER1; verify individual gaps only if needed.
- **Errors as values with context:** Fallible values plus cause/context chain. Status: `Implemented` for `T ? E`, `?`, rich `Error`, and conversions.
- **Safe-by-default sharp-on-request APIs:** Verified TLS, linear regex, explicit unsafe. Status: `Implemented/Ratified`; Decimal remains a separate gap.
- **One-line common case, layered expert path:** Simple helpers plus deeper APIs. Status: `Implemented/design principle`.
- **Observability in the box:** Structured logs, tracing, metrics. Status: `Implemented` E2-M12/D-OBS3; OTel exporter remains package-level future.
- **Doc examples run as tests:** Examples are tests and docs. Status: `Implemented` D-TEST4.
- **Property-based testing / self-fuzzing:** Parameterized tests with generation/shrinking. Status: `Implemented` D-TEST1.
- **Coverage:** `jet test --coverage` tooling. Status: `Implemented` D-COV1.
- **Editions/epochs:** Controlled evolution without breaking old code. Status: `Implemented`.
- **Stdlib ergonomic laws/checklist:** Written API review rubric for safe, obvious APIs. Status: `New/doc`; principles are mostly practiced but not a standalone checklist.
- **Collections breadth:** Add `Set<T>` and `Deque<T>`/ring buffer. Status: `New`; no card found.
- **Text grapheme iteration/normalization:** Human-visible Unicode clusters and normalization. Status: `New/std`; strings are Unicode-aware but grapheme API was not found.
- **Display/Debug split:** Separate user display from debug representation. Status: `New/minor`; formatting/interpolation exist.
- **Time library depth:** Dates, zones, calendar math, injectable clock. Status: `Implemented/Partial`; deterministic `Clock` exists, richer `jet.time` should be verified against expectations.
- **Arbitrary-precision integers:** BigInt type. Status: `New/std`; no built-in BigInt found.
- **Random split:** Fast seedable PRNG vs crypto RNG. Status: `Partial/Implemented`; `core.random` and `jet.crypto` exist, API clarity may need review.
- **Path objects and atomic write/dir-walk:** Safer filesystem API than raw strings. Status: `Partial`; path/list_dir work exists, full Path/atomic-write surface may need verification.
- **Safe subprocess:** Arg-list process APIs, never shell strings by default. Status: `Implemented`.
- **Unified serde data model:** One derive for JSON/CSV/TOML/YAML/etc. Status: `Implemented/Ratified` D-SERDE1–12/D-ENC1.
- **Dynamic `Data` tree for formats:** One rich parse tree across JSON/TOML/YAML/CSV. Status: `Implemented` D-ENC-DYN1.
- **JSON/CSV/TOML/YAML/archive modules:** First-party data format ring. Status: `Implemented/Planned`; YAML core implemented, full YAML tags are Tower c153.
- **Compression formats:** zip/tar/gzip/zstd/brotli-style support. Status: `Partial`; `jet.archive` planned/built-from-source track c50, broader codecs not all verified.
- **Linear-time regex:** RE2-style non-backtracking default. Status: `Implemented` D-REGEX1; native engine remains an obligation/future.
- **Networking crown jewels:** HTTP client/server, routes, TLS, URL, WebSocket. Status: `Partial/Planned`; HTTP/routing/TLS exist, WebSocket/Go-scale server is Epoch 3 async plan.
- **Misuse-resistant crypto API:** High-level `seal`/`sign` envelope hiding nonce footguns. Status: `New`; crypto primitives exist, envelope API not found.
- **Post-quantum crypto agility:** Hybrid X25519+ML-KEM behind safe API. Status: `New/far-horizon`; sequence after safe crypto envelope.
- **CLI arg parsing:** Declarative args with generated help. Status: `Implemented/Ratified` D-ARGS1.
- **UUID and encoding utilities:** UUID v4/v7, base64, hex. Status: `New/std`; base64 appears only in FFI examples, not as Core API.
- **Database driver interface:** General `database/sql`-style interface, parameterized-only. Status: `Partial`; SQLite/db work exists through c50/`jet.db`, generic driver interface not found.
- **Embedded/no-runtime library layering:** `core ⊂ alloc ⊂ std`, same code from server to microcontroller. Status: `Partial/Planned`; freestanding/no-std direction exists, full ring layering is not designed.
- **Explicit allocation at boundaries:** Caller-supplied buffers/allocators for fixed-memory use. Status: `Ratified/Partial`; allocator/arena work exists, broad std API convention not complete.
- **Opt-in GC library:** Library-provided garbage collector for long-running processes. Status: `New`; no card found.
- **Cross-platform raylib builtin:** First-party/native graphics/raylib package. Status: `New`; examples exist, built-in package not found.
- **Built-in vectors/swizzling:** Vector types with `x/y/z/w` style access. Status: `Partial`; SIMD/linalg vectors are implemented/ratified, named swizzles deferred/not found.
- **Component-wise vector arithmetic:** Vector operators apply lane-wise by default. Status: `Implemented` for closed SIMD lane types; broader user vector overloading is not open.
- **`jet.linalg` / math package:** Vectors, matrices, decompositions, FFT later. Status: `Ratified/partly implemented` D-MATHLIB1/D-SIMD1/2.
- **Package build-from-source:** Jetpack realizes deps from source and ships ring packages. Status: `Tower c50`.
- **Signed package cache:** Signed binary/source package cache. Status: `Tower c56 frozen`; checksum/signing floor partially covered by package signing decisions.
- **Publish/registry UX:** Real registry upload, semver resolver, publish flow. Status: `Tower c96`.
- **Plugin target:** Build/load plugin packages safely. Status: `Tower c81` deciding.

## UI, Full-Stack, And Interop

- **Typed client/server protocol:** Declare protocol/session once and generate both sides. Status: `Deferred` D-PROTO1; strategically important for TypeScript replacement.
- **One type system across the wire:** Eliminate hand-synced TS/Zod/tRPC layers. Status: `Partial`; serde exists, protocol/session generation remains D-PROTO1.
- **Reactive UI stack:** Reactivity → view model → typed styles → headless/styled kit → motion/app kit. Status: `New/Reopen`; depends on whether core reactivity decision is reconsidered.
- **Ownable component kit:** Copy-in-and-own UI components rather than locked theme/framework. Status: `New`; no UI card found.
- **Typed styles:** CSS/style layer with compile-time property/unit checking. Status: `New`; rides units and UI track.
- **Accessibility by default:** Components ship ARIA/focus/keyboard behavior and release-gated a11y diagnostics. Status: `New`; no card found.
- **Motion as reactive state:** Animation is derived state, not a separate runtime. Status: `New/Reopen`; downstream of reactivity call.
- **Render-target abstraction:** Web/native/embedded/TUI as backend traits. Status: `New`; research says decide early, no card found.
- **Web backend JS DOM + WASM:** Emit JS DOM ops for views, keep logic/compute in WASM. Status: `New/planned-adjacent`; no web backend card found.
- **Native app backend:** FFI to native widgets first, own-renderer later. Status: `New`; C FFI exists, native UI strategy not carded.
- **JS/npm and Swift interop:** Let users call existing ecosystems from day one. Status: `New`; only C/Rust FFI and package plans exist.
- **TypeScript/Swift competitive analysis:** Keep the "replace TS/Swift" feature lens explicit. Status: `Research note`; concrete ideas are protocol, UI, interop, web/app backends.

## Performance, Layout, And Low-Level

- **Arenas/temp storage:** First-class arena and bump allocation patterns. Status: `Implemented/Ratified` core arena regions; compiler inference remains Tower c26.
- **Implicit swappable allocator context:** Jai-style ambient allocator. Status: `Implemented/Ratified` as `#Context(allocator: ...)` plus explicit allocator APIs; broad policy still evolving.
- **Arena allocator compiler inference:** Compiler chooses arena placement. Status: `Tower c26` far-horizon.
- **Manual scoped cleanup / defer:** Run cleanup at scope exit. Status: `Implemented` as `core.scope.guard`; `defer` keyword remains a reopen.
- **Low-level pointer syntax cleanup:** Retire `Ptr<T>`/old `std.mem` syntax. Status: `Implemented` D-CAP9 uses `*T` and postfix `p.*`; Core naming replaced `std`.
- **Labeled `ref` fields / original ownership model relook:** Revisit stored references and ownership vocabulary. Status: `Partial/Implemented`; capability sigils and `ref` hardening landed, exact old note likely stale.
- **Visible uninitialization:** Opt out of zero-fill with write-before-read proof. Status: `Tower c76` ready; fixed-array prerequisite is now satisfied.
- **Columnar/SoA layout:** Struct-of-arrays storage for cache-friendly data. Status: `Implemented` D-SOA1/2 with `#layout(columnar)`.
- **C-compatible layout:** `#layout(c)` and related layout controls. Status: `Ratified/Implemented` D-REPRC1.
- **Portable SIMD:** Safe lane types and operations. Status: `Implemented` D-SIMD1/2.
- **JIT tier:** Add Cranelift JIT, later own bytecode/native JIT. Status: `Tower c139 ready`, c140/c141 frozen.

## Research / Far-Horizon Notes

- **Content-addressed package/store identity:** Use hashes for dependencies, cache entries, and reproducibility. Status: `Partial implemented` in package/build cache; broader content-addressed definitions remain separate.
- **Structural merge by meaning:** Version-control merges understand program structure. Status: `New/far-horizon`.
- **Formal proofs for responsiveness/performance:** Always-responds and constant-time proof tracks. Status: `Deferred` D-VERIFY1/D-BUDGET1.
- **Adaptive fidelity under load:** One knob for quality/perf tradeoff. Status: `New/library/framework`.
- **Carbon/battery-aware runtime policies:** Runtime responds to environmental constraints. Status: `New/library/framework`.
- **Post-self-host macro ecosystem:** Reconsider deeper metaprogramming after self-host. Status: `Tower c154 frozen`.
