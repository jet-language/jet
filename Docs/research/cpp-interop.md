# C++ interop for incremental adoption

**Status:** research note for Tower card #2901. This note records the adoption boundary and evidence; it does not add a C++ bridge implementation, a new dependency, or a new public command.

**Claim tags:** `[PRIMARY]` is a primary source statement; `[OBSERVED]` is a repository or command observation; `[INFERENCE]` is a design consequence of those observations; `[PROPOSED]` is a ballot-ready alternative, not a selected design.

## Executive summary

Jet already has a ratified C++ boundary. `D-FFI-CPP1=A` specifies a Clang-based binder that emits a generated, cached C shim archive; classes are opaque `#SingleUse` handles with consuming cleanup, exceptions become `T !CppError`, overloads become argument labels, operators become named methods, and templates instantiate on demand ([ratified syntax decision](../spec/syntax-decisions.md#L3248-L3258)). The unified FFI specification further requires an explicit namespace, target and absolute Clang/archiver paths, provenance for search and link inputs, and reuse of that provenance at final link ([spec](../spec/spec.md#L1340-L1354)).

That boundary is the useful incremental-adoption story: C++ remains C++ on the native side, while Jet consumes one checked C ABI projection. It is not a promise to import arbitrary C++ object layout or to guess an ABI from mangled names. The C++ ABI document explicitly says that platform vendors retain authority over their C++ ABI ([Itanium ABI, introduction](https://itanium-cxx-abi.github.io/cxx-abi/abi.html#intro)).

The first prototype binds a real class with a constructor, two overloads, a throwing method, a callback, and a threaded free function. Binding succeeds and produces the expected opaque-handle/fallible projection. Execution is currently blocked in all three requested native paths before the backend runs: the generated projection contains `Counter.{ value: value }`, while the current checker requires `Counter{…}`. This note records that exact blocker rather than claiming cross-tier output.

## Constraints Jet cannot weaken

Jet's invariants apply to every approach below:

- **I2:** rejection of generated backend code is an internal compiler error, not a user diagnostic. Unsupported C++ shapes therefore must be rejected by Jet's own semantic boundary before generated code reaches a backend.
- **I4:** an unsupported ownership, ABI, template, exception, or standard-library shape needs a registered diagnostic with What/Why/Fix text and a UI snapshot.
- **I6:** compiler and compiler-seam crates use path dependencies only; an external Clang or archiver is an admitted tool seam, not a new compiler-crate dependency.
- **I9:** AOT, default `jet run`, `jet run --interpret`, and web when applicable preserve one meaning. A C++ feature cannot be silently AOT-only or gain a second runtime implementation.

These are the repository invariants in [`AGENTS.md`](../../AGENTS.md#L29-L37), not new C++ policy.

## Four incumbent approaches

The comparison uses the four approaches named by card #2901. They solve different points in the design space; none is evidence that Jet should copy its whole implementation.

| Approach | What the primary source actually provides | Incremental-adoption shape | Hard boundary relevant to Jet |
|---|---|---|---|
| **Zig: `translate-c` plus a C-facing boundary** | Zig documents `zig translate-c` as a one-file C translation CLI. Flags are forwarded to Clang, and the translated file is written to stdout ([Zig language reference](https://ziglang.org/documentation/master/#C-Translation-CLI)). The same page warns that target and C flags must match the eventual compile or subtle ABI incompatibilities can result ([target/cflags warning](https://ziglang.org/documentation/master/#Using--target-and--cflags)). | Existing C++ can stay in its native build if it exposes a C-compatible wrapper. A selected template specialization can be exposed as a concrete C function/type. The documented primitive is C translation, not a general C++ semantic importer. `[INFERENCE]` Arbitrary C++ templates, overload sets, classes, and exceptions therefore need a C++ wrapper or an equivalent explicit selection step before a Zig-facing declaration exists. | Do not treat `translate-c` as proof that a C++ header is directly importable. Do not infer symbols, class layout, or exception behavior from a translated C view. Jet would need the same target/flags identity check and a checked boundary.
| **Swift: direct Clang C++ importer** | Swift 5.9 added C++ interop. The compiler embeds Clang and imports C++ headers through Clang modules; Swift calls imported C++ functions and types directly ([Swift guide: introduction/importing/working with APIs](https://www.swift.org/documentation/cxx-interop/#introduction), [importing C++](https://www.swift.org/documentation/cxx-interop/#importing-c-into-swift), [working with imported APIs](https://www.swift.org/documentation/cxx-interop/#working-with-imported-c-apis)). | A C++ project can add a module map and consume many non-templated functions, constructors, member functions, operators, and supported value/reference types without hand-written wrappers. A class-template specialization must already be instantiated and named in a C++ header ([Swift status: templates](https://www.swift.org/documentation/cxx-interop/status#c-types-supported-in-swift)). | Swift cannot catch C++ exceptions; an uncaught exception reaching Swift terminates the program ([Swift status: exceptions](https://www.swift.org/documentation/cxx-interop/status#c-exceptions)). Mixed code must use the same C++ standard library ([Swift status: standard library](https://www.swift.org/documentation/cxx-interop/status#c-standard-library-support)). Direct import is powerful, but it makes target ABI, ownership annotations, reference lifetimes, and supported-type tables part of the importer contract.
| **Carbon: bidirectional semantic interop** | Carbon's primary design says a subset of C++ APIs is available in both directions, including classes/structs and templates, with wrappers and generic programming used to minimize overhead ([Carbon philosophy](https://raw.githubusercontent.com/carbon-language/carbon-lang/trunk/docs/design/interoperability/philosophy_and_goals.md#philosophy)). It targets C++17 and allows mixed toolchains only when ABI-compatible; mixed-toolchain support is intentionally degraded but must not semantically diverge ([toolchain goals](https://raw.githubusercontent.com/carbon-language/carbon-lang/trunk/docs/design/interoperability/philosophy_and_goals.md#support-mixing-carbon-and-c-toolchains)). | This is the strongest “adopt one module at a time” ambition: C++ libraries remain usable, Carbon may expose C++ idioms, and bridge code is allowed beside the consumer. It also accepts that C++ code does not receive Carbon's full safety mechanisms ([Carbon safety goal](https://raw.githubusercontent.com/carbon-language/carbon-lang/trunk/docs/design/interoperability/philosophy_and_goals.md#practical-safety-guarantees-and-testing-mechanisms)). | Carbon explicitly lists C++ exceptions without bridge code as a non-goal: an unannotated exception may terminate the program ([Carbon exception non-goal](https://raw.githubusercontent.com/carbon-language/carbon-lang/trunk/docs/design/interoperability/philosophy_and_goals.md#support-for-c-exceptions-without-bridge-code)). Object lifetime rules and inheritance from non-pure-interface classes remain open questions ([Carbon open questions](https://raw.githubusercontent.com/carbon-language/carbon-lang/trunk/docs/design/interoperability/philosophy_and_goals.md#open-questions-to-be-resolved-later)). Jet must turn those risks into explicit checked boundary facts rather than inherit termination behavior.
| **Rust CXX: declared `extern "C++"` bridge** | CXX requires an explicit bridge module whose `extern "C++"` block names the headers, types, and signatures ([CXX extern C++](https://cxx.rs/extern-c%2B%2B.html#extern-c)). C++ types are opaque behind indirection; mutation uses `Pin<&mut T>`, thread safety is not assumed, and signatures are checked with generated C++ static assertions ([opaque types/functions](https://cxx.rs/extern-c%2B%2B.html#opaque-c-types), [functions](https://cxx.rs/extern-c%2B%2B.html#functions-and-member-functions)). Types with nontrivial move behavior or a destructor must be opaque behind a reference or smart pointer; only genuinely trivial types can be passed by value ([CXX `ExternType::Kind`](https://cxx.rs/extern-c%2B%2B.html#integrating-with-bindgen-generated-or-handwritten-unsafe-bindings)). | A large C++ codebase can adopt one reviewed bridge at a time. The bridge declaration is the selected public surface; upstream C++ can retain its class implementation and ownership model. Generic C++ use is selected through concrete bridge declarations/instantiations rather than an open-ended importer. | The declaration is not a complete proof of safety: the checked source says the programmer still owns claims that static information cannot express, including call safety and lifetime meaning. Jet must add diagnostics, provenance, and tier parity instead of treating a bridge declaration as permission to erase those facts. CXX's opaque/nontrivial rule is a good lower bound for Jet's own by-value refusal.

### Per-approach mapping to I2/I4/I6/I9

This is the Jet adaptation of each incumbent, not a claim that the incumbent implements Jet's invariants.

| Approach | I2: sema must own rejection | I4: diagnostic product | I6: dependency seam | I9: one meaning |
|---|---|---|---|---|
| Zig-style C translation/C wrapper | Reject C++-only declarations before a translated C projection can produce backend errors. | Register unsupported C++ shape, target/flag mismatch, and wrapper mismatch with What/Why/Fix. | Invoke an admitted external tool with pinned identity; do not add Zig/Clang libraries to compiler seam crates. | The same selected wrapper/projection must feed AOT, JIT, interpreter, and web when applicable; no C-only fallback tier. |
| Swift-style direct importer | Reject unsupported imported references, templates, and exceptions in Jet sema rather than letting the host crash. | Explain missing module map, unsupported type, lifetime annotation, standard-library mismatch, or exception boundary. | A Clang-driven tool is an external seam with provenance; no hidden Swift runtime or new compiler dependency. | If direct import is ever chosen, the operation and error/ownership facts must be shared by every applicable tier. |
| Carbon-style bidirectional boundary | Reject unsupported mixed-toolchain and semantic-divergence cases before lowering. | Explain when bridge code, ABI-compatible toolchains, or a supported subset is required. | Keep foreign compiler/build tools outside compiler seam crates; make ownership of the selected build explicit. | Native and mixed paths may differ in optimization, not meaning; Jet cannot add a backend-only C++ interpretation. |
| CXX-style declared bridge | Validate declarations against the selected C++ translation unit and reject nontrivial/value/lifetime mismatches in sema. | Show which declaration, header, target, ownership, and safety fact failed. | Treat the bridge generator and C++ compiler as declared tools; preserve package/build provenance. | A bridge descriptor is consumable by all applicable tiers or the operation is rejected as unsupported; AOT-only is not an escape hatch. |

## What incremental C++ adoption actually requires

C++ interop is not just a list of callable names. The ABI specification covers object layout, virtual tables, calling interfaces, exception handling, global naming, and object-code conventions ([Itanium ABI introduction](https://itanium-cxx-abi.github.io/cxx-abi/abi.html#intro)); it also warns that this generic document is not authoritative for every platform. The following table turns those facts into explicit Jet obligations and refusals.

| C++ concern | Evidence | Jet must do | Jet must refuse |
|---|---|---|---|
| **Name mangling, target, and calling convention** | The Itanium ABI treats global naming as part of the object-code interface and defines external-name mangling ([ABI mangling](https://itanium-cxx-abi.github.io/cxx-abi/abi.html#mangling)). The prototype archive contains distinct symbols for `acme::Counter::add(long)` and `acme::Counter::add(double)`, observed as `_ZN4acme7Counter3addEl` and `_ZN4acme7Counter3addEd`. | Let Clang select declarations and target-specific symbols; emit C-linkage shim entry points; pin target triple, Clang, archiver, include paths, library paths, and linked archives in provenance; reuse the same provenance at final link. | Never concatenate or guess mangled names; never expose a C++ object file built for a different target/compiler/standard library; never accept stale descriptor/archive identity. |
| **Exceptions** | The C++ exception ABI uses a language/runtime-specific unwind and personality process ([Itanium exception ABI](https://itanium-cxx-abi.github.io/cxx-abi/abi-eh.html#base-framework)). The fixture's `fail_if_negative` and callback path throw `std::runtime_error` ([fixture](../../tests/fixtures/mixed_repo/cpp/counter.cpp#L9-L16)). | Catch C++ exceptions inside the C++ shim and convert them to `CppError` at every fallible call. Preserve normal results, error code, and cleanup semantics; record only typed Jet boundary errors. | Never let a C++ exception unwind through Jet frames, become a raw backend/host diagnostic, or be silently converted to panic. Do not claim recovery for exception payloads the boundary did not model. |
| **Templates and overloads** | Swift exposes instantiated class/struct specializations, not class templates directly ([Swift status](https://www.swift.org/documentation/cxx-interop/status#c-types-supported-in-swift)); CXX uses explicit bridge declarations. Carbon documents templates as a major interop goal, while mixed toolchains may need bridge code. | Resolve overloads semantically and expose stable Jet labels; request concrete template instantiations on demand, with explicit type arguments and target/tool identity. | Do not promise an arbitrary dependent-template family, universal/rvalue/variadic/non-type template without a checked mapping, or an overload whose declaration is ambiguous. |
| **RAII and object lifetime** | CXX requires nontrivial-move/destructor types to remain opaque behind indirection ([CXX `ExternType::Kind`](https://cxx.rs/extern-c%2B%2B.html#integrating-with-bindgen-generated-or-handwritten-unsafe-bindings)). The C++ ABI has distinct complete, base, and deleting destructor concepts ([ABI definitions](https://itanium-cxx-abi.github.io/cxx-abi/abi.html#definitions)). | Represent class instances as owned opaque `#SingleUse` handles; make cleanup explicit and consuming; preserve borrow/owner facts in signatures; invalidate the handle after close and make failure behavior fallible. | Do not copy/drop a nontrivial C++ class as if it were a Jet value; do not infer destructor safety from a pointer; do not let a borrowed view outlive its owner or permit use after close. |
| **Virtual layout, inheritance, and downcasts** | A dynamic C++ class has one or more virtual tables; those tables contain offsets, data pointers, function pointers, RTTI, and possibly multiple secondary tables ([ABI virtual layout](https://itanium-cxx-abi.github.io/cxx-abi/abi.html#vtable-general)). Carbon leaves inheritance from non-pure-interface C++ types open ([Carbon inheritance question](https://raw.githubusercontent.com/carbon-language/carbon-lang/trunk/docs/design/interoperability/philosophy_and_goals.md#carbon-type-inheritance-from-non-pure-interface-c-types)). | Keep classes opaque and call only declarations selected by Clang. If inheritance is ever admitted, require a separately specified ABI/layout/lifetime contract and tier proof. | Do not expose C++ class layout, vtable pointers, arbitrary base/derived casts, or virtual dispatch as ordinary Jet structs/methods without a ratified target-specific contract. |
| **Standard-library types and borrowed views** | Swift requires the same C++ standard library on both sides and supports a finite list; `std::tuple` and `std::variant` are not fully supported ([Swift standard-library constraints](https://www.swift.org/documentation/cxx-interop/status#c-standard-library-support)). Carbon notes that a `std::vector<T>` mapping can constrain evolution and that common types may need copy-based conversion ([Carbon type-mapping goals](https://raw.githubusercontent.com/carbon-language/carbon-lang/trunk/docs/design/interoperability/philosophy_and_goals.md#unsurprising-mappings-between-c-and-carbon-types)). | Maintain an audited mapping table. Treat `std::string_view`, `std::span`, iterators, and similar views as borrowed values with an explicit owner/invalidation contract; otherwise use an opaque wrapper or an explicit copy. | Do not assume `std::string`, `string_view`, `span`, containers, allocators, or smart pointers have one layout across standard libraries/targets. Refuse unsupported or lifetime-ambiguous types rather than silently copying or borrowing. |
| **Callbacks and threads** | The checked fixture passes a callback and uses `std::thread` ([fixture header](../../tests/fixtures/mixed_repo/cpp/counter.hpp#L15-L16), [implementation](../../tests/fixtures/mixed_repo/cpp/counter.cpp#L13-L22)). The generated descriptor represents callbacks through a C ABI and marks callback validation/reentrancy capabilities ([observed generated projection](#generated-projection)). | Generate a checked C callback trampoline with a Jet signature, contain callback errors/throws, and state thread/reentrancy assumptions in the descriptor. | Do not allow an unbounded C++ callback to call arbitrary Jet state, unwind through Jet, or outlive the closure/host capability without a checked lifetime contract. |

## The four project arrangements

The foreign-source proposal calls these four arrangements, and explicitly marks their commands and generated APIs as proposed UX rather than existing capability ([proposal](../proposals/ffi-owned-source-and-boundaries.md#four-project-arrangements-four-complete-experiences)). They are adoption workflows, not four competing C++ binders.

| Arrangement | C++ adoption requirement | I2/I4/I6/I9 consequence | Refusal condition |
|---|---|---|---|
| **1. Jet application with owned C++ code** | The Jet package owns the C++ source, headers, defines, include roots, target, and build facts. Bind the original header, but check the implementation's ownership, effects, callbacks, and throw behavior rather than trusting declarations alone. | Sema can report a covered fault at the selected foreign source; diagnostics remain Jet-owned; Clang/C++ tools remain admitted external seams; all applicable tiers consume the same descriptor. | Do not claim safety because source is nearby. Unknown macros, target flags, ownership, or implementation-dependent layout remain blockers. |
| **2. Jet application with an external C++ package** | Install the generated Jet API and evidence matched to the actual package/archive; provenance must bind header, archive, toolchain, target, and link inputs. | Mismatched evidence is an I4 diagnostic, not a best-effort link; package metadata is a build seam, not a compiler-crate dependency; every tier uses the matched projection. | Do not vendor a stale generated API beside a different archive or silently rebuild a package with unknown flags. |
| **3. Jet module inside an existing C++ project** | Preserve the existing CMake/native build and native callers. Replace one eligible module with Jet while keeping a C/C++-compatible endpoint and explicit ownership/error contract. | The native build remains the project owner; Jet's checker owns boundary diagnostics; the endpoint has one semantic operation across tiers. | Do not make the Jet module require a second hidden build graph or change native callers' class layout/ABI without an owner-approved migration. |
| **4. Foreign project using Jet's compiler/build driver** | Keep C++ sources and native front ends; Jet owns only the selected shared compilation/build/checking path and records which build steps it controls. | Foreign compiler errors must not become hidden Jet backend failures; selected tools remain explicit dependencies; all Jet execution tiers consume the same bound semantics. | Do not silently take over a CMake/build graph or claim mixed-project support without proving the selected actions and artifact provenance. |

## Current Jet boundary and prototype evidence

### Ratified mechanism

The current law is one mechanism, not a direct-import/shim hybrid:

1. `jet inspect bind cpp <header>` asks Clang for a target-specific AST and emits a content-addressed C-ABI shim archive.
2. The generated `cpp.<library>` projection exposes opaque handles, fallible methods, consuming close, checked callbacks, and on-demand concrete templates.
3. The projection and shim are cached under `.jet/bindings/cpp/`; target, tools, headers, archives, search paths, and libraries are pinned in provenance.
4. AOT, default `jet run`, interpreter, and web when applicable must consume the same descriptor and error/ownership meanings.

This is the direct repository contract ([decision](../spec/syntax-decisions.md#L3248-L3258), [unified FFI spec](../spec/spec.md#L1340-L1354)). A future direct Clang operation or explicit `.cppbind.json` selection would be a new owner decision, not an unannounced second mechanism.

### C++ workload

The checked fixture contains:

- `acme::Counter(int64_t)`, `add(int64_t)`, `add(double)`, and `fail_if_negative(int64_t)` ([header](../../tests/fixtures/mixed_repo/cpp/counter.hpp#L4-L13));
- `apply` with a callback and `threaded` using `std::thread` ([header](../../tests/fixtures/mixed_repo/cpp/counter.hpp#L15-L16), [implementation](../../tests/fixtures/mixed_repo/cpp/counter.cpp#L13-L22));
- a Jet consumer that constructs, calls both overload projections, crosses a callback and thread boundary, exercises the throwing method, and consumes the owner ([Jet fixture](../../tests/fixtures/mixed_repo/cpp/run.jet#L1-L20)).

The implementation deliberately throws on the negative path and on a rejected callback. This gives the boundary a real exception/error case instead of only a successful arithmetic call.

### Prototype commands and observations

All repository commands below were run through `scripts/agent/jet-env`; scratch files are under `$HOME/.cache/jet-test-scratch/cpp-interop-2901`.

**Native C++ archive.** The fixture was compiled as C++17 for the selected target and archived:

```text
scripts/agent/jet-env full clang++ -std=c++17 -fPIC -pthread -c "$HOME/.cache/jet-test-scratch/cpp-interop-2901/counter.cpp" -target x86_64-unknown-linux-gnu -I "$HOME/.cache/jet-test-scratch/cpp-interop-2901" -o "$HOME/.cache/jet-test-scratch/cpp-interop-2901/counter.o"
scripts/agent/jet-env full ar rcs "$HOME/.cache/jet-test-scratch/cpp-interop-2901/libcounter_impl.a" "$HOME/.cache/jet-test-scratch/cpp-interop-2901/counter.o"
```

Observed completion line:

```text
CXX_ARCHIVE_READY target=x86_64-unknown-linux-gnu
```

`nm --defined-only` showed Itanium-style mangled overload symbols, including:

```text
_ZN4acme7Counter3addEl
_ZN4acme7Counter3addEd
_ZN4acme7Counter16fail_if_negativeEl
```

`nm -C` demangled those to `acme::Counter::add(long)`, `acme::Counter::add(double)`, and `acme::Counter::fail_if_negative(long)`. This is why Jet must ask Clang for declarations and emit a stable C-linkage shim instead of guessing C++ symbols.

**Bind.** The header was bound with the target, absolute Clang/archiver paths, namespace, include/library roots, and two linked libraries:

```text
scripts/agent/jet-env full jet inspect bind cpp counter.hpp \
  --target x86_64-unknown-linux-gnu \
  --clang /nix/store/w021fbcg4z6vxihnp6gb6vijyifl051f-clang-wrapper-21.1.8/bin/clang++ \
  --ar /nix/store/w88q44gqd1qg5wmkk7v0h97rpiqvam0l-gcc-wrapper-15.3.0/bin/ar \
  --pkg counter --namespace acme -I . -L . -l counter_impl -l pthread
```

Observed completion:

```text
bound 6 C++ members from `counter.hpp` → .jet/bindings/cpp/counter.jet
CPP_BIND_EXIT=0
```

The generated projection records `CppError`, `#SingleUse` `Counter`, consuming `close_counter(^Counter)`, overload labels (`add_amount`, `add`), callback and threaded calls, and `-[FFI.Cpp]>` effects ([captured artifact](#generated-projection)). The descriptor provenance records `language=cpp`, `transport=clang-cxx-shim`, `binder-schema=jet-cpp-bind-v3`, the source header, target, absolute tools, namespace, and linked archives.

#### Generated projection

The captured generated file is `$HOME/.cache/jet-test-scratch/cpp-interop-2901/.jet/bindings/cpp/counter.jet` in the scratch evidence, with these relevant lines:

```jet
pub enum CppError { Exception InvalidHandle ResourceLimit }

#SingleUse
pub struct Counter { value: Int }

pub fn new_counter(start: Int) Counter !CppError -[FFI.Cpp]> { ... }

impl Counter {
    pub fn add_amount(self, amount: Int) Int !CppError -[FFI.Cpp]> { ... }
    pub fn add(self, factor: Float) Int !CppError -[FFI.Cpp]> { ... }
    pub fn fail_if_negative(self, value: Int) Int !CppError -[FFI.Cpp]> { ... }
}

pub fn close_counter(value: ^Counter) -[FFI.Cpp]> { ... }
pub fn apply(callback: fn(Int) Int -[]>, value: Int) Int !CppError -[FFI.Cpp]> { ... }
pub fn threaded(value: Int) Int !CppError -[FFI.Cpp]> { ... }
```

`[OBSERVED]` The constructor body currently contains `return Ok(Counter.{ value: value })` (generated artifact line 25). That is the exact source-level defect which blocks the three requested executions; it is not an ABI or C++ semantic result.

The binder's C++ shim allocates `Counter` in the generated constructor, clears the slot, and calls `delete value` in the generated `counter_close` entry point inside a `try/catch`. Thus `close_counter(^counter)` is the prototype's destructor/RAII path; it is consuming and error-reporting rather than an implicit Jet value drop ([binder generator](../../crates/jet-pkg-model/src/CppBind.rs#L1119-L1178)).

#### Tier proof and blocker

Each requested native command was attempted against the same fixture:

```text
scripts/agent/jet-env full jet check /home/nate/.cache/jet-test-scratch/cpp-interop-2901/run.jet
scripts/agent/jet-env full jet run /home/nate/.cache/jet-test-scratch/cpp-interop-2901/run.jet
scripts/agent/jet-env full jet run --interpret /home/nate/.cache/jet-test-scratch/cpp-interop-2901/run.jet
scripts/agent/jet-env full jet build --profile=debug /home/nate/.cache/jet-test-scratch/cpp-interop-2901/run.jet
```

All four exited 1 before executing the C++ call. The common diagnostic was:

```text
Error [E0320]: Struct construction uses `Counter{…}`, not `Counter.{…}`
  --> /home/nate/.cache/jet-test-scratch/cpp-interop-2901/run.jet:21:1
    |
 21 |
    | ^
 Why: Literal heads place no dot before their brace (D-LIT-DOT1)
 Fix: Write `Counter{…}`
More: jet-lang.dev/e/E0320

1 problem found
run `jet explain E0320` to learn more
```

Therefore this prototype **does not claim identical output**. It proves the C++ header/archive bind and records an exact pre-execution blocker for AOT/build, default `jet run`, and `jet run --interpret`. Web was not measured: the same source-level failure occurs before backend selection, so no web result is claimed. Fixing the generated syntax and rerunning the shared workload is required before a cross-tier result can be reported.

## Owner gates and ballot-ready alternatives

The owner gate must choose between alternatives for the same `counter.hpp` program; it must not create a hidden second mechanism. `D-CPPINTEROP1` is already filed and ratified, so its options are recorded here, not reopened.

### Ratified ballot: `D-CPPINTEROP1`

The exact repository query was:

```text
scripts/agent/jet-env full node plugins/tower/tower.mjs decision show D-CPPINTEROP1 | jq -r '"status=\(.status) outcome=\(.outcome) options=\([.options[].key]|join(","))"'
```

It returned:

```text
status=ratified outcome=A options=A,B,C
```

The three same-program alternatives are:

- **A — Keep the generated C shim:** complete adoption through the existing header-driven binder and ordinary Jet calls. This is the ratified option.
- **B — Adopt a direct Clang importer:** replace shim lowering with a direct C++ call operation, while preserving the same Counter ownership, errors, callback containment, and tier proof.
- **C — Use an explicit binding declaration:** make a `.cppbind.json` selection the sole input to the existing shim lowerer, while preserving the same generated Jet projection and Counter calls.

No direct importer, explicit declaration file, or second runtime is implied by this research note.

### Future ballot-ready questions (not decisions)

1. **Which surface is admitted for a nontrivial C++ class?** For the same `Counter`, should Jet expose (A) an opaque owned handle with named methods and consuming close; (B) a direct by-value class only when Clang proves trivial move/destructor and layout; or (C) reject the class until a wrapper supplies a C ABI? The current ratified answer is A for the generated binder; B/C would need a new owner decision.
2. **How is a throwing method exposed?** For the same `fail_if_negative`, should Jet (A) catch at the C++ shim and return `CppError`; (B) require the package to provide a no-throw result wrapper; or (C) reject the declaration? Current `D-FFI-CPP1=A` selects A; this question is only for future boundary expansion.
3. **How are templates selected?** For one requested `vector<Int>` or other concrete specialization, should Jet (A) instantiate on demand from the header; (B) require a package-provided explicit instantiation/archive; or (C) reject templates and require a wrapper? Current syntax permits on-demand instantiation; unsupported dependent shapes remain rejected.
4. **How are borrowed standard-library views represented?** For a `std::string_view`/`std::span` method, should Jet (A) require an owner/invalidation contract and expose a borrowed view; (B) make an explicit copy into a Jet-owned value; or (C) reject the method? No lifetime guess is allowed.
5. **Which virtual/inheritance subset is admitted?** For a class inheriting a C++ interface, should Jet (A) expose only an opaque handle and declared virtual calls; (B) admit a separately specified pure-interface ABI contract; or (C) require a C++ wrapper? The ABI/layout contract must be explicit before B can be selected.
6. **Which tiers must carry a C++ binding?** For the same Counter descriptor, should the proof require (A) AOT, default `jet run`, interpreter, and web where applicable; (B) an owner-ratified declaration that web is inapplicable; or (C) reject a binding that cannot carry one descriptor across those tiers? I9 makes A the default; an exception must be named and ratified.

## Sources

### Repository sources

- [`AGENTS.md` invariants](../../AGENTS.md#L29-L37)
- [`D-FFI-CPP1=A`](../spec/syntax-decisions.md#L3248-L3258)
- [Unified FFI frame](../spec/spec.md#L1340-L1354)
- [Four project arrangements](../proposals/ffi-owned-source-and-boundaries.md#four-project-arrangements-four-complete-experiences)
- [C++ fixture header](../../tests/fixtures/mixed_repo/cpp/counter.hpp)
- [C++ fixture implementation](../../tests/fixtures/mixed_repo/cpp/counter.cpp)
- [Jet fixture consumer](../../tests/fixtures/mixed_repo/cpp/run.jet)

### Primary external sources

- [Zig language reference: C translation CLI](https://ziglang.org/documentation/master/#C-Translation-CLI)
- [Swift: Mixing Swift and C++](https://www.swift.org/documentation/cxx-interop/)
- [Swift: supported features and constraints](https://www.swift.org/documentation/cxx-interop/status)
- [Carbon: interoperability philosophy and goals](https://raw.githubusercontent.com/carbon-language/carbon-lang/trunk/docs/design/interoperability/philosophy_and_goals.md)
- [CXX: `extern "C++"`](https://cxx.rs/extern-c%2B%2B.html)
- [Itanium C++ ABI](https://itanium-cxx-abi.github.io/cxx-abi/abi.html)
- [Itanium C++ exception-handling ABI](https://itanium-cxx-abi.github.io/cxx-abi/abi-eh.html)
