# views

## Ratified

- **D-MEM1 / D-MEM-PARAM1=A** — unmarked parameters are reads, `&T` is exclusive write, `^T` is take, and `~` is copy. Raw `&T` reference returns/fields remain deleted; safe stored/returned views use named `View<T>`/`ViewMut<T>`. — `docs/spec/syntax-decisions.md:2189-2204`
- **D-SHAPE-PLACE1=A** — a place has a maximal field/index/range projection; bare access is a checked read window, `&place` is the exclusive write window, and `~place` is independent owned storage. Many reads may overlap; a write must be exclusive; move/resize is rejected while a live window could be invalidated. `.view()` is superseded. — `docs/spec/syntax-decisions.md:2276-2284`
- **D-MEM-VIEWRET1=B** — `View<T>` and `ViewMut<T>` may cross return/field boundaries, with public source provenance; sema proves the owner outlives each view and keeps at most one mutable view live. It “does not revive raw `&T` returns or fields,” and no lifetime syntax is added. — `docs/spec/syntax-decisions.md:2286-2295`
- **D-MEMPROVENANCE2=A / D-MEMPROVENANCE3=A** — inferred provenance remains the beginner default; an expert may use trailing `from` to name owners, and every return path's inferred owners must be a subset of the declared set, which is published to callers. — `tower:D-MEMPROVENANCE2`; `docs/spec/syntax-decisions.md:2297-2309`
- **D-MEM-COPYSEM1=A** — a read-only view entering a non-view destination now means an owned copy “with the same lowering as an explicit `~` store” on every tier. Declared `View<T>`/`ViewMut<T>` fields/returns are untouched; `ViewMut` into a non-view slot remains an error. — `docs/spec/syntax-decisions.md:7637-7637`
- **D-PIN1=A / D-PIN2=A / D-PIN3=A** — `mem.pin(&place) -> Pin<T>` is the address-stability contract; `Pin<T>` is a tracked write window, and pinned storage cannot move, replace, or resize while live. Pin reuses the owner/provenance graph and has structural `Pin<U>` projection. — `docs/spec/syntax-decisions.md:6695-6703`
- **D-CONC-SHARE1=A** — `shared expr` constructs the shared cell, ordinary field access is used, each statement is one atomic step, and multi-step changes commit under `#Transact`; crossing safety remains checked. — `docs/spec/syntax-decisions.md:2425-2431`
- **D-COMPUTE-TYPE1=D** — `Tensor<T>` owns ranked multidimensional storage; `View<T>` is the “sole borrowed strided projection” for host/device memory, and `Vec<N>`/`Matrix<M,N>` share the substrate and cross zero-copy. — `docs/spec/syntax-decisions.md:5889-5894`

## Shipped

- Ordinary range/place view, copy, and mutable-write behavior is exercised by `examples/features/tooling/compute_views.jet`: `~tensor[1..2]` copies, while `&tensor[1..2]` writes through to the owner.
- `String.trim()`, `.after(sep)`, and `.before(sep)` bound to a local return zero-copy string views; crossing a return/field boundary uses named `View<str>` provenance. — `docs/spec/syntax-decisions.md:2228-2233`
- `core.text` exposes Unicode algorithms. `graphemes`, `words`, and `sentences` return `[String]`; `byte_count`, `scalar_count`, and `scalars` keep UTF-8 bytes and scalar values distinct. — `docs/reference/core-library.md:2187-2213`
- `core.mem` arenas return scope-bound zero-copy views; escaping them is E0631 and use after reset/close is E0632. Copying with `~x` is the explicit escape. — `docs/reference/core-library.md:3477-3513`
- `mem.pin(&place)` and its no-move behavior are documented and have the flagship proof `examples/features/memory/pin.jet`. — `docs/spec/syntax-decisions.md:6695-6705`
- `JetTensor` stores `shape`, `strides`, and `data: Arc<Vec<f64>>`; `JetComputeViewMut` retains a mutable owner window and exclusivity flag. — `crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs:4013-4032`
- Task/channel crossing rejects `View<T>` and string-view windows; crossing values must be sendable and owned. — `docs/reference/core-library.md:2979-2987`
- The built-module inventory includes `core.files` but no `core.files.mmap`; it does include `core.mem`, `core.compute`, and `core.plugin`. — `docs/reference/core-library.md:4281-4296`

## Undecided

- Whether `core.text` and byte APIs should expose an explicit zero-copy `View`/slice surface instead of (or in addition to) owned `[String]` results, including Unicode-boundary, mutation, and lifetime rules.
- Whether `core.files.mmap` should exist, and if so how mapping ownership, file growth, unmapping, authority, and `View<T>`/`Pin<T>` provenance are checked.
- What complete public contract covers strided views across host/device transfers, non-contiguous writes, and alias/race proofs beyond D-COMPUTE-TYPE1's single `View<T>` substrate.
- Whether tensor view storage must support dtypes or owners beyond the shipped `Arc<Vec<f64>>` representation without creating a second view model.

## Conflicts

- D-MEM1 deletes raw `&T` stored/returned references. A proposal for raw borrow fields/returns or a lifetime-syntax layer conflicts with the current memory model; use `View<T>`/`ViewMut<T>` with provenance.
- D-MEM-COPYSEM1 makes read-view-to-non-view storage an owned copy. A “zero-copy assignment” ballot for those destinations would directly contradict the ruling; only a declared view boundary can retain a view.
- D-SHAPE-PLACE1 gives `&place` exclusive write and `~place` owned storage; `.view()` is retired. Do not introduce another place/view sigil or silently permit overlapping writes.
- Arena views cannot escape their region (E0631) or survive reset/close (E0632). `mem.pin` stabilizes one place but does not turn an arena view into an immortal or freely sendable value.
- Task/channel laws reject `View<T>` and string-view windows at crossings. A view-based shared/IPC proposal must first define ownership and sendability instead of assuming current channels can carry borrowed windows.
- D-COMPUTE-TYPE1 names `View<T>` as the sole borrowed strided projection. A second tensor-specific borrow/view mechanism would fragment the ratified substrate.
