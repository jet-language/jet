# FFI / interop program (Epoch 4)

**Status:** planning. Covers Tower cards **#180** (Next-generation FFI — the
frame), **#124** (JS/npm + Swift interop — P0), and cross-references **#5**
(`plugin` target, whose plan lives in `../../sidequests/plugin-target.md`).
These three are one coherent program: #180 ratified the shared FFI *law*; #124
is the JS and Swift *instances* of that law; #5 is the WASM substrate several of
the binders reuse. This file makes each implementable the day its per-language
ballots ratify.

Owner bar (card #180 body, 2026-07-02): it must be **almost trivial to use
almost any other language as a wrapper/binding**, so in-situ replacements are
easy and Jet taps *all* developments of other ecosystems immediately.

---

## 1. The frame is already law — do not re-litigate it

`D-FFI-UNIFY1=A` is ratified (syntax-decisions.md, "FFI & external
dependencies"). Every foreign language mounts the **same** structure; the
per-language ballots below choose only *how deep the binder goes*, never the
surface:

- **Namespace.** Each language is `<lang>.<lib>` — `c.raylib`, `rust.serde`,
  `py.h5instrument`, `js.plotly`, `swift.Alamofire`.
- **Three tiers** (S59 generalized):
  - *script* — `use "xxhash.h" as xx` / `use "lodash" as _` — bind on first
    compile;
  - *project* — `use py.h5instrument as h5`, dep pinned in `pkg.jet` as
    `<lib>: <lang>@"ref"`;
  - *overlay* — `#Extern module py.h5instrument { … }` refines generated types,
    overlay wins.
- **`jet bind <lang>`** is the per-language binder. It writes inspectable,
  committable bindings to `.jet/bindings/<lang>/<lib>.jet`. Existing binders:
  `Source`→`crates/jet-driver/src/CBind.rs` (C, native std-only prototype
  parser), `crates/jet-driver/src/FFI.rs` (`extern rust`, materializes a cached
  cargo project under `~/.cache/jet/ffi/` and links the rlib). Every new
  language adds a binder in this shape.
- **Safe by construction (I1).** Generated bindings are safe wrappers;
  marshaling internals are compiler-vetted like std internals. Calling a foreign
  symbol *outside* a binding needs `#Unsafe("reason")`. No option below spends
  safety as its default.
- **Diagnostics are Jet diagnostics (I2/I4).** No foreign toolchain error
  (pip, npm, swiftc, cargo) ever reaches the user unlaundered — every binder
  failure gets a Jet E-code + `tests/ui` snapshot.
- **In-situ replacement is structural.** Any `<lang>.<lib>` is shadowed by a
  Jet package exporting the same surface — change one `deps:` line, call sites
  never move. This is the "swap plotly.js for `core.plot` next year" promise.

The three follow-up ballots are **D-FFI-PY1**, **D-FFI-JS1**, **D-FFI-SWIFT1**
(full text carried on cards #180/#124 for owner decision). Implementation of
any language waits on its ballot; the shared binder scaffolding below can start
against the ratified frame today.

---

## 2. Shared binder scaffolding (frame-only, unblocked now)

Independent of the per-language depth picks, build the common substrate all
binders sit on. This is the #180 implementation core.

1. **`Language` enum + namespace routing.** Generalize the C-only path so
   `<lang>.<lib>` imports route to a registered binder. Today C and rust are
   special-cased across `crates/jet-driver/src/{CBind,FFI}.rs`,
   `crates/jet-sema/src/Sema/FFI.rs`, `crates/jet-codegen/src/Codegen/Imports.rs`.
   Introduce a `ForeignBinder` trait seam: `probe(ref) -> BindPlan`,
   `generate(BindPlan) -> bindings.jet`, `link(BindPlan) -> LinkArtifact`.
   C and rust become the first two `impl`s (no behavior change — S59/S50
   preserved).
2. **`.jet/bindings/<lang>/` cache + provenance.** Extend the existing
   `.jet/bindings/c/` cache to a per-language tree; record the resolved ref +
   content hash in `.jet/lock` (D-DEP1 vendoring/hash-pinning law, already
   applies to every language's refs).
3. **`use <lang>.<lib>` grammar.** Parser already accepts `use c.raylib` and
   `use "header.h" as x`; widen the `<lang>` head to the registered set and
   emit E32xx-family "unknown foreign language / no binder" for unregistered
   ones. Formatter round-trip + fmt STABILITY test (mandatory for new syntax).
4. **Overlay merge.** `#Extern module <lang>.<lib> { … }` already parses for C;
   make the merge (bindgen ∪ overlay, overlay wins) language-generic.
5. **Diagnostic family.** Reserve an E-code block for foreign-binder failures
   (bind failure, ref-not-found, type-not-marshalable, ambiguous overlay,
   missing-toolchain) shared across languages; each language specializes the
   *fix* text. Snapshots per language.

Deliverables: examples under `examples/features/interop/` (one per language as
its binder lands), golden-enforced (I5).

---

## 3. Python — `D-FFI-PY1` (see ballot on card #180)

**Use case (frame story).** A lab-data tool: the only mature reader for an
instrument format is a Python package; the best numeric stack (numpy/scipy) is
Python. "This afternoon", behind a typed Jet API with honest effects.

**Depth question.** Python's ecosystem is C-extension-heavy (numpy, torch), so
the binder must reach real CPython, not a reimplementation. The axis is
**in-process embed (zero-copy, weaker crash isolation)** vs **supervised sidecar
broker (crash-isolated, marshaled boundary)** vs **Python-on-WASM sandbox** vs
**shallow script shim**. Recommended: a **sidecar-broker default with an
opt-in embed tier** — safe supervised CPython worker is the default surface
(`#(Py)` effect; a crashing native extension takes down the worker, not your
program), and `py@embed` (manifest/marker) switches hot paths to in-process
`libpython` for zero-copy on buffer-protocol arrays. One `use py.X` surface;
the host tier never changes call sites. This mirrors D-PLUGIN1's shape (sandbox
default, native the expert opt-in).

**Marshalling.** scalars ↔ Int/Float/Bool; `str` ↔ String (UTF-8); `bytes`,
buffer-protocol arrays (numpy) ↔ `[T]` / zero-copy `View<T>` in embed, copied
in broker; `dict` ↔ typed struct or `Map`; `list`/`tuple` ↔ `[T]`; Python
exceptions → Jet `Result` + E-code (never a raw traceback). `None` ↔ optional.

**Async.** Python `asyncio` coroutine → Jet async task; `await py.fn(...)`
drives the worker/embed event loop.

**Distribution.** `use py.h5instrument` resolves the PyPI ref pinned in
`pkg.jet` (`h5instrument: py@"==2.1"`); jetpack fetches the wheel, vendors +
hash-pins (D-DEP1). The **CPython interpreter/`libpython` is a native runtime
dependency** — provisioned by the jetpack core provider / nixpkgs (interim
per the ratified native-deps policy), never vendored into `Source/` (I6).

**Safety (I1).** User code safe unless `#Unsafe`. Broker gives process-level
crash containment; embed's `libpython` linkage + GIL management is
compiler-vetted marshaling (std-internal tier). Exceptions and interpreter
errors laundered to Jet E-codes (I2/I4).

**Owner gate raised → `D-DEP-PY1`.** Embedding `libpython` and/or bundling a
CPython runtime is a new *runtime-side* external dependency — an I6 owner
decision, exactly parallel to `D-DEP-WASM1`. Ballot below.

**Implementation once D-FFI-PY1 + D-DEP-PY1 ratify.**
1. `PyBinder impl ForeignBinder`: probe a PyPI ref → introspect the installed
   package (module `__all__` + type stubs where present) → emit
   `.jet/bindings/py/<lib>.jet` typed wrappers.
2. Broker: a supervised worker process (reuse the jetpack `services`
   supervision seam, U12) with a typed length-prefixed message boundary;
   marshal per the table above.
3. Embed tier (`py@embed`): link `libpython`, GIL-scoped call wrappers,
   buffer-protocol zero-copy for `View<T>`.
4. `#(Py)` effect (amend the D-EFF4 set — this is an owner-visible effect
   addition; confirm in the D-FFI-PY1 ballot).
5. Example `examples/features/interop/python-h5.jet` + golden; E-code snapshots.

---

## 4. JS / npm — `D-FFI-JS1` (see ballot on card #124) — **P0**

**Two hosts, one surface.** JS is unique because Jet already has two JS
contexts:
- **Web target** (`D-WEBBACKEND1=A`): the browser *is* the JS engine. `use
  js.plotly` in a web module binds an npm package that runs in the browser JS
  host; the JS/WASM boundary types are fixed by `D-JSBIND1=A` (scalars, String,
  lists/maps of ABI-safe values, Codable structs/enums; closures/resources
  rejected unless wrapped), the WASM triple by `D-WEBKIND1=A`
  (`wasm32-unknown-unknown` + generated JS loader), the partition by
  `D-WASM1=A` (effect inference + `#Target`).
- **Native target** (CLI/server): there is no ambient JS engine. To let a
  native Jet binary call an npm package, mount JS **as a WASM component on the
  already-approved wasmtime runtime** (`D-DEP-WASM1=A`, the #5 substrate) via a
  QuickJS-in-wasm / componentize-js interpreter. Sandboxed, I1-clean, **zero
  new runtime dependency** (reuses wasmtime). A Node-subprocess broker is the
  opt-in tier for packages needing full Node APIs / native addons.

Recommended D-FFI-JS1: **target-dispatched JS host** (browser on web, JS-on-
wasm on native, Node-broker opt-in), typed via the `jet bind js` stub path.
One `use js.X` surface; the host is chosen by the compile target, call sites
never change.

**Typed surface + the D-NPMTYPE1 reconciliation (key sub-decision).**
`D-NPMTYPE1=A` set the floor: typed npm surfaces are **first-party hand-authored
Jet stub packages**, *no `.d.ts` parsing*. The D-FFI-UNIFY1 frame promotes
`jet bind js` to a real binder that emits `.jet/bindings/js/<lib>.jet`. These
compose but the depth ballot must state the reconciliation explicitly:
**`jet bind js` generates a committable, Jet-checked stub from the package's
`.d.ts` (inspectable binder output, not runtime `.d.ts` semantics), superseding
D-NPMTYPE1's hand-authored-only floor** — this is what "tap ALL npm
immediately" requires; long-tail packages with no `.d.ts` fall back to a
`#Unsafe`-gated dynamic `Any` surface (D-NPMTYPE1 option C, demoted to the
opt-in escape hatch, never the default). D-FFI-JS1 decides this; flag as the
one place the frame amends a prior ratification.

**Marshalling.** Governed by `D-JSBIND1=A` on both hosts: number ↔ Int/Float,
string ↔ String, array ↔ `[T]`, object ↔ Codable struct / `Map`; JS `throw` →
Jet `Result` + E-code; **JS `Promise` ↔ Jet async** (`await js.fn(...)`);
Jet closure → JS callback only via the explicit wrap D-JSBIND1 requires.

**Distribution.** `plotly: js@"plotly.js@2"` in `pkg.jet`; jetpack fetches from
the npm registry, vendors + hash-pins (D-DEP1). Yes — `use js.lodash` fetches
from npm via jetpack, with npm as a registry provider (see `D-JPK-EXTPROV1`
gate). The JS-on-wasm runtime is wasmtime (already approved); no new I6 dep for
the native path. The browser path adds no runtime.

**Safety (I1).** Web: the browser sandbox + D-JSBIND1 closed ABI. Native: the
wasmtime sandbox (I1-clean by construction, same as plugins). Node-broker tier
is process-isolated. User code safe unless `#Unsafe`; npm/registry errors
laundered to Jet E-codes.

**Sequencing (honors D-JSWIFTFFI1=A: JS first).** Web-host binding rides
D-WEBBACKEND1 implementation; native JS-on-wasm rides the #5 wasmtime embed.
Both are unblocked at the ballot level.

**Implementation once D-FFI-JS1 ratifies.**
1. `JsBinder impl ForeignBinder`: resolve npm ref → `jet bind js` emits typed
   `.jet/bindings/js/<lib>.jet` (from `.d.ts` where present, else `Any`
   fallback surface).
2. Web host: wire the stub to the D-WEBBACKEND1 JS emitter (the npm package
   loads in the browser; calls cross the D-JSBIND1 boundary).
3. Native host: QuickJS/componentize-js as a wasm component on the #5 wasmtime
   `PluginHost`; marshal per D-JSBIND1.
4. Node-broker opt-in tier (reuse the Python broker transport).
5. Examples `examples/features/interop/{web-plotly,native-lodash}.jet` +
   golden; E-code snapshots.

---

## 5. Swift — `D-FFI-SWIFT1` (see ballot on card #124)

**Use case.** Call platform Swift libraries (Foundation, AppKit, networking)
from a native macOS/iOS Jet app — the `#122` native-UI story. Because these are
*platform framework* libraries, WASM is out (loses the frameworks); the real
Swift runtime is required.

**Depth question.** `D-JSWIFTFFI1=A` fixed the transport: **Swift via the
C-ABI** (Swift's own ABI is unstable; `@_cdecl` is the only stable seam). The
depth axis is how much projection sits on top of that seam: **hand-exported
C-ABI only (shallow)** vs **swift-bridge-style generated bridge (projects
classes/String/Array/errors, auto-emits the `@_cdecl` shims)** vs **full
Clang-importer projection of `.swiftinterface`**. Recommended:
**swift-bridge-style generated projection over the C-ABI seam** — `jet bind
swift` runs `swiftc` to emit `@_cdecl` shims for the declared surface and
generates typed Jet wrappers; the C-ABI is the stable transport underneath
(honoring D-JSWIFTFFI1), the projection is the "feels native" depth the frame
wants for static languages. Full Clang-importer is the aspirational end-state,
deferred.

**Marshalling.** Swift `String` ↔ String (UTF-8), `Array<T>` ↔ `[T]`,
`Optional<T>` ↔ optional, structs by-value, **classes/actors as opaque
ref-counted handles** (retain/release managed by the binder — I1-vetted),
Swift `throws` → Jet `Result`, Swift `async` → Jet async via `@_cdecl`
continuation shims. No raw pointer crosses outside `#Unsafe`.

**Async.** Swift `async` bridged through generated continuation shims to Jet
async tasks.

**Distribution.** `Alamofire: swift@"5.8"` in `pkg.jet`; jetpack fetches the
SwiftPM/git ref, vendors + hash-pins (D-DEP1). `swiftc` is a **build-time
native toolchain** (like the C compiler for S59) — provisioned via
jetpack/nixpkgs/Xcode; not an I6 *compiler* dep, so no new I6 ballot, but its
registry fetch policy rides `D-JPK-EXTPROV1`.

**Safety (I1).** User code safe unless `#Unsafe`; handle lifetimes + marshaling
compiler-vetted; `swiftc`/linker errors laundered to Jet E-codes (I2/I4).

**Sequencing.** `D-JSWIFTFFI1=A` gated Swift on the native backend
(`D-NATIVEUI1`, now ratified=A). Platform-only (macOS/iOS + Linux-Swift); on
other platforms `use swift.X` gives an honest gated E-code, never a raw
toolchain failure. Swift lands **after** JS (D-JSWIFTFFI1 sequencing) and after
the #122 native backend implementation.

**Implementation once D-FFI-SWIFT1 ratifies + native backend exists.**
1. `SwiftBinder impl ForeignBinder`: from a bridge description (or `pub`
   surface of a declared Swift module), `swiftc`-emit `@_cdecl` shims →
   compile to a static lib → generate `.jet/bindings/swift/<lib>.jet` wrappers
   over the resulting C ABI (reuse the S59 C-binding lowering).
2. Handle lifetime shims (retain/release), error/async continuation shims.
3. Platform gating diagnostic.
4. Example `examples/features/interop/swift-foundation.jet` (macOS lane) +
   golden; E-code snapshots.

---

## 6. #124 plan vet — contradictions with the ratified frame

Card #124's stored plan (`D-JSWIFTFFI1=A`, pre-dating D-FFI-UNIFY1) is **stale
on surface, sound on sequence**. Corrections this program supersedes:

| #124 stored plan | D-FFI-UNIFY1 frame — corrected |
|---|---|
| `import npm:"pkg"` syntax | **Wrong surface.** Canonical is `use js.<lib>` in the `<lang>.<lib>` namespace; `import npm:"…"` was never ratified and contradicts the frame. Kill it. |
| "JS interop = a web-target import protocol, not a separate FFI mechanism" | **Superseded.** JS is now one instance of the *one* FFI structure. It is not web-only: the native JS-on-wasm host (§4) extends it to CLI/server. |
| "Type stubs hand-authored like @types/, raise D-NPMTYPE1" | D-NPMTYPE1 ratified (=A, hand-authored floor). Frame promotes `jet bind js` to generate committable stubs; §4 reconciles (the one prior-ratification amendment D-FFI-JS1 must confirm). |
| "Swift via C-ABI deferred until D-NATIVEUI1 ships" | **Still correct.** D-NATIVEUI1 ratified=A; Swift stays sequenced after JS + native backend. No change. |
| Gated on D-WEBBACKEND1 + D-WASM1 | **Still correct** for the *web* JS host; the *native* JS host is additionally gated on the #5 wasmtime embed. Both now ratified at ballot level. |

Net: #124 keeps its P0 priority and its JS-first/Swift-later sequence, but its
surface is replaced by §4/§5 here. The `import npm:` spelling must not be
implemented.

---

## 7. Program sequencing

```
Phase 0 — frame scaffolding (unblocked now, §2)
  ForeignBinder trait seam; .jet/bindings/<lang>/ cache; use <lang>.<lib>
  grammar widen + formatter; generic overlay merge; shared E-code family.
  C + rust reslotted as the first two binders (no behavior change).

Phase 1 — JS (P0), rides D-WEBBACKEND1 + #5 wasmtime
  D-FFI-JS1 ratifies -> JsBinder; web host (browser) + native host (JS-on-wasm).
  Gate: D-JPK-EXTPROV1 (npm registry provider).

Phase 2 — Python
  D-FFI-PY1 + D-DEP-PY1 ratify -> PyBinder broker; embed tier; #(Py) effect.
  Gate: D-JPK-EXTPROV1 (PyPI provider), D-DEP-PY1 (runtime dep).

Phase 3 — Swift, after native backend (#122/D-NATIVEUI1 impl) + JS
  D-FFI-SWIFT1 ratifies -> SwiftBinder swift-bridge projection over C-ABI.
  Gate: D-JPK-EXTPROV1 (SwiftPM provider).
```

Edges: Phase 0 gates everything (the seam). JS before Python before Swift
follows D-JSWIFTFFI1's JS-first law and the native-backend dependency for
Swift. The npm/PyPI/SwiftPM registry-provider policy (`D-JPK-EXTPROV1`) is one
gate shared by all three distribution stories. `D-DEP-PY1` is Python-only.

---

## 8. Owner gates raised by this program (ballots)

| Ballot | Gate | Why it's an owner call |
|---|---|---|
| **D-FFI-PY1** | Python binder depth | per-language depth on the D-FFI-UNIFY1 frame |
| **D-FFI-JS1** | JS/npm binder depth + D-NPMTYPE1 reconciliation | per-language depth; amends a prior ratification |
| **D-FFI-SWIFT1** | Swift binder depth | per-language depth (C-ABI transport fixed) |
| **D-DEP-PY1** | embed `libpython` / bundle CPython = new runtime dep | I6 — mirrors D-DEP-WASM1 |
| **D-JPK-EXTPROV1** | npm / PyPI / SwiftPM as jetpack registry providers | network reach + trust + vendoring policy, like the Nix bridge (U16) |

(#5's own open gates — plugin export-surface and version handshake — are balloted
in `../../sidequests/plugin-target.md` as `D-PLUGIN-EXPORT1` /
`D-PLUGIN-VERSION1`.)
