# ffi

## Ratified

- **S50 / D-FFI-CAP1** — Rust uses `extern rust "crate@version" { fn name(args) T = "rust::path" }`; version pins are required and “the by-value boundary is the default floor.” Checked `&T` exclusive lends, `^T` ownership transfers, and returned handles attachable to `#Close(fn)` are allowed; callbacks and trait objects remain outside this tier. — `docs/spec/syntax-decisions.md:3078-3083`
- **S59 / D-SHAPE-CASE2=A** — C uses generated bindings plus an optional overlay, “by-value first, pointers only inside S58.” `#Bindgen module c.<lib>.__bindgen__` and `#Extern module c.<lib>` are the binding surfaces; all foreign namespaces use the ordinary member-list form and there is “no FFI-only import grammar.” — `docs/spec/syntax-decisions.md:3085-3097`
- **D-SHAPE-CASE2=A** — binding modules are exempt from Jet casing checks, while call sites preserve foreign spelling. Link declarations are `<lib>: c@system` or `c@"vendor/path"` in `package.jet` `deps`; pkg-config fallback ends in E3201, and C dependencies are link dependencies, not packages. `jet inspect bind` uses a native std-only C-prototype parser and binds scalars, `char*`↔String, and `#define` constants. — `docs/spec/syntax-decisions.md:3099-3111`
- **D-CABI-CALLBACK1=A / D-CABI-RESULT1=C / D-CABI-PLATFORM1=A** — C callbacks must be C-convention, C-safe, non-null, monomorphic, explicitly `-[]>` or capture-free with an empty effect row; they must be safe for foreign threads, with no heap allocation, mutable static/TLS state, scheduler access, or panic-capable path. The pointer is stable for program lifetime and may be concurrent/reentrant; unsupported cases are E3203. — `docs/spec/syntax-decisions.md:3113-3120`
- **D-FFI-INLINE1=A** — `#FFI(<lang>) fn` has one triple-quoted raw foreign-source body; bytes pass exactly as written, sema checks every call site, and unsafe-language bodies additionally require an enclosing `#Unsafe("reason")`. — `docs/spec/syntax-decisions.md:3135-3147`
- **D-FFI-CPP1=A** — `cpp.*` is full-depth clang binding: classes become opaque `#SingleUse` owned handles with consuming cleanup, methods are ordinary Jet methods, exceptions surface as `T !CppError`, and the effect is `-[FFI.Cpp]>`. — `docs/spec/syntax-decisions.md:3165-3175`
- **D-FFI-UNIFY1** — every language mounts as `<lang>.<lib>` with script, project, and overlay tiers; `jet inspect bind <lang>` emits inspectable bindings; generated bindings are safe wrappers; raw foreign symbols outside a binding require `#Unsafe("reason")`; binders report Jet diagnostics; any `<lang>.<lib>` can be shadowed in situ without changing call sites. — `docs/spec/syntax-decisions.md:3255-3270`
- **D-AUTHORITY-ROOTS1=A** — `FFI` is one grantable authority root with language leaves such as `FFI.Cpp` and `FFI.Py`; flat language roots are deleted. — `docs/spec/syntax-decisions.md:6999-7015`
- **D-FFI-CAP1=A** — `&` means “exclusive access for exactly this call”; foreign code may read/write through it but must not retain it and no copy occurs. `^` transfers ownership and kills the Jet name. `#Close(fn)` joins `close(^)` and runs exactly once or the program does not compile. The proposed `extern c "lib" { … }` block is explicitly “proposed grammar, not current surface”; integration is through D-FFI-UNIFY1. — `docs/spec/syntax-decisions.md:7643-7643`

## Shipped

- `examples/features/lowlevel/ffi.jet` demonstrates version-pinned `extern rust`; `examples/features/lowlevel/inline_c.jet` demonstrates `#Unsafe` plus `#FFI(c)`; `examples/features/lowlevel/cbind/run.jet` demonstrates C binding, `char*`/String, and scalar calls; `examples/features/lowlevel/polyglot_go/run.jet` demonstrates a language binder handle.
- `jet inspect bind` and the C binding shape are documented as the native std-only parser path; C linking is declared through `package.jet` `deps` with `c@system` or a vendor path. — `docs/spec/syntax-decisions.md:3099-3111`
- `#Close` is a registered marker and joins the ownership protocol; `#ABI` and `#Close` are present in the syntax marker registry. — `crates/jet-foundation/src/Syntax/markers.rs:84-95`
- `#Import(c)` / `#Export(c)` are registered guest-boundary forms for a matching C ABI edge. — `crates/jet-foundation/src/Syntax.rs:337-341`

## Undecided

- What exact `jet inspect bind` and generated-binding contract should do for opaque C pointer handles (the E3208 probe case): which ownership shape is emitted, how `#SingleUse`/`#Close` is selected, and how incomplete headers are diagnosed.
- What native-link declaration contract covers `@nixpkgs` attributes and the E3210 failure path beyond the ratified `c@system` / `c@"vendor/path"` and E3201 fallback.
- How an FFI authority grant is written and checked at a package boundary (`allow: [FFI]` / E1803), including whether binder-generated calls record language leaves or only the parent root.
- Which `*Int` pointer forms are accepted across a C boundary (E3202), and whether C++/other language binders share that exact scalar-pointer rule.
- Whether the unified binder surface needs a ratified opaque-handle, callback, or resource ABI versioning rule beyond the current C ABI and C++ cleanup laws.

## Conflicts

- D-FFI-CAP1 already grants checked `&`/`^`/`#Close` semantics; a ballot proposing a second ownership or close convention would reopen a ratified ruling.
- D-FFI-UNIFY1 requires one `<lang>.<lib>` structure and says raw symbols outside a binding require `#Unsafe`; a new parallel `extern c` grammar or safe raw-pointer escape is forbidden by the current law.
- S50 and S59 are by-value-first floors; pointers are restricted to the unsafe tier, and Rust callbacks/trait objects remain outside S50. Do not turn those exclusions into an implicit new safe surface.
- D-CABI-* fixes the C callback safety/ABI contract, including concurrent or reentrant invocation; a ballot that merely repeats those callback guarantees is an implementation card, not a new primitive.
- D-FFI-CPP1 already ratifies full-depth C++ binding, opaque `#SingleUse` cleanup, exceptions, templates, and `FFI.Cpp`; a basic-only C++ proposal would conflict with it.
- Link deps are not packages, and missing native resolution is a typed failure (E3201); native linking must not silently become ambient package discovery. — `docs/spec/syntax-decisions.md:3107-3111`
