# Rust struct and `impl` placement in real projects

**Date:** 2026-07-24  
**Scope:** Where inherent methods (`impl Type { … }`) live relative to `struct` / `enum` / `union` definitions in widely used Rust crates, plus theory and community norms.  
**Not in scope:** Trait-only APIs, pure macro crates with almost no inherent methods.

## Verdict

In practice, Rust projects **colocate** a type and its inherent `impl` in the **same file**, usually **right under** the type. That is the default.

They **split** `impl` blocks across files only for clear reasons: feature gates, platform backends, generated code, or a type that grew too large. That is uncommon once you ignore name collisions.

Rust’s syntax keeps **data** (`struct`) and **methods** (`impl`) apart. That is not the same as keeping them far apart in the tree. Most teams still put them next to each other, which is close to the C++ habit of “members first, then methods,” even though Rust forbids methods *inside* the `struct` item.

---

## Method

### Corpus

Scanned published crate sources from the local Cargo registry, plus:

- `library/core`, `library/alloc`, `library/std` from a shallow clone of `rust-lang/rust`
- Jet’s own `crates/` and `Source/` trees (for a large in-house compiler)

Popular crates included `tokio`, `serde`, `clap_builder`, `axum`, `hyper`, `reqwest`, `rustls`, `hashbrown`, `rayon`, `syn`, `wasmtime`, `cranelift-codegen`, and many smaller libs (`bytes`, `http`, `chrono`, `indexmap`, …).

### What we counted

For each **unique type name** in a crate (one definition of that simple name):

1. **Same file** — every inherent `impl Type` lives in the type’s defining `.rs` file  
2. **Adjacent** — first inherent `impl` starts within **8 lines** of the type’s closing brace (or `;`)  
3. **Multi-file** — at least one inherent `impl` lives in another file  
4. **Multi-block** — more than one inherent `impl Type` block exists (same file or not)

We **skipped homonyms** (`Sender`, `Context`, `Client`, … defined in many modules). Counting those as “one type split across files” would be wrong and would inflate the multi-file rate.

The scanner is a brace-aware heuristic, not `rustc`. It is good enough for placement trends; it can miss macro-only types and can mis-read odd syntax.

Reproduce:

```sh
node docs/research/_scripts/analyze_struct_impl.mjs label=/path/to/crate …
```

---

## Empirical results

### Headline rates (unique type names)

| Slice | Types with inherent `impl` | Same file | Multi-file | Multi-block | Adjacent (of same-file) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Full corpus | 1765 | **91.8%** | 8.2% | 9.0% | 75.6% |
| Corpus without `syn` | 1629 | **97.6%** | 2.4% | 9.5% | 76.4% |
| App/library crates only (no std, no Jet, no `syn`, no cranelift) | 892 | **97.9%** | 2.1% | 10.2% | 70.8% |
| `core` + `alloc` + `std` | 208 | 93.8% | 6.3% | 23.6% | 72.3% |

**Read this as:** about **19 in 20** real types keep all inherent methods in the defining file. About **3 in 4** of those put the first `impl` immediately under the type.

### Per-crate snapshots (selected)

| Crate | Same file | Multi-file | Multi-block | Notes |
| --- | ---: | ---: | ---: | --- |
| `axum`, `clap_builder`, `rayon`, `chrono`, `serde`, `bytes`, … | 100% | 0% | low–moderate | Textbook colocation |
| `rustls` | 98.6% | 1.4% | 4.1% | Rare split: `ConfigBuilder` client/server |
| `tokio` | 94.8% | 5.2% | 11.1% | cfg / io_uring / loom helpers |
| `hashbrown` | 83.3% | 16.7% | 27.8% | `rustc_entry`, rayon, SIMD `Group` |
| `rust` `std` | 96.2% | 3.8% | 11.5% | OS / thread extensions |
| `syn` 2.x | 22.8% | 77.2% | 2.9% | **Outlier:** generated `src/gen/debug.rs` |

### What “not colocated” looks like when it is real

Real multi-file inherent `impl`s in this corpus fall into a few buckets:

1. **Generated companions** — `syn` keeps type defs in hand-written files and dumps large `Debug` / helper `impl`s into `src/gen/debug.rs`.  
2. **Feature or backend modules** — `hashbrown` rayon hooks; `rustls` ring vs aws-lc; SIMD `Group` backends.  
3. **Role split for one type** — `rustls::ConfigBuilder` methods in `client/builder.rs` and `server/builder.rs`.  
4. **cfg / platform / loom** — Tokio `FastRand` / `RngSeed` extras; std thread `Builder` scoped API.  
5. **File-size / concern split inside one module tree** — e.g. type in `inner.rs`, more methods in `inner/extract.rs` (`indexmap`).

Homonyms (many `Receiver` types) look like multi-file splits in a naive scan. After we drop them, Tokio’s multi-file rate falls from ~19% to ~5%.

### Same module vs same file

Rust’s privacy unit is the **module**, not the file. A child file can still write `impl ParentType` if the type is visible.

In the cleaned corpus, almost all same-crate splits that remain are **other directory** (feature/backend/gen), not “same folder, second file.” Same-dir splits exist but are rare.

So:

- **Same file** ≈ default community home for inherent methods  
- **Same module, other file** ≈ allowed, used when a file gets huge or a submodule owns a concern  
- **Other module / other dir** ≈ special case (features, OS, codegen)

---

## Community norms (what people say)

Authoritative style docs agree with the corpus:

| Source | Rule of thumb |
| --- | --- |
| [Canonical Rust best practices — ordering](https://canonical.github.io/rust-best-practices/ordering-discipline.html) | Put `impl SomeType` in the **same file**, **right under** the type. Trait `impl`s go with the trait or the type (crate-local “orphan” sense). |
| [PingCAP Rust style — traits](https://pingcap.github.io/style-guide/rust/traits.html) | Write `impl`s **directly after** their concrete types. Prefer one module; split only if size or clarity demands it. Multiple inherent `impl` blocks are OK to **group methods for rustdoc**. |
| [Clippy `multiple_inherent_impl`](https://github.com/rust-lang/rust-clippy/blob/master/clippy_lints/src/inherent_impl.rs) | Restriction lint: many inherent `impl` blocks make code **harder to navigate**. Default scope is whole crate; config can relax to file or module. |
| [users.rust-lang.org style thread](https://users.rust-lang.org/t/a-question-of-style-for-impl-of-structs/118029) | Default: one big inherent `impl`. Split for rustdoc sections, feature gates, or multi-file size — not for taste alone. |
| [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/predictability.html) | Prefer **inherent methods** when there is a clear receiver (`C-METHOD`); constructors are inherent (`C-CTOR`). Placement is left to module layout, but discoverability in rustdoc assumes methods hang off the type. |

**Community preference in one line:** colocate type + inherent methods; use extra `impl` blocks or files as a **scaling tool**, not as the default layout.

---

## Why Rust separates `struct` and `impl` (language design)

This is separate from “should they live in one file?”

### What the language forces

1. **`struct` is layout only** — fields, visibility, `repr`, niches. No method bodies inside the item.  
2. **`impl` is behavior** — inherent methods, or `impl Trait for Type`.  
3. **Many `impl` blocks are legal** — different `where` bounds, `cfg`, or rustdoc groupings.  
4. **Trait impls cannot live inside the struct** — and orphan/coherence rules care about *which crate* defines the impl, not which file.  
5. **Privacy is modular** — any module in the crate that can see the type may add inherent methods (subject to visibility). There is no C++-style “only members declared in the class body.”

### Design payoffs

| Payoff | Why the split helps |
| --- | --- |
| Clear data vs behavior | Readers can scan layout without method noise. |
| Trait system | `impl Trait for T` matches the same shape as inherent `impl T`. |
| Coherence / orphans | Impl location is a crate-level rule; files stay flexible. |
| cfg and features | Optional methods can sit in optional modules without rewriting the struct. |
| Generics | Separate `impl` blocks can carry different type/lifetime bounds. |
| Tooling | rustdoc groups by `impl` block; rust-analyzer jumps between type and impls. |

### Cost vs C++ “methods inside the class”

C++ (and Java/C#/Swift to varying degrees) lets you declare methods in the type body so “everything about `T`” is one syntactic nest.

Rust pays a small **locality tax** at the syntax level: you always jump from `struct` to `impl`. In return you get:

- one grammar for inherent and trait behavior  
- easy multi-block organization  
- no pressure to shove trait impls into the type item  

In real trees, teams cancel most of that tax by putting `impl` **immediately below** the type — the C++ reading order, without C++ nesting.

---

## Theory: colocate vs split

### When colocation (same file, adjacent) is better

- One mental object: “what is `T`, and what can I do with it?”  
- Reviews and refactors stay in one diff hunk.  
- rustdoc and “go to definition” match how people browse.  
- Matches Canonical / PingCAP / most crates in this study.

**Default for Jet and for most library code:** same file, `struct` then inherent `impl`, then important trait `impl`s.

### When splitting is better

| Reason | Example from corpus |
| --- | --- |
| Generated code | `syn` `gen/debug.rs` |
| Optional features | rayon impl modules on maps/sets |
| Backends / SIMD / crypto providers | `hashbrown` groups; `rustls` sign/ticketer |
| Public API roles | client vs server builder methods |
| File length | submodule next to `mod.rs` / parent file |
| rustdoc sections | several inherent `impl` blocks with docs (same file still preferred) |

### Tradeoffs of *not* colocating

**Costs**

- Navigation: “where is method `foo`?” needs search or IDE support.  
- Onboarding: new readers miss methods that live far from the type.  
- Clippy / style guides treat scatter as a smell unless justified.  
- Risk of accidental API surface growth in obscure modules.

**Benefits**

- Keeps the defining file readable when methods run to thousands of lines.  
- Isolates `cfg(feature = …)` and platform code.  
- Lets codegen own large, boring `impl`s.  
- Can mirror product seams (client/server, async/blocking) without duplicate types.

**Net:** split for **scale or seams**, not for ideology. The data say splits stay rare outside codegen-heavy crates.

---

## Practice vs your C++ preference

You prefer C++ style: fields first, then methods, still *inside* the type.

| Concern | C++ class body | Idiomatic Rust |
| --- | --- | --- |
| Field list alone | Possible but often mixed with methods | Always alone in `struct` |
| Methods after fields | Same syntactic nest | Same **file**, next item (`impl`) |
| Methods in other files | Out-of-line defs; still declared in class | Inherent `impl` elsewhere in crate; **no** central declaration list |
| Interfaces / traits | Base classes, free functions, ADL | Separate `impl Trait for Type` |
| “Declare then define” | Headers vs `.cpp` | Modules + privacy; often no split at all |

**In practice for Rust:** your preferred *reading order* is what most crates already do. The language will not put method bodies inside `struct`. Fighting that (e.g. huge macros that fake nested methods) is non-idiomatic and hurts tools.

**In theory:** separate `struct` / `impl` is the better fit for Rust’s trait and coherence model. Colocated *files* give you the C++ locality benefit without giving up that model.

**Which is “better”?**

- **Better in Rust practice:** same-file colocation (empirically dominant; style guides agree).  
- **Better in Rust theory:** keep `struct` and `impl` as distinct items; allow multiple `impl`s; do not demand class-nested methods.  
- **Better match to your taste without fighting Rust:** treat “`struct` + following `impl` in one file” as the unit of design, the way a C++ class body is the unit in C++.

---

## Recommendations

1. **Default:** one file owns the type. Order: type → inherent `impl` → major trait `impl`s → helpers.  
2. **Prefer one inherent `impl` block** unless you want rustdoc groupings or `cfg` sections.  
3. **Split files** only for features, platforms, generated code, or true size pain. Document the seam in the module docs.  
4. **Do not** scatter inherent methods across random modules for “clean architecture” alone.  
5. **Do not** try to embed method bodies inside `struct` via macros to mimic C++. Use adjacent `impl` instead.  
6. Enable or consult Clippy’s `multiple_inherent_impl` if a crate starts to sprawl without a policy.

---

## Limits of this study

- Heuristic parser; not rustc.  
- Inherent methods only; trait `impl` placement is a related but different question (often “with the type” or “with the trait”).  
- Homonym filter removes false multi-file hits but also drops any *real* split that shares a popular name with another type in the same crate.  
- `syn` skews multi-file upward because of codegen; exclude it when you want “hand-written app/library” norms (~98% same file).  
- Jet’s full tree was noisy when `target/` / `build/` leaked in; figures above use `crates/` and `Source/` only.

---

## Sources

- Corpus scan script: `docs/research/_scripts/analyze_struct_impl.mjs`  
- Canonical: [Ordering discipline](https://canonical.github.io/rust-best-practices/ordering-discipline.html)  
- PingCAP: [Implementing traits](https://pingcap.github.io/style-guide/rust/traits.html)  
- Clippy: [`multiple_inherent_impl`](https://github.com/rust-lang/rust-clippy/blob/master/clippy_lints/src/inherent_impl.rs)  
- Rust API Guidelines: [Predictability](https://rust-lang.github.io/api-guidelines/predictability.html) (`C-METHOD`, `C-CTOR`)  
- Forum: [Style for impl of structs](https://users.rust-lang.org/t/a-question-of-style-for-impl-of-structs/118029)  
- Coherence / orphans (why impls are crate-scoped, not class-scoped): [RFC 2451](https://github.com/rust-lang/rfcs/blob/master/text/2451-re-rebalancing-coherence.md), [Little Orphan Impls](https://smallcultfollowing.com/babysteps/blog/2015/01/14/little-orphan-impls/)
